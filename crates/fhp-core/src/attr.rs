//! The single HTML start-tag attribute grammar shared by the tokenizer, the
//! tree builder, and the encoding prescan.
//!
//! [`TagScanState`] tracks where a byte sits inside a tag body so callers can
//! decide whether a `>` ends the tag (it does not inside a quoted value) and
//! whether a trailing `/` is a self-closing flag (it is not inside an unquoted
//! value). [`iter_raw_attrs`] splits a tag's attribute region into raw
//! name/value pairs, [`AttrSplitter`] does the same for a tag that arrives in
//! pieces, and [`SeenAttributes`] implements first-wins duplicate detection
//! with ASCII case-insensitive names.

use std::collections::HashSet;
use std::collections::hash_map::RandomState;
use std::hash::{Hash, Hasher};
use std::mem::MaybeUninit;

use crate::is_html_whitespace;

/// Position of a byte inside a tag body, following the WHATWG tokenizer's
/// tag-name and attribute states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagScanState {
    /// Inside the tag name.
    Name,
    /// Between attributes (also after a `/` that is not followed by `>`).
    BeforeAttrName,
    /// Inside an attribute name.
    AttrName,
    /// Whitespace after an attribute name, before a possible `=`.
    AfterAttrName,
    /// After `=`, before the value starts.
    BeforeAttrValue,
    /// Inside an unquoted attribute value.
    Unquoted,
    /// Inside a value quoted with the given byte (`"` or `'`).
    Quoted(u8),
}

impl TagScanState {
    /// State at the first byte of the tag name, which is
    /// [`tag_name_offset`] bytes after the `<`. Inside the name, `=` and
    /// quotes are ordinary name bytes, for start and end tags alike.
    pub const START: Self = Self::Name;

    /// Advance the state by one byte.
    ///
    /// Never called with the `>` that ends the tag; callers stop at a `>` for
    /// which [`TagScanState::gt_is_literal`] is `false`.
    #[inline]
    pub fn step(self, byte: u8) -> Self {
        let ws = is_html_whitespace(byte);
        match self {
            Self::Name => {
                if ws || byte == b'/' {
                    Self::BeforeAttrName
                } else {
                    Self::Name
                }
            }
            Self::BeforeAttrName => {
                if ws || byte == b'/' {
                    Self::BeforeAttrName
                } else {
                    Self::AttrName
                }
            }
            Self::AttrName | Self::AfterAttrName => {
                if ws {
                    Self::AfterAttrName
                } else if byte == b'/' {
                    Self::BeforeAttrName
                } else if byte == b'=' {
                    Self::BeforeAttrValue
                } else {
                    Self::AttrName
                }
            }
            Self::BeforeAttrValue => {
                if ws {
                    Self::BeforeAttrValue
                } else if byte == b'"' || byte == b'\'' {
                    Self::Quoted(byte)
                } else {
                    Self::Unquoted
                }
            }
            Self::Unquoted => {
                if ws {
                    Self::BeforeAttrName
                } else {
                    Self::Unquoted
                }
            }
            Self::Quoted(quote) => {
                if byte == quote {
                    Self::BeforeAttrName
                } else {
                    Self::Quoted(quote)
                }
            }
        }
    }

    /// Fold [`TagScanState::step`] over `bytes`.
    #[inline]
    pub fn advance(self, bytes: &[u8]) -> Self {
        bytes.iter().fold(self, |state, &byte| state.step(byte))
    }

    /// Whether a `>` in this state is part of an attribute value rather than
    /// the end of the tag.
    #[inline]
    pub fn gt_is_literal(self) -> bool {
        matches!(self, Self::Quoted(_))
    }
}

/// Offset of the first tag-name byte in `markup`, which starts with the `<`
/// of a start tag (`1`) or the `</` of an end tag (`2`).
///
/// [`TagScanState::START`] applies from this offset, so an end tag's name is
/// scanned in the tag-name state rather than as an attribute name.
#[inline]
pub fn tag_name_offset(markup: &[u8]) -> usize {
    debug_assert_eq!(markup.first(), Some(&b'<'));
    if markup.get(1) == Some(&b'/') { 2 } else { 1 }
}

