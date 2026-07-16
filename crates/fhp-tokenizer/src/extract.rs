//! Token extraction — stage 2 of the two-stage tokenizer pipeline.
//!
//! Uses the structural index from stage 1 to locate `<` and `>` boundaries,
//! then scans the actual input bytes between them to extract tag names,
//! attributes, comments, and text content. This hybrid approach combines
//! SIMD-accelerated delimiter finding with scalar content parsing.

use std::borrow::Cow;
use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::mem::MaybeUninit;

use fhp_core::tag::Tag;

use crate::TreeSink;
use crate::structural::StructuralIndex;
use crate::token::{Attribute, Token};

const INLINE_SEEN_ATTRIBUTES: usize = 8;
const ASCII_CI_FINGERPRINT_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

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
        state.write_u64(self.fingerprint);
    }
}

/// A no-op hasher for the already-hashed attribute fingerprints.
#[derive(Default)]
struct FingerprintHasher(u64);

impl Hasher for FingerprintHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

type SeenAttributeSet<'a> = HashSet<SeenAttribute<'a>, BuildHasherDefault<FingerprintHasher>>;

struct SeenAttributes<'a> {
    bloom: u64,
    // `MaybeUninit` avoids zeroing 192 bytes for the overwhelmingly common
    // one-to-three-attribute case. The initialized prefix is tracked by
    // `inline_len` and `SeenAttribute` is `Copy`, so no drop work is needed.
    inline: [MaybeUninit<SeenAttribute<'a>>; INLINE_SEEN_ATTRIBUTES],
    inline_len: usize,
    heap: Option<SeenAttributeSet<'a>>,
}

impl<'a> SeenAttributes<'a> {
    #[inline]
    fn new() -> Self {
        Self {
            bloom: 0,
            inline: [MaybeUninit::uninit(); INLINE_SEEN_ATTRIBUTES],
            inline_len: 0,
            heap: None,
        }
    }

    #[inline]
    fn duplicate_or_insert(&mut self, name: &'a [u8]) -> bool {
        if self.inline_len == 0 {
            self.inline[0].write(SeenAttribute {
                fingerprint: 0,
                name,
            });
            self.inline_len = 1;
            return false;
        }
        if self.bloom == 0 {
            let first = self.inline_entries()[0];
            if first.name.eq_ignore_ascii_case(name) {
                return true;
            }
            let first_fingerprint = ascii_ci_fingerprint(first.name);
            self.inline[0].write(SeenAttribute {
                fingerprint: first_fingerprint,
                name: first.name,
            });
            self.bloom = fingerprint_bloom(first_fingerprint);
        }

        let fingerprint = ascii_ci_fingerprint(name);
        self.duplicate_or_insert_hashed(name, fingerprint)
    }

    /// Variant for callers that already visited every name byte while parsing.
    #[inline]
    fn duplicate_or_insert_hashed(&mut self, name: &'a [u8], fingerprint: u64) -> bool {
        let candidate = SeenAttribute { fingerprint, name };

        if let Some(heap) = &mut self.heap {
            return !heap.insert(candidate);
        }

        let bloom = fingerprint_bloom(fingerprint);
        if self.bloom & bloom == bloom
            && self
                .inline_entries()
                .iter()
                .any(|seen| seen.fingerprint == fingerprint && seen.name.eq_ignore_ascii_case(name))
        {
            return true;
        }

        self.bloom |= bloom;
        if self.inline_len < INLINE_SEEN_ATTRIBUTES {
            self.inline[self.inline_len].write(candidate);
            self.inline_len += 1;
        } else {
            let mut heap = SeenAttributeSet::with_capacity_and_hasher(
                INLINE_SEEN_ATTRIBUTES * 2,
                BuildHasherDefault::default(),
            );
            for &seen in self.inline_entries() {
                heap.insert(seen);
            }
            heap.insert(candidate);
            self.heap = Some(heap);
        }
        false
    }

    #[inline]
    fn hashes_during_parse(&self) -> bool {
        self.bloom != 0
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

#[inline]
fn ascii_ci_fingerprint(name: &[u8]) -> u64 {
    let mut hash = ASCII_CI_FINGERPRINT_OFFSET;
    for &byte in name {
        hash = ascii_ci_fingerprint_step(hash, byte);
    }
    hash
}

#[inline(always)]
fn ascii_ci_fingerprint_step(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte.to_ascii_lowercase())).wrapping_mul(0x0000_0100_0000_01b3)
}

#[inline]
fn fingerprint_bloom(fingerprint: u64) -> u64 {
    (1u64 << (fingerprint & 63)) | (1u64 << ((fingerprint >> 32) & 63))
}

#[cfg(feature = "entity-decode")]
#[inline]
fn maybe_decode_entities<'a>(input: &'a str) -> Cow<'a, str> {
    crate::entity::decode_entities(input)
}

#[cfg(feature = "entity-decode")]
#[inline]
fn maybe_decode_attribute_entities<'a>(input: &'a str) -> Cow<'a, str> {
    crate::entity::decode_attribute_entities(input)
}

#[cfg(not(feature = "entity-decode"))]
#[inline]
fn maybe_decode_entities<'a>(input: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(input)
}

#[cfg(not(feature = "entity-decode"))]
#[inline]
fn maybe_decode_attribute_entities<'a>(input: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(input)
}

