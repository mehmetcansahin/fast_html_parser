//! Token extraction — stage 2 of the two-stage tokenizer pipeline.
//!
//! Uses the structural index from stage 1 to locate `<` and `>` boundaries,
//! then scans the actual input bytes between them to extract tag names,
//! attributes, comments, and text content. This hybrid approach combines
//! SIMD-accelerated delimiter finding with scalar content parsing.

use std::borrow::Cow;

use fhp_core::tag::Tag;

use crate::TreeSink;
use crate::structural::StructuralIndex;
use crate::token::{Attribute, Token};

#[cfg(feature = "entity-decode")]
#[inline]
fn maybe_decode_entities<'a>(input: &'a str) -> Cow<'a, str> {
    crate::entity::decode_entities(input)
}

#[cfg(not(feature = "entity-decode"))]
#[inline]
fn maybe_decode_entities<'a>(input: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(input)
}

/// Extract tokens from pre-indexed UTF-8 input.
///
/// `input` must be the same text that was passed to
/// [`StructuralIndexer::index`](crate::structural::StructuralIndexer::index)
/// as bytes.
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
    let mut tokens = Vec::with_capacity(index.estimated_token_count());
    let mut parser = Parser::new(input);

    for delim in index.iter_delimiters() {
        parser.on_delimiter(delim.pos, delim.byte, &mut tokens);
    }

    // Flush trailing text.
    parser.flush_trailing(&mut tokens);

    tokens
}