/// One attribute as it appears in the source, before entity decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawAttr<'a> {
    /// The attribute name exactly as written.
    pub name: &'a str,
    /// [`ascii_ci_fingerprint`] of `name`, computed while scanning it.
    pub fingerprint: u64,
    /// The raw (undecoded) value; `None` for a boolean attribute.
    pub value: Option<&'a str>,
}

/// Iterator over the attributes of a tag's attribute region.
///
/// Created by [`iter_raw_attrs`]. Duplicates are yielded as-is; callers that
/// need first-wins semantics filter them with [`SeenAttributes`].
#[derive(Clone, Debug)]
pub struct RawAttrs<'a> {
    region: &'a str,
    pos: usize,
}

/// Split the bytes between a tag name and its closing `>` (excluding a
/// self-closing `/`) into raw attributes.
#[inline]
pub fn iter_raw_attrs(region: &str) -> RawAttrs<'_> {
    RawAttrs { region, pos: 0 }
}

impl<'a> RawAttrs<'a> {
    #[inline]
    fn skip_whitespace(&mut self, bytes: &[u8]) {
        while self.pos < bytes.len() && is_html_whitespace(bytes[self.pos]) {
            self.pos += 1;
        }
    }

    /// Slice the region; every boundary passed here sits next to an ASCII
    /// delimiter or at the region end.
    #[inline]
    fn slice(&self, start: usize, end: usize) -> &'a str {
        debug_assert!(self.region.is_char_boundary(start) && self.region.is_char_boundary(end));
        // SAFETY: `start` and `end` are either the region bounds or positions
        // of ASCII delimiter bytes (whitespace, `=`, `/`, `>`, quotes), which
        // are always UTF-8 character boundaries.
        unsafe { self.region.get_unchecked(start..end) }
    }
}

impl<'a> Iterator for RawAttrs<'a> {
    type Item = RawAttr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.region.as_bytes();
        let end = bytes.len();
        loop {
            self.skip_whitespace(bytes);
            if self.pos >= end {
                return None;
            }

            // Attribute name. A leading `=` belongs to the name (WHATWG
            // "before attribute name state"), matching `TagScanState`.
            let name_start = self.pos;
            let mut fingerprint = ASCII_CI_FINGERPRINT_OFFSET;
            if bytes[self.pos] == b'=' {
                fingerprint = ascii_ci_fingerprint_step(fingerprint, b'=');
                self.pos += 1;
            }
            while self.pos < end && !is_attr_name_end(bytes[self.pos]) {
                fingerprint = ascii_ci_fingerprint_step(fingerprint, bytes[self.pos]);
                self.pos += 1;
            }
            if self.pos == name_start {
                // A stray `/` (or `>` inside a malformed region): skip it.
                self.pos += 1;
                continue;
            }
            let name = self.slice(name_start, self.pos);

            self.skip_whitespace(bytes);
            if self.pos >= end || bytes[self.pos] != b'=' {
                return Some(RawAttr {
                    name,
                    fingerprint,
                    value: None,
                });
            }
            self.pos += 1;
            self.skip_whitespace(bytes);

            let value = if self.pos < end && (bytes[self.pos] == b'"' || bytes[self.pos] == b'\'') {
                let quote = bytes[self.pos];
                self.pos += 1;
                let value_start = self.pos;
                let value_end = memchr::memchr(quote, &bytes[value_start..])
                    .map_or(end, |offset| value_start + offset);
                // Skip the closing quote when present.
                self.pos = (value_end + 1).min(end);
                self.slice(value_start, value_end)
            } else {
                let value_start = self.pos;
                while self.pos < end
                    && !is_html_whitespace(bytes[self.pos])
                    && bytes[self.pos] != b'>'
                {
                    self.pos += 1;
                }
                self.slice(value_start, self.pos)
            };
            return Some(RawAttr {
                name,
                fingerprint,
                value: Some(value),
            });
        }
    }
}