/// Extract tokens from pre-indexed UTF-8 input.
///
/// `input` must have the same length and structural delimiters as the bytes
/// passed to [`StructuralIndexer::index`](crate::structural::StructuralIndexer::index).
/// Each recorded delimiter is validated before that indexed position is used.
///
/// # Panics
///
/// Panics if the index length or a recorded delimiter does not match `input`.
///
/// # Example
///
/// ```
/// use fhp_tokenizer::structural::StructuralIndexer;
/// use fhp_tokenizer::extract::extract_tokens;
///
/// let html = "<div>hello</div>";
/// let indexer = StructuralIndexer::new();
/// let index = indexer.index(html.as_bytes());
/// let tokens = extract_tokens(html, &index);
/// assert!(tokens.len() >= 3); // OpenTag, Text, CloseTag
/// ```
pub fn extract_tokens<'a>(input: &'a str, index: &StructuralIndex) -> Vec<Token<'a>> {
    assert_eq!(
        input.len(),
        index.input_len(),
        "structural index does not match input: input length differs"
    );

    let mut tokens = Vec::with_capacity(index.estimated_token_count());
    let mut parser = Parser::new(input);

    for delim in index.iter_delimiters() {
        assert_eq!(
            input.as_bytes().get(delim.pos).copied(),
            Some(delim.byte),
            "structural index does not match input: delimiter differs at byte {}",
            delim.pos
        );
        parser.on_delimiter(delim.pos, delim.byte, &mut tokens);
    }

    // Flush trailing text.
    parser.flush_trailing(&mut tokens);

    tokens
}

/// Extract tokens from pre-indexed raw bytes after UTF-8 validation.
///
/// # Panics
///
/// Panics if the index length or a recorded delimiter does not match `input`.
pub fn extract_tokens_bytes<'a>(
    input: &'a [u8],
    index: &StructuralIndex,
) -> Result<Vec<Token<'a>>, std::str::Utf8Error> {
    let input = std::str::from_utf8(input)?;
    Ok(extract_tokens(input, index))
}

/// Parsing mode — tracks what the parser is currently inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Normal text content between tags.
    Data,
    /// Inside a tag (between `<` and `>`).
    InTag,
    /// Inside a comment (`<!-- ... -->`).
    InComment,
    /// Inside a doctype (`<!DOCTYPE ...>`).
    InDoctype,
    /// Inside a CDATA section (`<![CDATA[ ... ]]>`).
    InCData,
    /// Inside a raw text element (script/style/iframe).
    InRawText,
    /// Inside an RCDATA element (title/textarea).
    InRcData,
    /// After a `<plaintext>` start tag. Everything through EOF is text.
    InPlainText,
}

