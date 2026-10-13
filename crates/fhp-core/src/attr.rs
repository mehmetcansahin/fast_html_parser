//! The single HTML start-tag attribute grammar shared by the tokenizer, the
//! tree builder, and the encoding prescan.
//!
//! [`TagScanState`] tracks where a byte sits inside a tag body so callers can
//! decide whether a `>` ends the tag (it does not inside a quoted value) and
//! whether a trailing `/` is a self-closing flag (it is not inside an unquoted
//! value). [`iter_raw_attrs`] splits a tag's attribute region into raw
//! name/value pairs, and [`SeenAttributes`] implements first-wins duplicate
//! detection with ASCII case-insensitive names.

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