/// Splits a tag into raw attributes while the tag is scanned in pieces.
///
/// The resumable form of [`iter_raw_attrs`], for a tokenizer that already
/// finds quotes and `>` with an index: it scans up to each quote whose
/// meaning it needs, and jumps from an opening quote straight to the closing
/// one. Attributes come out exactly as [`iter_raw_attrs`] yields them for the
/// tag's attribute region, duplicates included, so the region is never
/// scanned a second time. [`AttrSplitter::state`] follows [`TagScanState`]
/// from [`TagScanState::START`].
///
/// # Example
///
/// ```
/// use fhp_core::attr::{AttrSplitter, iter_raw_attrs};
///
/// let tag = r#"<a href="/x" hidden>"#;
/// let close = tag.len() - 1;
/// let mut splitter = AttrSplitter::new();
/// let mut attrs = Vec::new();
/// splitter.scan(tag, 1, close, &mut attrs);
/// let name_end = splitter.finish(tag, close, &mut attrs);
/// assert_eq!(&tag[1..name_end], "a");
/// assert!(attrs.into_iter().eq(iter_raw_attrs(&tag[name_end..close])));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct AttrSplitter {
    state: TagScanState,
    /// End of the tag name, once the name state is left.
    tag_name_end: usize,
    /// The attribute being scanned: its name range, the name's
    /// [`ascii_ci_fingerprint`], and where its value starts.
    name_start: usize,
    name_end: usize,
    fingerprint: u64,
    value_start: usize,
}