/// Direct input parser driven by structural delimiter positions.
///
/// Can be used in two modes:
/// - Vec mode: via `on_delimiter` / `flush_trailing` (pushes to `Vec<Token>`)
/// - Callback mode: via `on_delimiter_cb` / `flush_trailing_cb` (invokes closure)
pub(crate) struct Parser<'a> {
    input_str: &'a str,
    input: &'a [u8],
    mode: Mode,
    /// Position after the last emitted token (start of next text region).
    cursor: usize,
    /// Position of the `<` that opened the current tag.
    tag_open_pos: usize,
    /// Position of the `<!` or `<!--` that opened special content.
    special_open_pos: usize,
    /// Tag we're inside for raw text mode.
    raw_text_tag: Tag,
    /// While inside a tag, the quote char (`"` or `'`) of the attribute value
    /// we are currently within, or `None` when not inside a quoted value. Used
    /// so a `>` inside an attribute value does not close the tag.
    attr_quote: Option<u8>,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            input_str: input,
            input: input.as_bytes(),
            mode: Mode::Data,
            cursor: 0,
            tag_open_pos: 0,
            special_open_pos: 0,
            raw_text_tag: Tag::Unknown,
            attr_quote: None,
        }
    }

    /// Update the in-attribute quote state for a quote byte (`"` or `'`) seen
    /// inside a tag. Opening a quote records its char; the matching char closes
    /// it; the other quote char inside a quoted value is a literal (ignored).
    #[inline(always)]
    fn update_attr_quote(&mut self, byte: u8) {
        match self.attr_quote {
            None => self.attr_quote = Some(byte),
            Some(open) if open == byte => self.attr_quote = None,
            Some(_) => {}
        }
    }

    /// Process a structural delimiter (Vec mode).
    fn on_delimiter(&mut self, pos: usize, byte: u8, tokens: &mut Vec<Token<'a>>) {
        self.on_delimiter_impl(pos, byte, &mut |token| tokens.push(token));
    }

    /// Process a structural delimiter (callback mode).
    pub(crate) fn on_delimiter_cb(
        &mut self,
        pos: usize,
        byte: u8,
        emit: &mut impl FnMut(Token<'a>),
    ) {
        self.on_delimiter_impl(pos, byte, emit);
    }

    /// Shared delimiter dispatch logic.
    #[inline(always)]
    fn on_delimiter_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        match self.mode {
            Mode::Data => self.on_data_impl(pos, byte, emit),
            Mode::InTag => self.on_in_tag_impl(pos, byte, emit),
            Mode::InComment => self.on_in_comment_impl(pos, byte, emit),
            Mode::InDoctype => self.on_in_doctype_impl(pos, byte, emit),
            Mode::InCData => self.on_in_cdata_impl(pos, byte, emit),
            Mode::InRawText | Mode::InRcData => self.on_in_raw_text_impl(pos, byte, emit),
            Mode::InPlainText => {}
        }
    }

    /// In Data mode: only `<` matters.
    #[inline(always)]
    fn on_data_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'<' {
            let after = self.peek(pos + 1);
            let after2 = self.peek(pos + 2);
            if !starts_markup_after_lt(after, after2) {
                return;
            }

            // Flush text before this `<`.
            self.flush_text_impl(pos, emit);
            self.tag_open_pos = pos;
            self.attr_quote = None;

            // Peek ahead to classify what follows `<`.
            if after == Some(b'!') {
                // Could be comment, doctype, or CDATA.
                if after2 == Some(b'-') && self.peek(pos + 3) == Some(b'-') {
                    self.mode = Mode::InComment;
                    self.special_open_pos = pos;
                } else if after2.is_some_and(|b| b == b'D' || b == b'd') {
                    self.mode = Mode::InDoctype;
                    self.special_open_pos = pos;
                } else if after2 == Some(b'[') && self.is_cdata_open(pos) {
                    self.mode = Mode::InCData;
                    self.special_open_pos = pos;
                } else {
                    // Unknown <! — treat as doctype-like.
                    self.mode = Mode::InDoctype;
                    self.special_open_pos = pos;
                }
            } else {
                // Normal tag (open or close).
                self.mode = Mode::InTag;
            }
        }
        // Other delimiters in Data mode are part of text content (entities, etc.)
    }

    /// In tag mode: `>` closes the tag, unless it sits inside an attribute
    /// value. `"`/`'` toggle the quoted-value state.
    #[inline(always)]
    fn on_in_tag_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        match byte {
            b'"' | b'\'' => self.update_attr_quote(byte),
            b'>' if self.attr_quote.is_none() => {
                // Parse the tag content between `<` and `>`.
                self.parse_tag_impl(self.tag_open_pos, pos, emit);
                self.cursor = pos + 1;
                // parse_tag may have entered a text-element mode — don't override.
                if !matches!(
                    self.mode,
                    Mode::InRawText | Mode::InRcData | Mode::InPlainText
                ) {
                    self.mode = Mode::Data;
                }
            }
            // Other delimiters inside tags (`=`, `/`, and `>` inside a quoted
            // value) are handled during tag parsing when we see the closing `>`.
            _ => {}
        }
    }

    /// In comment mode: look for `-->`.
    fn on_in_comment_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'>' && pos >= 2 {
            // Check for `-->`.
            if self.input[pos - 1] == b'-' && self.input[pos - 2] == b'-' {
                // Comment content is between `<!--` and `-->`.
                let content_start = self.special_open_pos + 4; // after `<!--`
                let content_end = pos - 2; // before `--`
                let content = if content_start <= content_end {
                    self.str_slice(content_start, content_end)
                } else {
                    ""
                };
                emit(Token::Comment {
                    content: Cow::Borrowed(content),
                });
                self.cursor = pos + 1;
                self.mode = Mode::Data;
            }
        }
    }

    /// In doctype mode: an unquoted `>` closes it.
    fn on_in_doctype_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if matches!(byte, b'"' | b'\'') {
            self.update_attr_quote(byte);
            return;
        }
        if byte == b'>' && self.attr_quote.is_none() {
            // Content between `<!` and `>`.
            let inner_start = self.special_open_pos + 2; // after `<!`
            let content = self.str_slice(inner_start, pos).trim();
            // Strip "DOCTYPE " prefix if present.
            let content =
                if content.len() >= 7 && content.as_bytes()[..7].eq_ignore_ascii_case(b"DOCTYPE") {
                    content[7..].trim_start()
                } else {
                    content
                };
            emit(Token::Doctype {
                content: Cow::Borrowed(content),
            });
            self.cursor = pos + 1;
            self.mode = Mode::Data;
        }
    }

    /// In CDATA mode: look for `]]>`.
    fn on_in_cdata_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'>' && pos >= 2 && self.input[pos - 1] == b']' && self.input[pos - 2] == b']' {
            // Content between `<![CDATA[` and `]]>`.
            let content_start = self.special_open_pos + 9; // after `<![CDATA[`
            let content_end = pos - 2; // before `]]`
            let content = if content_start <= content_end {
                self.str_slice(content_start, content_end)
            } else {
                ""
            };
            emit(Token::CData {
                content: Cow::Borrowed(content),
            });
            self.cursor = pos + 1;
            self.mode = Mode::Data;
        }
    }

    /// In raw text/RCDATA mode, only the matching end tag is markup.
    fn on_in_raw_text_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'<' && self.is_raw_text_close(pos) {
            if self.mode == Mode::InRcData {
                self.flush_text_impl(pos, emit);
            } else {
                self.flush_text_raw_impl(pos, emit);
            }
            self.tag_open_pos = pos;
            self.attr_quote = None;
            self.mode = Mode::InTag;
        }
    }

    /// Parse a complete tag from `<` at `open` to `>` at `close`.
    fn parse_tag_impl(&mut self, open: usize, close: usize, emit: &mut impl FnMut(Token<'a>)) {
        // Skip the `<`.
        let mut pos = open + 1;
        if pos >= close {
            return;
        }

        let first = self.input[pos];

        // Close tag: `</...>`
        if first == b'/' {
            pos += 1;
            let name_start = pos;
            while pos < close && !is_whitespace(self.input[pos]) && self.input[pos] != b'/' {
                pos += 1;
            }
            let name = self.str_slice(name_start, pos);
            let tag = Tag::from_bytes(&self.input[name_start..pos]);
            emit(Token::CloseTag {
                tag,
                name: Cow::Borrowed(name),
            });
            return;
        }

        // Open tag: `<name ...>` or `<name ... />`
        let name_start = pos;
        while pos < close
            && !is_whitespace(self.input[pos])
            && self.input[pos] != b'/'
            && self.input[pos] != b'>'
        {
            pos += 1;
        }
        let name = self.str_slice(name_start, pos);
        let tag = Tag::from_bytes(&self.input[name_start..pos]);

        // The flag records source syntax only. HTML tree construction ignores
        // a trailing slash on non-void HTML elements.
        let self_closing = close > 0 && self.input[close - 1] == b'/';

        // Parse attributes.
        let attrs = self.parse_attributes(pos, if self_closing { close - 1 } else { close });

        emit(Token::OpenTag {
            tag,
            name: Cow::Borrowed(name),
            attributes: attrs,
            self_closing,
        });

        // Enter the appropriate text-element mode.
        if name.eq_ignore_ascii_case("plaintext") {
            self.mode = Mode::InPlainText;
        } else if !tag.is_void() && tag.is_raw_text() {
            self.mode = Mode::InRawText;
            self.raw_text_tag = tag;
        } else if !tag.is_void() && tag.is_rcdata() {
            self.mode = Mode::InRcData;
            self.raw_text_tag = tag;
        }
    }

    /// Parse attributes from the region between tag name and `>`.
    fn parse_attributes(&self, start: usize, end: usize) -> Vec<Attribute<'a>> {
        let estimated = if end > start {
            // Attribute-heavy scraping input is commonly denser than the old
            // 15-byte estimate (for example, ` data-x=1`). Reserving from a
            // 10-byte estimate avoids the 2 -> 4 growth for three attributes
            // and the late reallocation around sixteen without letting a
            // single tag reserve an unbounded amount.
            ((end - start) / 10).clamp(2, 16)
        } else {
            2
        };
        let mut attrs: Vec<Attribute<'a>> = Vec::with_capacity(estimated);
        let mut seen = SeenAttributes::new();
        let mut pos = start;

        loop {
            // Skip whitespace.
            while pos < end && is_whitespace(self.input[pos]) {
                pos += 1;
            }
            if pos >= end {
                break;
            }

            // Attribute name.
            let name_start = pos;
            let hash_during_parse = seen.hashes_during_parse();
            let mut fingerprint = ASCII_CI_FINGERPRINT_OFFSET;
            if hash_during_parse {
                while pos < end
                    && !is_whitespace(self.input[pos])
                    && self.input[pos] != b'='
                    && self.input[pos] != b'/'
                    && self.input[pos] != b'>'
                {
                    fingerprint = ascii_ci_fingerprint_step(fingerprint, self.input[pos]);
                    pos += 1;
                }
            } else {
                while pos < end
                    && !is_whitespace(self.input[pos])
                    && self.input[pos] != b'='
                    && self.input[pos] != b'/'
                    && self.input[pos] != b'>'
                {
                    pos += 1;
                }
            }
            let name_end = pos;
            if name_start == name_end {
                pos += 1;
                continue;
            }
            let attr_name = self.str_slice(name_start, name_end);
            let attr_name_bytes = attr_name.as_bytes();
            let duplicate = if hash_during_parse {
                seen.duplicate_or_insert_hashed(attr_name_bytes, fingerprint)
            } else {
                seen.duplicate_or_insert(attr_name_bytes)
            };

            // Skip whitespace.
            while pos < end && is_whitespace(self.input[pos]) {
                pos += 1;
            }

            // Check for `=`.
            if pos < end && self.input[pos] == b'=' {
                pos += 1; // skip '='

                // Skip whitespace.
                while pos < end && is_whitespace(self.input[pos]) {
                    pos += 1;
                }

                // Parse value.
                if pos < end && (self.input[pos] == b'"' || self.input[pos] == b'\'') {
                    // Quoted value.
                    let quote = self.input[pos];
                    pos += 1; // skip opening quote
                    let val_start = pos;
                    while pos < end && self.input[pos] != quote {
                        pos += 1;
                    }
                    let val_end = pos;
                    if pos < end {
                        pos += 1; // skip closing quote
                    }
                    let raw_value = self.str_slice(val_start, val_end);
                    if !duplicate {
                        let value = maybe_decode_attribute_entities(raw_value);
                        attrs.push(Attribute {
                            name: Cow::Borrowed(attr_name),
                            value: Some(value),
                        });
                    }
                } else {
                    // Unquoted value.
                    let val_start = pos;
                    while pos < end && !is_whitespace(self.input[pos]) && self.input[pos] != b'>' {
                        pos += 1;
                    }
                    let raw_value = self.str_slice(val_start, pos);
                    if !duplicate {
                        let value = maybe_decode_attribute_entities(raw_value);
                        attrs.push(Attribute {
                            name: Cow::Borrowed(attr_name),
                            value: Some(value),
                        });
                    }
                }
            } else {
                // Boolean attribute (no value).
                if !duplicate {
                    attrs.push(Attribute {
                        name: Cow::Borrowed(attr_name),
                        value: None,
                    });
                }
            }
        }

        attrs
    }

    /// Flush text from cursor to pos (generic).
    #[inline(always)]
    fn flush_text_impl(&mut self, pos: usize, emit: &mut impl FnMut(Token<'a>)) {
        if pos > self.cursor {
            let raw = self.str_slice(self.cursor, pos);
            if !raw.is_empty() {
                let content = maybe_decode_entities(raw);
                emit(Token::Text { content });
            }
        }
        self.cursor = pos;
    }

    /// Flush raw text from cursor to pos without entity decoding.
    #[inline(always)]
    fn flush_text_raw_impl(&mut self, pos: usize, emit: &mut impl FnMut(Token<'a>)) {
        if pos > self.cursor {
            let raw = self.str_slice(self.cursor, pos);
            if !raw.is_empty() {
                emit(Token::Text {
                    content: Cow::Borrowed(raw),
                });
            }
        }
        self.cursor = pos;
    }

    /// Flush trailing text at end of input (Vec mode).
    fn flush_trailing(&mut self, tokens: &mut Vec<Token<'a>>) {
        self.flush_trailing_impl(&mut |token| tokens.push(token));
    }

    /// Flush trailing text at end of input (callback mode).
    pub(crate) fn flush_trailing_cb(&mut self, emit: &mut impl FnMut(Token<'a>)) {
        self.flush_trailing_impl(emit);
    }

    // ---- TreeSink-based methods (zero-alloc path) ----

    /// Process a structural delimiter (sink mode).
    pub(crate) fn on_delimiter_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        match self.mode {
            Mode::Data => self.on_data_sink(pos, byte, sink),
            Mode::InTag => self.on_in_tag_sink(pos, byte, sink),
            Mode::InComment => self.on_in_comment_sink(pos, byte, sink),
            Mode::InDoctype => self.on_in_doctype_sink(pos, byte, sink),
            Mode::InCData => self.on_in_cdata_sink(pos, byte, sink),
            Mode::InRawText | Mode::InRcData => self.on_in_raw_text_sink(pos, byte, sink),
            Mode::InPlainText => {}
        }
    }

    /// In Data mode (sink): only `<` matters.
    #[inline(always)]
    fn on_data_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'<' {
            let after = self.peek(pos + 1);
            let after2 = self.peek(pos + 2);
            if !starts_markup_after_lt(after, after2) {
                return;
            }

            self.flush_text_sink(pos, sink);
            self.tag_open_pos = pos;
            self.attr_quote = None;

            if after == Some(b'!') {
                if after2 == Some(b'-') && self.peek(pos + 3) == Some(b'-') {
                    self.mode = Mode::InComment;
                    self.special_open_pos = pos;
                } else if after2.is_some_and(|b| b == b'D' || b == b'd') {
                    self.mode = Mode::InDoctype;
                    self.special_open_pos = pos;
                } else if after2 == Some(b'[') && self.is_cdata_open(pos) {
                    self.mode = Mode::InCData;
                    self.special_open_pos = pos;
                } else {
                    self.mode = Mode::InDoctype;
                    self.special_open_pos = pos;
                }
            } else {
                self.mode = Mode::InTag;
            }
        }
    }

    /// In tag mode (sink): `>` closes the tag, unless it sits inside an
    /// attribute value. `"`/`'` toggle the quoted-value state.
    #[inline(always)]
    fn on_in_tag_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        match byte {
            b'"' | b'\'' => self.update_attr_quote(byte),
            b'>' if self.attr_quote.is_none() => {
                self.parse_tag_sink(self.tag_open_pos, pos, sink);
                self.cursor = pos + 1;
                if !matches!(
                    self.mode,
                    Mode::InRawText | Mode::InRcData | Mode::InPlainText
                ) {
                    self.mode = Mode::Data;
                }
            }
            _ => {}
        }
    }

    /// In comment mode (sink): look for `-->`.
    fn on_in_comment_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'>' && pos >= 2 && self.input[pos - 1] == b'-' && self.input[pos - 2] == b'-' {
            let content_start = self.special_open_pos + 4;
            let content_end = pos - 2;
            let content = if content_start <= content_end {
                self.str_slice(content_start, content_end)
            } else {
                ""
            };
            sink.comment(content);
            self.cursor = pos + 1;
            self.mode = Mode::Data;
        }
    }

    /// In doctype mode (sink): an unquoted `>` closes it.
    fn on_in_doctype_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if matches!(byte, b'"' | b'\'') {
            self.update_attr_quote(byte);
            return;
        }
        if byte == b'>' && self.attr_quote.is_none() {
            let inner_start = self.special_open_pos + 2;
            let content = self.str_slice(inner_start, pos).trim();
            let content =
                if content.len() >= 7 && content.as_bytes()[..7].eq_ignore_ascii_case(b"DOCTYPE") {
                    content[7..].trim_start()
                } else {
                    content
                };
            sink.doctype(content);
            self.cursor = pos + 1;
            self.mode = Mode::Data;
        }
    }

    /// In CDATA mode (sink): look for `]]>`.
    fn on_in_cdata_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'>' && pos >= 2 && self.input[pos - 1] == b']' && self.input[pos - 2] == b']' {
            let content_start = self.special_open_pos + 9;
            let content_end = pos - 2;
            let content = if content_start <= content_end {
                self.str_slice(content_start, content_end)
            } else {
                ""
            };
            sink.cdata(content);
            self.cursor = pos + 1;
            self.mode = Mode::Data;
        }
    }

    /// In raw text/RCDATA mode (sink), only the matching end tag is markup.
    fn on_in_raw_text_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'<' && self.is_raw_text_close(pos) {
            self.flush_text_sink(pos, sink);
            self.tag_open_pos = pos;
            self.attr_quote = None;
            self.mode = Mode::InTag;
        }
    }

    /// Parse a complete tag from `<` at `open` to `>` at `close` (sink mode).
    ///
    /// Instead of parsing attributes into a Vec, passes the raw attribute
    /// region to the sink for direct-to-slab parsing.
    fn parse_tag_sink<S: TreeSink>(&mut self, open: usize, close: usize, sink: &mut S) {
        let mut pos = open + 1;
        if pos >= close {
            return;
        }

        let first = self.input[pos];

        // Close tag: `</...>`
        if first == b'/' {
            pos += 1;
            let name_start = pos;
            while pos < close && !is_whitespace(self.input[pos]) && self.input[pos] != b'/' {
                pos += 1;
            }
            let name = self.str_slice(name_start, pos);
            let tag = Tag::from_bytes(&self.input[name_start..pos]);
            sink.close_tag(tag, name);
            return;
        }

        // Open tag: `<name ...>` or `<name ... />`
        let name_start = pos;
        while pos < close
            && !is_whitespace(self.input[pos])
            && self.input[pos] != b'/'
            && self.input[pos] != b'>'
        {
            pos += 1;
        }
        let name = self.str_slice(name_start, pos);
        let tag = Tag::from_bytes(&self.input[name_start..pos]);

        let has_trailing_slash = close > 0 && self.input[close - 1] == b'/';
        let self_closing = has_trailing_slash;

        // Attribute region: from after tag name to before `>` (or `/>`).
        let attr_end = if has_trailing_slash { close - 1 } else { close };
        let attr_raw = self.str_slice(pos, attr_end);

        sink.open_tag(tag, name, attr_raw, self_closing);

        // Enter the appropriate text-element mode.
        if name.eq_ignore_ascii_case("plaintext") {
            self.mode = Mode::InPlainText;
        } else if !tag.is_void() && tag.is_raw_text() {
            self.mode = Mode::InRawText;
            self.raw_text_tag = tag;
        } else if !tag.is_void() && tag.is_rcdata() {
            self.mode = Mode::InRcData;
            self.raw_text_tag = tag;
        }
    }

    /// Flush text from cursor to pos (sink mode — raw, no entity decode).
    #[inline(always)]
    fn flush_text_sink<S: TreeSink>(&mut self, pos: usize, sink: &mut S) {
        if pos > self.cursor {
            let raw = self.str_slice(self.cursor, pos);
            if !raw.is_empty() {
                sink.text(raw);
            }
        }
        self.cursor = pos;
    }

    /// Flush trailing text at end of input (sink mode).
    pub(crate) fn flush_trailing_sink<S: TreeSink>(&mut self, sink: &mut S) {
        let end = self.input.len();
        if end > self.cursor {
            let raw = self.str_slice(self.cursor, end);
            if !raw.is_empty() {
                sink.text(raw);
            }
        }
    }

    /// Flush trailing text at end of input (generic).
    #[inline]
    fn flush_trailing_impl(&mut self, emit: &mut impl FnMut(Token<'a>)) {
        let end = self.input.len();
        if end > self.cursor {
            let raw = self.str_slice(self.cursor, end);
            if !raw.is_empty() {
                let content = if matches!(self.mode, Mode::InRawText | Mode::InPlainText) {
                    Cow::Borrowed(raw)
                } else {
                    maybe_decode_entities(raw)
                };
                emit(Token::Text { content });
            }
        }
    }

    /// Check if `<` at `pos` starts the close tag for the current raw text element.
    fn is_raw_text_close(&self, pos: usize) -> bool {
        let remaining = &self.input[pos..];
        if remaining.len() < 3 {
            return false;
        }
        if remaining[1] != b'/' {
            return false;
        }
        let tag_name = self.raw_text_tag.as_str().unwrap_or("");
        let name_len = tag_name.len();
        if remaining.len() < 2 + name_len + 1 {
            return false;
        }
        let candidate = &remaining[2..2 + name_len];
        if !candidate.eq_ignore_ascii_case(tag_name.as_bytes()) {
            return false;
        }
        let after = remaining[2 + name_len];
        after == b'>' || after == b'/' || is_whitespace(after)
    }

    /// Peek at a byte in the input, returning `None` if out of bounds.
    #[inline(always)]
    fn peek(&self, pos: usize) -> Option<u8> {
        self.input.get(pos).copied()
    }

    /// Check whether the input at `pos` begins the literal `<![CDATA[`.
    ///
    /// `pos` points at `<`; the caller has already matched `<![`. The CDATA
    /// close handler assumes a 9-byte `<![CDATA[` prefix, so we must verify the
    /// full literal before entering CDATA mode. Otherwise the assumed content
    /// offset could fall in the middle of a multi-byte character and produce an
    /// invalid-UTF-8 slice.
    #[inline(always)]
    fn is_cdata_open(&self, pos: usize) -> bool {
        self.input
            .get(pos + 3..pos + 9)
            .is_some_and(|s| s == b"CDATA[")
    }

    /// Get a `&str` slice from the input.
    #[inline(always)]
    fn str_slice(&self, start: usize, end: usize) -> &'a str {
        if start >= end || end > self.input.len() {
            return "";
        }
        self.input_str
            .get(start..end)
            .expect("tokenizer produced a range outside UTF-8 character boundaries")
    }
}