/// Extract tokens from pre-indexed raw bytes after UTF-8 validation.
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
    /// Inside raw text element (script/style).
    InRawText,
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
            Mode::InRawText => self.on_in_raw_text_impl(pos, byte, emit),
        }
    }

    /// In Data mode: only `<` matters.
    fn on_data_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'<' {
            // Flush text before this `<`.
            self.flush_text_impl(pos, emit);
            self.tag_open_pos = pos;

            // Peek ahead to classify what follows `<`.
            let after = self.peek(pos + 1);
            let after2 = self.peek(pos + 2);

            if after == Some(b'!') {
                // Could be comment, doctype, or CDATA.
                if after2 == Some(b'-') && self.peek(pos + 3) == Some(b'-') {
                    self.mode = Mode::InComment;
                    self.special_open_pos = pos;
                } else if after2.is_some_and(|b| b == b'D' || b == b'd') {
                    self.mode = Mode::InDoctype;
                    self.special_open_pos = pos;
                } else if after2 == Some(b'[') {
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

    /// In tag mode: `>` closes the tag.
    fn on_in_tag_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'>' {
            // Parse the tag content between `<` and `>`.
            self.parse_tag_impl(self.tag_open_pos, pos, emit);
            self.cursor = pos + 1;
            // parse_tag may have set InRawText for script/style — don't override.
            if self.mode != Mode::InRawText {
                self.mode = Mode::Data;
            }
        }
        // Other delimiters inside tags (`=`, `"`, `'`, `/`) are handled
        // during tag parsing when we see `>`.
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

    /// In doctype mode: `>` closes it.
    fn on_in_doctype_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'>' {
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

    /// In raw text mode: only look for `</script>` or `</style>`.
    fn on_in_raw_text_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'<' && self.is_raw_text_close(pos) {
            // Flush raw text content.
            self.flush_text_impl(pos, emit);
            self.tag_open_pos = pos;
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
            while pos < close && !is_whitespace(self.input[pos]) {
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

        // Check for self-closing at the end.
        let self_closing = close > 0 && self.input[close - 1] == b'/' || tag.is_void();

        // Parse attributes.
        let attrs = self.parse_attributes(
            pos,
            if self_closing && close > 0 && self.input[close - 1] == b'/' {
                close - 1
            } else {
                close
            },
        );

        emit(Token::OpenTag {
            tag,
            name: Cow::Borrowed(name),
            attributes: attrs,
            self_closing,
        });

        // Enter raw text mode for script/style.
        if tag.is_raw_text() {
            self.mode = Mode::InRawText;
            self.raw_text_tag = tag;
        }
    }

    /// Parse attributes from the region between tag name and `>`.
    fn parse_attributes(&self, start: usize, end: usize) -> Vec<Attribute<'a>> {
        let estimated = if end > start {
            ((end - start) / 15).clamp(2, 16)
        } else {
            2
        };
        let mut attrs = Vec::with_capacity(estimated);
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
            while pos < end
                && !is_whitespace(self.input[pos])
                && self.input[pos] != b'='
                && self.input[pos] != b'/'
                && self.input[pos] != b'>'
            {
                pos += 1;
            }
            let name_end = pos;
            if name_start == name_end {
                pos += 1;
                continue;
            }
            let attr_name = self.str_slice(name_start, name_end);

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
                    let value = maybe_decode_entities(raw_value);
                    attrs.push(Attribute {
                        name: Cow::Borrowed(attr_name),
                        value: Some(value),
                    });
                } else {
                    // Unquoted value.
                    let val_start = pos;
                    while pos < end && !is_whitespace(self.input[pos]) && self.input[pos] != b'>' {
                        pos += 1;
                    }
                    let raw_value = self.str_slice(val_start, pos);
                    let value = maybe_decode_entities(raw_value);
                    attrs.push(Attribute {
                        name: Cow::Borrowed(attr_name),
                        value: Some(value),
                    });
                }
            } else {
                // Boolean attribute (no value).
                attrs.push(Attribute {
                    name: Cow::Borrowed(attr_name),
                    value: None,
                });
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
            Mode::InRawText => self.on_in_raw_text_sink(pos, byte, sink),
        }
    }

    /// In Data mode (sink): only `<` matters.
    fn on_data_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'<' {
            self.flush_text_sink(pos, sink);
            self.tag_open_pos = pos;

            let after = self.peek(pos + 1);
            let after2 = self.peek(pos + 2);

            if after == Some(b'!') {
                if after2 == Some(b'-') && self.peek(pos + 3) == Some(b'-') {
                    self.mode = Mode::InComment;
                    self.special_open_pos = pos;
                } else if after2.is_some_and(|b| b == b'D' || b == b'd') {
                    self.mode = Mode::InDoctype;
                    self.special_open_pos = pos;
                } else if after2 == Some(b'[') {
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

    /// In tag mode (sink): `>` closes the tag.
    fn on_in_tag_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'>' {
            self.parse_tag_sink(self.tag_open_pos, pos, sink);
            self.cursor = pos + 1;
            if self.mode != Mode::InRawText {
                self.mode = Mode::Data;
            }
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

    /// In doctype mode (sink): `>` closes it.
    fn on_in_doctype_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'>' {
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

    /// In raw text mode (sink): only look for `</script>` or `</style>`.
    fn on_in_raw_text_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'<' && self.is_raw_text_close(pos) {
            self.flush_text_sink(pos, sink);
            self.tag_open_pos = pos;
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
            while pos < close && !is_whitespace(self.input[pos]) {
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
        let self_closing = has_trailing_slash || tag.is_void();

        // Attribute region: from after tag name to before `>` (or `/>`).
        let attr_end = if has_trailing_slash { close - 1 } else { close };
        let attr_raw = self.str_slice(pos, attr_end);

        sink.open_tag(tag, name, attr_raw, self_closing);

        // Enter raw text mode for script/style.
        if tag.is_raw_text() {
            self.mode = Mode::InRawText;
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
    #[inline(always)]
    fn flush_trailing_impl(&mut self, emit: &mut impl FnMut(Token<'a>)) {
        let end = self.input.len();
        if end > self.cursor {
            let raw = self.str_slice(self.cursor, end);
            if !raw.is_empty() {
                let content = maybe_decode_entities(raw);
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
        after == b'>' || is_whitespace(after)
    }

    /// Peek at a byte in the input, returning `None` if out of bounds.
    #[inline]
    fn peek(&self, pos: usize) -> Option<u8> {
        self.input.get(pos).copied()
    }

    /// Get a `&str` slice from the input.
    #[inline]
    fn str_slice(&self, start: usize, end: usize) -> &'a str {
        if start >= end || end > self.input.len() {
            return "";
        }
        debug_assert!(self.input_str.is_char_boundary(start));
        debug_assert!(self.input_str.is_char_boundary(end));
        // SAFETY: `start/end` are derived from ASCII delimiter boundaries
        // and parser cursor positions, which are UTF-8 char boundaries for
        // a validated `&str` input.
        unsafe { self.input_str.get_unchecked(start..end) }
    }
}

/// Check if a byte is ASCII whitespace.
#[inline(always)]
fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
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
    fn entity_in_text() {
        let tokens = tokenize("a &amp; b");
        match &tokens[0] {
            Token::Text { content } => {
                assert_eq!(content.as_ref(), "a & b");
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
                assert_eq!(attributes[0].value.as_deref(), Some("a & b"));
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