impl Default for AttrSplitter {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl AttrSplitter {
    /// A splitter at the first tag-name byte, [`tag_name_offset`] bytes after
    /// the `<`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            state: TagScanState::START,
            tag_name_end: 0,
            name_start: 0,
            name_end: 0,
            fingerprint: ASCII_CI_FINGERPRINT_OFFSET,
            value_start: 0,
        }
    }

    /// Grammar state after the bytes scanned so far.
    #[inline]
    pub fn state(&self) -> TagScanState {
        self.state
    }

    /// Scan `input[pos..end]`, pushing every attribute that ends inside it.
    ///
    /// Calls must cover the tag in order and stop before the `>` that ends
    /// it. The one gap allowed is inside a quoted value: while
    /// [`AttrSplitter::state`] is [`TagScanState::Quoted`], the next call may
    /// start at the closing quote.
    ///
    /// # Panics
    ///
    /// Panics if `pos < end` and `end > input.len()`, or if an attribute
    /// boundary is not a character boundary of `input`. Boundaries sit next
    /// to ASCII delimiters, so that only happens when `pos` or `end` splits a
    /// character or the calls for one tag pass different inputs.
    #[inline(always)]
    pub fn scan<'a>(&mut self, input: &'a str, pos: usize, end: usize, out: &mut Vec<RawAttr<'a>>) {
        // The commonest calls end a quoted value at a closing quote the
        // caller found, or are empty because a `>` directly follows one. Both
        // stay inline; a call costs as much as scanning a short name.
        if pos >= end {
            return;
        }
        if let TagScanState::Quoted(quote) = self.state {
            if pos + 1 == end && input.as_bytes()[pos] == quote {
                self.push(input, Some(pos), out);
                self.state = TagScanState::BeforeAttrName;
                return;
            }
        }
        self.scan_bytes(input, pos, end, out);
    }

    fn scan_bytes<'a>(
        &mut self,
        input: &'a str,
        mut pos: usize,
        end: usize,
        out: &mut Vec<RawAttr<'a>>,
    ) {
        let bytes = &input.as_bytes()[..end];
        while pos < bytes.len() {
            match self.state {
                TagScanState::Name => {
                    match bytes[pos..]
                        .iter()
                        .position(|&byte| is_html_whitespace(byte) || byte == b'/')
                    {
                        Some(offset) => {
                            self.tag_name_end = pos + offset;
                            self.state = TagScanState::BeforeAttrName;
                            pos += offset + 1;
                        }
                        None => break,
                    }
                }
                TagScanState::BeforeAttrName => {
                    while pos < bytes.len()
                        && (is_html_whitespace(bytes[pos]) || bytes[pos] == b'/')
                    {
                        pos += 1;
                    }
                    let Some(&byte) = bytes.get(pos) else { break };
                    // Any other byte starts a name, a leading `=` included.
                    self.name_start = pos;
                    self.fingerprint = ascii_ci_fingerprint_step(ASCII_CI_FINGERPRINT_OFFSET, byte);
                    self.state = TagScanState::AttrName;
                    pos += 1;
                }
                TagScanState::AttrName => {
                    let mut fingerprint = self.fingerprint;
                    while pos < bytes.len() && !is_attr_name_end(bytes[pos]) {
                        fingerprint = ascii_ci_fingerprint_step(fingerprint, bytes[pos]);
                        pos += 1;
                    }
                    self.fingerprint = fingerprint;
                    let Some(&byte) = bytes.get(pos) else { break };
                    self.name_end = pos;
                    pos += 1;
                    self.state = match byte {
                        b'=' => TagScanState::BeforeAttrValue,
                        b'/' => {
                            self.push(input, None, out);
                            TagScanState::BeforeAttrName
                        }
                        _ => TagScanState::AfterAttrName,
                    };
                }
                TagScanState::AfterAttrName => {
                    while pos < bytes.len() && is_html_whitespace(bytes[pos]) {
                        pos += 1;
                    }
                    let Some(&byte) = bytes.get(pos) else { break };
                    if byte == b'=' {
                        self.state = TagScanState::BeforeAttrValue;
                        pos += 1;
                    } else {
                        // The before-name state skips a `/` and starts the
                        // next name at any other byte.
                        self.push(input, None, out);
                        self.state = TagScanState::BeforeAttrName;
                    }
                }
                TagScanState::BeforeAttrValue => {
                    while pos < bytes.len() && is_html_whitespace(bytes[pos]) {
                        pos += 1;
                    }
                    let Some(&byte) = bytes.get(pos) else { break };
                    if byte == b'"' || byte == b'\'' {
                        self.state = TagScanState::Quoted(byte);
                        pos += 1;
                    } else {
                        self.state = TagScanState::Unquoted;
                    }
                    self.value_start = pos;
                }
                TagScanState::Unquoted => {
                    while pos < bytes.len() && !is_html_whitespace(bytes[pos]) {
                        pos += 1;
                    }
                    if pos == bytes.len() {
                        break;
                    }
                    self.push(input, Some(pos), out);
                    self.state = TagScanState::BeforeAttrName;
                    pos += 1;
                }
                TagScanState::Quoted(quote) => match memchr::memchr(quote, &bytes[pos..]) {
                    Some(offset) => {
                        self.push(input, Some(pos + offset), out);
                        self.state = TagScanState::BeforeAttrName;
                        pos += offset + 1;
                    }
                    None => break,
                },
            }
        }
    }

    /// End the tag at `end`, the position of its closing `>`, after scanning
    /// up to it. Pushes the attribute still open there and returns the end
    /// of the tag name.
    ///
    /// The attribute region is `input[tag_name_end..end]`, less the `/` of a
    /// self-closing tag. The splitter is spent afterwards.
    ///
    /// # Panics
    ///
    /// As [`AttrSplitter::scan`], if `end` is not a character boundary.
    #[inline]
    pub fn finish<'a>(&self, input: &'a str, end: usize, out: &mut Vec<RawAttr<'a>>) -> usize {
        let mut open = *self;
        match self.state {
            TagScanState::Name => return end,
            TagScanState::BeforeAttrName => {}
            TagScanState::AttrName => {
                open.name_end = end;
                open.push(input, None, out);
            }
            TagScanState::AfterAttrName => open.push(input, None, out),
            // `a=` has an empty value.
            TagScanState::BeforeAttrValue => {
                open.value_start = end;
                open.push(input, Some(end), out);
            }
            // A quoted value is only open here when its quote is unterminated.
            TagScanState::Unquoted | TagScanState::Quoted(_) => open.push(input, Some(end), out),
        }
        self.tag_name_end
    }

    #[inline]
    fn push<'a>(&self, input: &'a str, value_end: Option<usize>, out: &mut Vec<RawAttr<'a>>) {
        out.push(RawAttr {
            name: split_slice(input, self.name_start, self.name_end),
            fingerprint: self.fingerprint,
            value: value_end.map(|end| split_slice(input, self.value_start, end)),
        });
    }
}