/// Check if a byte is ASCII whitespace.
#[inline(always)]
fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0C | b'\r')
}

#[inline(always)]
fn starts_markup_after_lt(after: Option<u8>, after2: Option<u8>) -> bool {
    match after {
        Some(b'!') | Some(b'?') => true,
        Some(b'/') => after2.is_some_and(is_tag_name_start),
        Some(b) => is_tag_name_start(b),
        None => false,
    }
}

#[inline(always)]
fn is_tag_name_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::StructuralIndexer;

    fn tokenize(html: &str) -> Vec<Token<'_>> {
        let indexer = StructuralIndexer::new();
        let index = indexer.index(html.as_bytes());
        extract_tokens(html, &index)
    }

    #[test]
    fn simple_div() {
        let tokens = tokenize("<div>hello</div>");
        assert!(tokens.len() >= 3, "got {tokens:?}");

        match &tokens[0] {
            Token::OpenTag { tag, name, .. } => {
                assert_eq!(*tag, Tag::Div);
                assert_eq!(name.as_ref(), "div");
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }

        match &tokens[1] {
            Token::Text { content } => {
                assert_eq!(content.as_ref(), "hello");
            }
            other => panic!("expected Text, got {other:?}"),
        }

        match &tokens[2] {
            Token::CloseTag { tag, name } => {
                assert_eq!(*tag, Tag::Div);
                assert_eq!(name.as_ref(), "div");
            }
            other => panic!("expected CloseTag, got {other:?}"),
        }
    }

    #[test]
    fn self_closing_br() {
        let tokens = tokenize("<br/>");
        assert!(!tokens.is_empty(), "got {tokens:?}");
        match &tokens[0] {
            Token::OpenTag {
                tag, self_closing, ..
            } => {
                assert_eq!(*tag, Tag::Br);
                assert!(*self_closing);
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn tag_with_attributes() {
        let tokens = tokenize("<a href=\"url\" class=\"link\">text</a>");

        match &tokens[0] {
            Token::OpenTag { attributes, .. } => {
                assert_eq!(attributes.len(), 2, "attrs: {attributes:?}");
                assert_eq!(attributes[0].name.as_ref(), "href");
                assert_eq!(attributes[0].value.as_deref(), Some("url"));
                assert_eq!(attributes[1].name.as_ref(), "class");
                assert_eq!(attributes[1].value.as_deref(), Some("link"));
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn boolean_attribute() {
        let tokens = tokenize("<input disabled>");
        match &tokens[0] {
            Token::OpenTag { attributes, .. } => {
                assert_eq!(attributes.len(), 1, "attrs: {attributes:?}");
                assert_eq!(attributes[0].name.as_ref(), "disabled");
                assert!(attributes[0].value.is_none());
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn text_only() {
        let tokens = tokenize("just plain text");
        assert_eq!(tokens.len(), 1);
        match &tokens[0] {
            Token::Text { content } => {
                assert_eq!(content.as_ref(), "just plain text");
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn empty_input() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    #[should_panic(expected = "structural index does not match input")]
    fn rejects_structural_index_from_different_input() {
        let indexer = StructuralIndexer::new();
        let index = indexer.index(b"<div>");

        let _ = extract_tokens("xxxxx", &index);
    }

    #[test]
    #[should_panic(expected = "structural index does not match input")]
    fn rejects_structural_index_with_different_length() {
        let indexer = StructuralIndexer::new();
        let index = indexer.index(b"<div>");

        let _ = extract_tokens("<div></div>", &index);
    }

    #[test]
    #[should_panic(expected = "structural index does not match input")]
    fn bytes_reject_structural_index_from_different_input() {
        let indexer = StructuralIndexer::new();
        let index = indexer.index(b"<div>");

        let _ = extract_tokens_bytes(b"xxxxx", &index);
    }

    #[test]
    fn entity_in_text() {
        let tokens = tokenize("a &amp; b");
        match &tokens[0] {
            Token::Text { content } => {
                let expected = if cfg!(feature = "entity-decode") {
                    "a & b"
                } else {
                    "a &amp; b"
                };
                assert_eq!(content.as_ref(), expected);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn comment() {
        let tokens = tokenize("<!-- hello -->");
        assert!(!tokens.is_empty(), "got {tokens:?}");
        let has_comment = tokens.iter().any(|t| matches!(t, Token::Comment { .. }));
        assert!(has_comment, "should have a comment token: {tokens:?}");
    }

    #[test]
    fn doctype() {
        let tokens = tokenize("<!DOCTYPE html>");
        assert!(!tokens.is_empty(), "got {tokens:?}");
        let has_doctype = tokens.iter().any(|t| matches!(t, Token::Doctype { .. }));
        assert!(has_doctype, "should have a doctype token: {tokens:?}");
    }

    #[test]
    fn nested_tags() {
        let tokens = tokenize("<div><span>text</span></div>");

        let names: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::OpenTag { name, .. } => Some(name.as_ref()),
                Token::CloseTag { name, .. } => Some(name.as_ref()),
                _ => None,
            })
            .collect();

        assert!(names.contains(&"div"), "names: {names:?}");
        assert!(names.contains(&"span"), "names: {names:?}");
    }

    #[test]
    fn entity_in_attribute() {
        let tokens = tokenize("<div title=\"a &amp; b\">x</div>");
        match &tokens[0] {
            Token::OpenTag { attributes, .. } => {
                assert_eq!(attributes.len(), 1);
                let expected = if cfg!(feature = "entity-decode") {
                    "a & b"
                } else {
                    "a &amp; b"
                };
                assert_eq!(attributes[0].value.as_deref(), Some(expected));
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn multiple_attributes_mixed() {
        let tokens = tokenize("<div id=\"main\" class='header' disabled data-x=42>");
        match &tokens[0] {
            Token::OpenTag { attributes, .. } => {
                assert_eq!(attributes.len(), 4, "attrs: {attributes:?}");
                assert_eq!(attributes[0].name.as_ref(), "id");
                assert_eq!(attributes[0].value.as_deref(), Some("main"));
                assert_eq!(attributes[1].name.as_ref(), "class");
                assert_eq!(attributes[1].value.as_deref(), Some("header"));
                assert_eq!(attributes[2].name.as_ref(), "disabled");
                assert!(attributes[2].value.is_none());
                assert_eq!(attributes[3].name.as_ref(), "data-x");
                assert_eq!(attributes[3].value.as_deref(), Some("42"));
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_attributes_are_ascii_case_insensitive_first_wins() {
        let tokens = tokenize("<div ID=first id='second' class=a CLASS=b>");
        match &tokens[0] {
            Token::OpenTag { attributes, .. } => {
                assert_eq!(attributes.len(), 2);
                assert_eq!(attributes[0].name.as_ref(), "ID");
                assert_eq!(attributes[0].value.as_deref(), Some("first"));
                assert_eq!(attributes[1].name.as_ref(), "class");
                assert_eq!(attributes[1].value.as_deref(), Some("a"));
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_tracker_handles_inline_and_heap_boundaries() {
        for count in [0usize, 1, 8, 9, 64, 1_000] {
            let mut html = String::from("<div");
            for index in 0..count {
                html.push_str(&format!(" data-{index}={index}"));
            }
            html.push_str(" DATA-0=duplicate>");

            let tokens = tokenize(&html);
            let Token::OpenTag { attributes, .. } = &tokens[0] else {
                panic!("expected OpenTag");
            };
            assert_eq!(attributes.len(), count.max(1));
            if count > 0 {
                assert_eq!(attributes[0].value.as_deref(), Some("0"));
            } else {
                assert_eq!(attributes[0].value.as_deref(), Some("duplicate"));
            }
        }
    }

    #[test]
    fn fingerprint_collision_still_checks_full_name() {
        let candidate = b"beta";
        let fingerprint = ascii_ci_fingerprint(candidate);
        let mut seen = SeenAttributes::new();
        seen.bloom = fingerprint_bloom(fingerprint);
        seen.inline[0].write(SeenAttribute {
            fingerprint,
            name: b"alpha",
        });
        seen.inline_len = 1;

        assert!(!seen.duplicate_or_insert(candidate));
        assert!(seen.duplicate_or_insert(b"BETA"));

        let mut heap_seen = SeenAttributes::new();
        heap_seen.bloom = fingerprint_bloom(fingerprint);
        heap_seen.inline[0].write(SeenAttribute {
            fingerprint,
            name: b"alpha",
        });
        heap_seen.inline_len = 1;
        for name in [
            b"bravo".as_slice(),
            b"charlie".as_slice(),
            b"delta".as_slice(),
            b"echo".as_slice(),
            b"foxtrot".as_slice(),
            b"golf".as_slice(),
            b"hotel".as_slice(),
            b"india".as_slice(),
        ] {
            assert!(!heap_seen.duplicate_or_insert_hashed(name, fingerprint));
        }
        assert!(heap_seen.heap.is_some());
        assert!(!heap_seen.duplicate_or_insert_hashed(candidate, fingerprint));
        assert!(heap_seen.duplicate_or_insert_hashed(b"BETA", fingerprint));
    }

    #[test]
    fn attribute_entities_use_ambiguous_ampersand_rules() {
        let tokens = tokenize("<div title='&copy=test' data-ok='&copy test'>");
        match &tokens[0] {
            Token::OpenTag { attributes, .. } => {
                let expected_ambiguous = "&copy=test";
                let expected_legacy = if cfg!(feature = "entity-decode") {
                    "© test"
                } else {
                    "&copy test"
                };
                assert_eq!(attributes[0].value.as_deref(), Some(expected_ambiguous));
                assert_eq!(attributes[1].value.as_deref(), Some(expected_legacy));
            }
            other => panic!("expected OpenTag, got {other:?}"),
        }
    }

    #[test]
    fn plaintext_treats_everything_after_start_tag_as_literal() {
        let tokens = tokenize("<plaintext><b>x&amp;</plaintext>");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(
            &tokens[1],
            Token::Text { content } if content == "<b>x&amp;</plaintext>"
        ));
    }

    #[test]
    fn script_raw_text() {
        let tokens = tokenize("<script>var x = 1 < 2;</script>");
        let text_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| matches!(t, Token::Text { .. }))
            .collect();
        assert!(
            text_tokens.iter().any(|t| {
                if let Token::Text { content } = t {
                    content.contains("var x = 1 < 2;") || content.contains("var x = 1 ")
                } else {
                    false
                }
            }),
            "should contain script text: {tokens:?}"
        );
    }

    #[test]
    fn comment_content() {
        let tokens = tokenize("<!-- this is a comment -->");
        match tokens.iter().find(|t| matches!(t, Token::Comment { .. })) {
            Some(Token::Comment { content }) => {
                assert_eq!(content.trim(), "this is a comment");
            }
            other => panic!("expected Comment, got {other:?}"),
        }
    }

    #[test]
    fn doctype_content() {
        let tokens = tokenize("<!DOCTYPE html>");
        match tokens.iter().find(|t| matches!(t, Token::Doctype { .. })) {
            Some(Token::Doctype { content }) => {
                assert_eq!(content.as_ref(), "html");
            }
            other => panic!("expected Doctype, got {other:?}"),
        }
    }
}