#[inline]
fn split_slice(input: &str, start: usize, end: usize) -> &str {
    match input.get(start..end) {
        Some(slice) => slice,
        None => split_range_panic(start, end),
    }
}

#[cold]
#[inline(never)]
fn split_range_panic(start: usize, end: usize) -> ! {
    panic!("attribute range {start}..{end} is not on character boundaries of the input")
}

#[inline(always)]
fn is_attr_name_end(byte: u8) -> bool {
    is_html_whitespace(byte) || matches!(byte, b'=' | b'/' | b'>')
}

const ASCII_CI_FINGERPRINT_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a over the ASCII-lowercased bytes of `name`.
///
/// Equal for names that differ only in ASCII case; used as a cheap
/// pre-filter before an exact case-insensitive comparison.
#[inline]
pub fn ascii_ci_fingerprint(name: &[u8]) -> u64 {
    name.iter()
        .fold(ASCII_CI_FINGERPRINT_OFFSET, |hash, &byte| {
            ascii_ci_fingerprint_step(hash, byte)
        })
}

#[inline(always)]
fn ascii_ci_fingerprint_step(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte.to_ascii_lowercase())).wrapping_mul(0x0000_0100_0000_01b3)
}

#[inline]
fn fingerprint_bloom(fingerprint: u64) -> u64 {
    (1u64 << (fingerprint & 63)) | (1u64 << ((fingerprint >> 32) & 63))
}

const INLINE_SEEN_ATTRIBUTES: usize = 8;

#[derive(Clone, Copy)]
struct SeenAttribute<'a> {
    fingerprint: u64,
    name: &'a [u8],
}

impl PartialEq for SeenAttribute<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint && self.name.eq_ignore_ascii_case(other.name)
    }
}

impl Eq for SeenAttribute<'_> {}

impl Hash for SeenAttribute<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the lowercased name with the randomly keyed hasher rather than
        // the public FNV fingerprint, so crafted fingerprint collisions cannot
        // degrade lookups to linear scans.
        for &byte in self.name {
            state.write_u8(byte.to_ascii_lowercase());
        }
        state.write_usize(self.name.len());
    }
}

/// First-wins duplicate detector for attribute names (ASCII
/// case-insensitive).
///
/// The first eight names live inline behind a 64-bit bloom filter; larger
/// tags spill into a randomly keyed hash set so lookups stay O(1).
pub struct SeenAttributes<'a> {
    bloom: u64,
    // `MaybeUninit` avoids zeroing 192 bytes for the overwhelmingly common
    // one-to-three-attribute case. The initialized prefix is tracked by
    // `inline_len` and `SeenAttribute` is `Copy`, so no drop work is needed.
    inline: [MaybeUninit<SeenAttribute<'a>>; INLINE_SEEN_ATTRIBUTES],
    inline_len: usize,
    heap: Option<HashSet<SeenAttribute<'a>, RandomState>>,
}

impl Default for SeenAttributes<'_> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> SeenAttributes<'a> {
    /// Create an empty tracker.
    #[inline]
    pub fn new() -> Self {
        Self {
            bloom: 0,
            inline: [MaybeUninit::uninit(); INLINE_SEEN_ATTRIBUTES],
            inline_len: 0,
            heap: None,
        }
    }

    /// Returns `true` if an equal (ASCII case-insensitive) name was already
    /// accepted; otherwise records `name` and returns `false`.
    ///
    /// `fingerprint` must be [`ascii_ci_fingerprint`]`(name)`.
    #[inline]
    pub fn duplicate_or_insert(&mut self, name: &'a [u8], fingerprint: u64) -> bool {
        debug_assert_eq!(fingerprint, ascii_ci_fingerprint(name));
        let candidate = SeenAttribute { fingerprint, name };

        if let Some(heap) = &mut self.heap {
            return !heap.insert(candidate);
        }

        let bloom = fingerprint_bloom(fingerprint);
        if self.bloom & bloom == bloom && self.inline_entries().contains(&candidate) {
            return true;
        }

        self.bloom |= bloom;
        if self.inline_len < INLINE_SEEN_ATTRIBUTES {
            self.inline[self.inline_len].write(candidate);
            self.inline_len += 1;
        } else {
            let mut heap =
                HashSet::with_capacity_and_hasher(INLINE_SEEN_ATTRIBUTES * 2, RandomState::new());
            heap.extend(self.inline_entries().iter().copied());
            heap.insert(candidate);
            self.heap = Some(heap);
        }
        false
    }

    #[inline]
    fn inline_entries(&self) -> &[SeenAttribute<'a>] {
        // SAFETY: only the prefix `[..inline_len]` is exposed, and every entry
        // in that prefix is initialized immediately before `inline_len` grows.
        unsafe {
            std::slice::from_raw_parts(
                self.inline.as_ptr().cast::<SeenAttribute<'a>>(),
                self.inline_len,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(region: &str) -> Vec<(&str, Option<&str>)> {
        iter_raw_attrs(region)
            .map(|attr| {
                assert_eq!(attr.fingerprint, ascii_ci_fingerprint(attr.name.as_bytes()));
                (attr.name, attr.value)
            })
            .collect()
    }

    fn end_state(body: &str) -> TagScanState {
        TagScanState::START.advance(body.as_bytes())
    }

    #[test]
    fn quotes_only_open_values_after_equals() {
        // A quote inside a name or an unquoted value is literal.
        assert_eq!(end_state("img alt=it's"), TagScanState::Unquoted);
        assert_eq!(end_state("div a\"b"), TagScanState::AttrName);
        assert_eq!(end_state("a b=x'y c='z"), TagScanState::Quoted(b'\''));
        assert!(end_state("a b=x'y c='z").gt_is_literal());
        assert_eq!(end_state("a b=x'y c='z>w'"), TagScanState::BeforeAttrName);
    }

    #[test]
    fn slash_is_literal_in_unquoted_values() {
        assert_eq!(
            end_state("a href=http://example.com/"),
            TagScanState::Unquoted
        );
        assert_eq!(end_state("a b=/"), TagScanState::Unquoted);
        assert_eq!(end_state("a b=\"x\"/"), TagScanState::BeforeAttrName);
        assert_eq!(end_state("br/"), TagScanState::BeforeAttrName);
    }

    /// State after the bytes of `markup` (starting at `<`), scanned from the
    /// first tag-name byte.
    fn markup_end_state(markup: &str) -> TagScanState {
        let bytes = markup.as_bytes();
        TagScanState::START.advance(&bytes[tag_name_offset(bytes)..])
    }

    #[test]
    fn tag_names_treat_equals_and_quotes_as_name_bytes() {
        assert_eq!(tag_name_offset(b"<p"), 1);
        assert_eq!(tag_name_offset(b"</p"), 2);
        assert_eq!(markup_end_state("</div=\"x"), TagScanState::Name);
        assert_eq!(markup_end_state("<a='x"), TagScanState::Name);
        // `=` before an attribute name starts the name (WHATWG).
        assert_eq!(markup_end_state("</div ='x"), TagScanState::AttrName);
        // End tags still scan attribute values.
        assert_eq!(
            markup_end_state("</p class=\"a>"),
            TagScanState::Quoted(b'"')
        );
    }

    #[test]
    fn whitespace_around_equals() {
        assert_eq!(end_state("a b = \"x"), TagScanState::Quoted(b'"'));
        assert_eq!(end_state("a b c"), TagScanState::AttrName);
        assert_eq!(end_state("a b "), TagScanState::AfterAttrName);
    }

    #[test]
    fn raw_attrs_follow_tag_grammar() {
        assert_eq!(attrs(" alt=it's"), [("alt", Some("it's"))]);
        assert_eq!(attrs(" a\"b"), [("a\"b", None)]);
        assert_eq!(
            attrs(" b=x'y c='z>w'"),
            [("b", Some("x'y")), ("c", Some("z>w"))]
        );
        assert_eq!(
            attrs(" href=http://example.com/"),
            [("href", Some("http://example.com/"))]
        );
        assert_eq!(
            attrs(" b = \"x y\"c d"),
            [("b", Some("x y")), ("c", None), ("d", None)]
        );
        assert_eq!(attrs(" b/c"), [("b", None), ("c", None)]);
        assert_eq!(attrs(" =x"), [("=x", None)]);
        assert_eq!(attrs(" a=\"unterminated"), [("a", Some("unterminated"))]);
        assert_eq!(attrs(" a="), [("a", Some(""))]);
        assert_eq!(attrs(" é=ü"), [("é", Some("ü"))]);
        assert!(attrs("  \t\n").is_empty());
    }

    /// Split the start tag at the start of `input` the way the tokenizer
    /// does: scan up to each quote, jump over quoted values, and finish at
    /// the first `>` outside one. Checks the state and the result against
    /// [`TagScanState`] and [`iter_raw_attrs`]; returns `false` when the tag
    /// does not end.
    fn check_split_like_tokenizer(input: &str) -> bool {
        let bytes = input.as_bytes();
        let start = tag_name_offset(bytes);
        let mut splitter = AttrSplitter::new();
        let mut attrs = Vec::new();
        let mut scan_pos = start;
        for pos in start..bytes.len() {
            match bytes[pos] {
                quote @ (b'"' | b'\'') => {
                    match splitter.state() {
                        TagScanState::Quoted(open) if open != quote => {}
                        TagScanState::Quoted(_) => splitter.scan(input, pos, pos + 1, &mut attrs),
                        _ => splitter.scan(input, scan_pos, pos + 1, &mut attrs),
                    }
                    scan_pos = pos + 1;
                    let state = TagScanState::START.advance(&bytes[start..=pos]);
                    assert_eq!(splitter.state(), state, "{input:?} at {pos}");
                }
                b'>' if !splitter.state().gt_is_literal() => {
                    splitter.scan(input, scan_pos, pos, &mut attrs);
                    let state = TagScanState::START.advance(&bytes[start..pos]);
                    assert_eq!(splitter.state(), state, "{input:?} at {pos}");
                    let name_end = splitter.finish(input, pos, &mut attrs);
                    let expected_name_end = bytes[start..pos]
                        .iter()
                        .position(|&byte| is_html_whitespace(byte) || byte == b'/')
                        .map_or(pos, |offset| start + offset);
                    assert_eq!(name_end, expected_name_end, "{input:?}");

                    let self_closing = bytes[pos - 1] == b'/' && state != TagScanState::Unquoted;
                    let region_end = if self_closing { pos - 1 } else { pos };
                    let region = &input[name_end..region_end.max(name_end)];
                    let expected: Vec<_> = iter_raw_attrs(region).collect();
                    assert_eq!(attrs, expected, "{input:?}");

                    // One call over the whole tag gives the same attributes.
                    let mut whole = AttrSplitter::new();
                    let mut whole_attrs = Vec::new();
                    whole.scan(input, start, pos, &mut whole_attrs);
                    assert_eq!(whole.finish(input, pos, &mut whole_attrs), name_end);
                    assert_eq!(whole_attrs, expected, "{input:?} in one call");
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    #[test]
    fn splitter_matches_raw_attrs() {
        for region in [
            "",
            " alt=it's",
            " a\"b",
            " b=x'y c='z>w'",
            " href=http://example.com/",
            " b = \"x y\"c d",
            " b/c",
            " =x",
            " ==x",
            " a=",
            " a= ",
            " a=\"\"",
            " é=ü",
            " Data-X=1 data-x='2' DATA-X",
            "/b=/",
            " b=\"x\"/",
            " b /",
            " b= /",
            "  \t\n",
        ] {
            assert!(check_split_like_tokenizer(&format!("<t{region}>")));
            assert!(check_split_like_tokenizer(&format!("</t{region}>")));
        }
        assert!(check_split_like_tokenizer("<a\"b c='d'>"));
        assert!(check_split_like_tokenizer("<br/>"));
        assert!(!check_split_like_tokenizer("<t a=\"x>"));
    }

    #[test]
    fn splitter_matches_raw_attrs_exhaustively() {
        // Every tag body up to five bytes over the bytes that change the
        // grammar state, plus a letter in both cases and a multibyte one.
        const ALPHABET: [&str; 9] = ["a", "B", " ", "=", "\"", "'", "/", ">", "é"];
        let mut body = Vec::new();
        let mut ended = 0;
        loop {
            let tag: String = std::iter::once("<t")
                .chain(body.iter().map(|&index| ALPHABET[index]))
                .chain(std::iter::once(">"))
                .collect();
            ended += usize::from(check_split_like_tokenizer(&tag));

            // Next body in counting order.
            let mut digit = 0;
            loop {
                if digit == body.len() {
                    if body.len() == 5 {
                        assert!(ended > 10_000, "only {ended} tags ended");
                        return;
                    }
                    body.push(0);
                    break;
                }
                body[digit] += 1;
                if body[digit] < ALPHABET.len() {
                    break;
                }
                body[digit] = 0;
                digit += 1;
            }
        }
    }

    #[test]
    #[should_panic(expected = "character boundaries")]
    fn splitter_rejects_a_split_character() {
        let input = "<t é=x>";
        let mut splitter = AttrSplitter::new();
        let mut attrs = Vec::new();
        splitter.scan(input, 1, 3, &mut attrs);
        // Resume inside `é`: the name would start mid-character.
        splitter.scan(input, 4, 7, &mut attrs);
        splitter.finish(input, 7, &mut attrs);
    }

    #[test]
    fn seen_attributes_are_case_insensitive_first_wins() {
        let names: Vec<String> = (0..40).map(|i| format!("Attr{i}")).collect();
        let uppers: Vec<String> = names.iter().map(|name| name.to_ascii_uppercase()).collect();
        let mut seen = SeenAttributes::new();
        for name in &names {
            assert!(
                !seen.duplicate_or_insert(name.as_bytes(), ascii_ci_fingerprint(name.as_bytes()))
            );
        }
        for upper in &uppers {
            let upper = upper.as_bytes();
            assert!(seen.duplicate_or_insert(upper, ascii_ci_fingerprint(upper)));
        }
        let fresh = b"other";
        assert!(!seen.duplicate_or_insert(fresh, ascii_ci_fingerprint(fresh)));
    }

    #[test]
    fn seen_attributes_inline_path() {
        let mut seen = SeenAttributes::new();
        let mut check =
            |name: &'static [u8]| seen.duplicate_or_insert(name, ascii_ci_fingerprint(name));
        assert!(!check(b"id"));
        assert!(check(b"ID"));
        assert!(!check(b"class"));
        assert!(check(b"Class"));
        assert!(!check(b"x"));
        assert!(check(b"id"));
    }
}
