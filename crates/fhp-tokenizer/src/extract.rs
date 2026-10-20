//! Token extraction — stage 2 of the two-stage tokenizer pipeline.
//!
//! Uses the structural index from stage 1 to locate `<` and `>` boundaries,
//! then scans the actual input bytes between them to extract tag names,
//! attributes, comments, and text content. This hybrid approach combines
//! SIMD-accelerated delimiter finding with scalar content parsing.

use std::borrow::Cow;

use fhp_core::attr::{SeenAttributes, TagScanState, iter_raw_attrs, tag_name_offset};
use fhp_core::is_html_whitespace;
use fhp_core::tag::Tag;

use crate::TreeSink;
use crate::structural::StructuralIndex;
use crate::token::{Attribute, Token};

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

/// Where [`tokenize_prefix`] stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrefixEnd {
    /// A start tag entered the text element `Tag` (or [`Tag::Plaintext`]);
    /// its text starts at the stop's `consumed` offset.
    TextElement(Tag),
    /// The `<` at this offset needs more input to classify. The bytes before
    /// it, from `consumed`, are text.
    Undecided(usize),
    /// Every delimiter was processed. Unfinished markup starts at `consumed`
    /// when `in_markup` is set; otherwise the rest of the input is text.
    End { in_markup: bool },
}

/// Result of [`tokenize_prefix`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrefixStop {
    /// Length of the input prefix whose tokens have all been emitted.
    pub(crate) consumed: usize,
    pub(crate) end: PrefixEnd,
}

/// Tokenize the complete markup at the start of `input`, which is followed by
/// more input that has not arrived yet.
///
/// This is the fused single-pass loop of [`crate::tokenize_with`] without the
/// end-of-input assumption: a `<` is only classified once enough bytes follow
/// it, trailing text is not flushed, and an unfinished construct is left
/// unemitted. It stops early when a start tag enters a text element, because
/// the streaming tokenizer bounds that text itself. The emitted tokens are
/// the leading tokens the one-shot tokenizer emits for any document that
/// starts with `input`.
pub(crate) fn tokenize_prefix<'a>(input: &'a str, emit: &mut impl FnMut(Token<'a>)) -> PrefixStop {
    const BLOCK_SIZE: usize = 64;

    let bytes = input.as_bytes();
    let compute = fhp_simd::dispatch::ops().compute_all_masks;
    let mut parser = Parser::new(input);

    let mut block_start = 0;
    while block_start < bytes.len() {
        let block_end = (block_start + BLOCK_SIZE).min(bytes.len());
        // SAFETY: dispatch function pointer matches detected CPU features.
        let masks = unsafe { compute(&bytes[block_start..block_end]) };
        let combined = masks.lt | masks.gt | masks.quot | masks.apos;
        let valid_bits = block_end - block_start;
        let mut bits = if valid_bits < 64 {
            combined & ((1u64 << valid_bits) - 1)
        } else {
            combined
        };
        while bits != 0 {
            let pos = block_start + bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let byte = bytes[pos];

            if parser.mode == Mode::Data {
                if byte != b'<' {
                    continue;
                }
                match classify_lt(&bytes[pos..], false) {
                    None => {
                        return PrefixStop {
                            consumed: parser.cursor,
                            end: PrefixEnd::Undecided(pos),
                        };
                    }
                    Some(LtKind::Text) => {}
                    Some(kind) => {
                        parser.flush_text_impl(pos, emit);
                        parser.enter_markup(pos, kind);
                    }
                }
                continue;
            }

            parser.on_delimiter_impl(pos, byte, emit);
            let text_element = match parser.mode {
                Mode::InRawText | Mode::InRcData => parser.raw_text_tag,
                Mode::InPlainText => Tag::Plaintext,
                _ => continue,
            };
            return PrefixStop {
                consumed: parser.cursor,
                end: PrefixEnd::TextElement(text_element),
            };
        }
        block_start = block_end;
    }

    PrefixStop {
        consumed: parser.cursor,
        end: PrefixEnd::End {
            in_markup: parser.mode != Mode::Data,
        },
    }
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
    /// Inside a doctype (`<!DOCTYPE ...>`). The first `>` ends it.
    InDoctype,
    /// Inside a CDATA section (`<![CDATA[ ... ]]>`).
    InCData,
    /// Inside a bogus comment (`<?...>`, `<!x...>`, `</ x...>`). The first
    /// `>` ends it.
    InBogusComment,
    /// Inside a raw text element (script/style/iframe/xmp/noembed/noframes).
    InRawText,
    /// Inside an RCDATA element (title/textarea).
    InRcData,
    /// After a `<plaintext>` start tag. Everything through EOF is text.
    InPlainText,
}

/// What a `<` in the data state starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LtKind {
    /// Not markup: the `<` is literal text.
    Text,
    /// A start or end tag (`<a`, `</a`).
    Tag,
    /// A comment (`<!--`).
    Comment,
    /// A CDATA section (`<![CDATA[`).
    CData,
    /// A doctype (`<!DOCTYPE`, ASCII case-insensitive).
    Doctype,
    /// A bogus comment (`<?`, `<!x`, `</ x`) whose data starts `data_offset`
    /// bytes after the `<` and ends at the first `>`.
    BogusComment { data_offset: usize },
    /// `</>`, which produces no token.
    EmptyEndTag,
}

/// Classify the markup started by `bytes[0] == b'<'` in the data state.
///
/// Returns `None` when more bytes are needed to decide; this only happens
/// when `!at_eof`.
pub(crate) fn classify_lt(bytes: &[u8], at_eof: bool) -> Option<LtKind> {
    debug_assert_eq!(bytes.first(), Some(&b'<'));
    let Some(&next) = bytes.get(1) else {
        return at_eof.then_some(LtKind::Text);
    };
    match next {
        b'!' => {
            let rest = &bytes[2..];
            if rest.starts_with(b"--") {
                return Some(LtKind::Comment);
            }
            if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"DOCTYPE") {
                return Some(LtKind::Doctype);
            }
            if rest.starts_with(b"[CDATA[") {
                return Some(LtKind::CData);
            }
            let undecided = (rest.len() < 2 && b"--".starts_with(rest))
                || (rest.len() < 7 && rest.eq_ignore_ascii_case(&b"DOCTYPE"[..rest.len()]))
                || (rest.len() < 7 && b"[CDATA[".starts_with(rest));
            if undecided && !at_eof {
                return None;
            }
            Some(LtKind::BogusComment { data_offset: 2 })
        }
        b'?' => Some(LtKind::BogusComment { data_offset: 1 }),
        b'/' => match bytes.get(2) {
            None => at_eof.then_some(LtKind::Text),
            Some(byte) if byte.is_ascii_alphabetic() => Some(LtKind::Tag),
            Some(b'>') => Some(LtKind::EmptyEndTag),
            Some(_) => Some(LtKind::BogusComment { data_offset: 2 }),
        },
        byte if byte.is_ascii_alphabetic() => Some(LtKind::Tag),
        _ => Some(LtKind::Text),
    }
}

/// Script-data escape state (WHATWG "script data escaped" and "script data
/// double escaped" states) inside `<script>` raw text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ScriptEscape {
    /// Plain script data: `</script` ends the element.
    #[default]
    None,
    /// After `<!--`: `</script` still ends the element, `<script` nests.
    Escaped,
    /// After `<!--<script`: `</script` only returns to [`ScriptEscape::Escaped`].
    DoubleEscaped,
}

/// Handle `data[pos] == b'<'` inside script text.
///
/// Returns the next escape state and whether the `<` starts the script's end
/// tag.
pub(crate) fn script_lt_event(
    state: ScriptEscape,
    data: &[u8],
    pos: usize,
) -> (ScriptEscape, bool) {
    let rest = &data[pos..];
    if state == ScriptEscape::None
        && rest.starts_with(b"<!--")
        && rest.get(4) != Some(&b'>')
        && rest.get(4..6) != Some(b"->".as_slice())
    {
        return (ScriptEscape::Escaped, false);
    }
    if rest.get(1) == Some(&b'/') && starts_tag_named(rest, 2, b"script") {
        return match state {
            ScriptEscape::DoubleEscaped => (ScriptEscape::Escaped, false),
            _ => (ScriptEscape::None, true),
        };
    }
    if state == ScriptEscape::Escaped && starts_tag_named(rest, 1, b"script") {
        return (ScriptEscape::DoubleEscaped, false);
    }
    (state, false)
}

/// Handle `data[pos] == b'>'` inside script text: `-->` leaves both escape
/// states.
pub(crate) fn script_gt_event(state: ScriptEscape, data: &[u8], pos: usize) -> ScriptEscape {
    if state != ScriptEscape::None && pos >= 2 && data[pos - 2..pos] == *b"--" {
        ScriptEscape::None
    } else {
        state
    }
}

/// Whether `data[pos..]` (starting at `<`) is the end tag `</name` (ASCII
/// case-insensitive) followed by `>`, `/`, or whitespace.
#[inline]
pub(crate) fn is_end_tag_at(data: &[u8], pos: usize, name: &[u8]) -> bool {
    let rest = &data[pos..];
    rest.get(1) == Some(&b'/') && starts_tag_named(rest, 2, name)
}

/// Whether `rest[name_start..]` is `name` (ASCII case-insensitive) followed
/// by a tag-name terminator (`>`, `/`, or whitespace).
#[inline]
fn starts_tag_named(rest: &[u8], name_start: usize, name: &[u8]) -> bool {
    let name_end = name_start + name.len();
    rest.get(name_start..name_end)
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        && rest
            .get(name_end)
            .is_some_and(|&byte| byte == b'>' || byte == b'/' || is_html_whitespace(byte))
}

/// Direct input parser driven by structural delimiter positions.
///
/// It only reacts to `<`, `>`, `"`, and `'`; any other delimiter reported by
/// an index is ignored. Can be used in two modes:
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
    /// Position of the `<!` that opened a comment, doctype, or CDATA section.
    special_open_pos: usize,
    /// Current text-element tag, or `Unknown` after its closing tag completes.
    raw_text_tag: Tag,
    /// Attribute-grammar state of the current tag, exact at `tag_scan_pos`.
    ///
    /// Only the bytes between quote delimiters are walked (lazily, when the
    /// next quote or a `>` after `/` needs the exact state), so every tag
    /// byte is visited at most once.
    tag_scan: TagScanState,
    /// Input position `tag_scan` describes.
    tag_scan_pos: usize,
    /// Start of the current bogus comment's data.
    bogus_data_start: usize,
    /// Escape state while inside `<script>` text.
    script_escape: ScriptEscape,
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
            tag_scan: TagScanState::START,
            tag_scan_pos: 0,
            bogus_data_start: 0,
            script_escape: ScriptEscape::None,
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
            Mode::InBogusComment => self.on_in_bogus_comment_impl(pos, byte, emit),
            Mode::InRawText | Mode::InRcData => self.on_in_raw_text_impl(pos, byte, emit),
            Mode::InPlainText => {}
        }
    }

    /// In Data mode: only `<` matters.
    #[inline(always)]
    fn on_data_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if let Some(kind) = self.data_markup(pos, byte) {
            self.flush_text_impl(pos, emit);
            self.enter_markup(pos, kind);
        }
    }

    /// In tag mode: `>` closes the tag unless it sits inside a quoted
    /// attribute value.
    #[inline(always)]
    fn on_in_tag_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if let Some(self_closing) = self.tag_delimiter(pos, byte) {
            self.parse_tag_impl(self.tag_open_pos, pos, self_closing, emit);
            self.finish_tag(pos);
        }
    }

    /// In comment mode: look for `-->` or `--!>`.
    fn on_in_comment_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if let Some(content_end) = self.comment_close(pos, byte) {
            emit(Token::Comment {
                content: Cow::Borrowed(self.str_slice(self.special_open_pos + 4, content_end)),
            });
            self.end_special(pos);
        }
    }

    /// In doctype mode: the first `>` closes it.
    fn on_in_doctype_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'>' {
            emit(Token::Doctype {
                content: Cow::Borrowed(self.doctype_content(pos)),
            });
            self.end_special(pos);
        }
    }

    /// In CDATA mode: look for `]]>`.
    fn on_in_cdata_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if self.cdata_close(pos, byte) {
            emit(Token::CData {
                content: Cow::Borrowed(self.str_slice(self.special_open_pos + 9, pos - 2)),
            });
            self.end_special(pos);
        }
    }

    /// In bogus-comment mode: the first `>` closes it.
    fn on_in_bogus_comment_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if byte == b'>' {
            emit(Token::Comment {
                content: Cow::Borrowed(self.str_slice(self.bogus_data_start, pos)),
            });
            self.end_special(pos);
        }
    }

    /// In raw text/RCDATA mode, only the matching end tag is markup.
    fn on_in_raw_text_impl(&mut self, pos: usize, byte: u8, emit: &mut impl FnMut(Token<'a>)) {
        if self.text_element_close(pos, byte) {
            if self.mode == Mode::InRcData {
                self.flush_text_impl(pos, emit);
            } else {
                self.flush_text_raw_impl(pos, emit);
            }
            self.begin_tag(pos);
        }
    }

    /// Parse a complete tag from `<` at `open` to `>` at `close`.
    fn parse_tag_impl(
        &mut self,
        open: usize,
        close: usize,
        self_closing: bool,
        emit: &mut impl FnMut(Token<'a>),
    ) {
        if self.input[open + 1] == b'/' {
            let (tag, name) = self.close_tag_name(open, close);
            self.leave_text_element();
            emit(Token::CloseTag {
                tag,
                name: Cow::Borrowed(name),
            });
            return;
        }

        let (name_end, tag, name) = self.open_tag_name(open, close);
        let attr_end = if self_closing { close - 1 } else { close };
        let attributes = self.parse_attributes(name_end, attr_end);
        emit(Token::OpenTag {
            tag,
            name: Cow::Borrowed(name),
            attributes,
            self_closing,
        });
        self.enter_text_element(tag);
    }

    /// Parse attributes from the region between tag name and `>` (or `/>`).
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

        for attr in iter_raw_attrs(self.str_slice(start, end)) {
            if seen.duplicate_or_insert(attr.name.as_bytes(), attr.fingerprint) {
                continue;
            }
            attrs.push(Attribute {
                name: Cow::Borrowed(attr.name),
                value: attr.value.map(maybe_decode_attribute_entities),
            });
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

    /// Emit whatever the current mode holds at end of input.
    #[inline]
    fn flush_trailing_impl(&mut self, emit: &mut impl FnMut(Token<'a>)) {
        let end = self.input.len();
        match self.mode {
            Mode::Data | Mode::InRcData => self.flush_text_impl(end, emit),
            Mode::InRawText | Mode::InPlainText => self.flush_text_raw_impl(end, emit),
            // EOF inside a tag drops the tag (WHATWG "eof-in-tag").
            Mode::InTag => {}
            Mode::InComment => emit(Token::Comment {
                content: Cow::Borrowed(self.eof_comment_content()),
            }),
            Mode::InCData => emit(Token::CData {
                content: Cow::Borrowed(self.str_slice(self.special_open_pos + 9, end)),
            }),
            Mode::InDoctype => emit(Token::Doctype {
                content: Cow::Borrowed(self.doctype_content(end)),
            }),
            Mode::InBogusComment => emit(Token::Comment {
                content: Cow::Borrowed(self.str_slice(self.bogus_data_start, end)),
            }),
        }
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
            Mode::InBogusComment => self.on_in_bogus_comment_sink(pos, byte, sink),
            Mode::InRawText | Mode::InRcData => self.on_in_raw_text_sink(pos, byte, sink),
            Mode::InPlainText => {}
        }
    }

    /// In Data mode (sink): only `<` matters.
    #[inline(always)]
    fn on_data_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if let Some(kind) = self.data_markup(pos, byte) {
            self.flush_text_sink(pos, sink);
            self.enter_markup(pos, kind);
        }
    }

    /// In tag mode (sink): `>` closes the tag unless it sits inside a quoted
    /// attribute value.
    #[inline(always)]
    fn on_in_tag_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if let Some(self_closing) = self.tag_delimiter(pos, byte) {
            self.parse_tag_sink(self.tag_open_pos, pos, self_closing, sink);
            self.finish_tag(pos);
        }
    }

    /// In comment mode (sink): look for `-->` or `--!>`.
    fn on_in_comment_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if let Some(content_end) = self.comment_close(pos, byte) {
            sink.comment(self.str_slice(self.special_open_pos + 4, content_end));
            self.end_special(pos);
        }
    }

    /// In doctype mode (sink): the first `>` closes it.
    fn on_in_doctype_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'>' {
            sink.doctype(self.doctype_content(pos));
            self.end_special(pos);
        }
    }

    /// In CDATA mode (sink): look for `]]>`.
    fn on_in_cdata_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if self.cdata_close(pos, byte) {
            sink.cdata(self.str_slice(self.special_open_pos + 9, pos - 2));
            self.end_special(pos);
        }
    }

    /// In bogus-comment mode (sink): the first `>` closes it.
    fn on_in_bogus_comment_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if byte == b'>' {
            sink.comment(self.str_slice(self.bogus_data_start, pos));
            self.end_special(pos);
        }
    }

    /// In raw text/RCDATA mode (sink), only the matching end tag is markup.
    fn on_in_raw_text_sink<S: TreeSink>(&mut self, pos: usize, byte: u8, sink: &mut S) {
        if self.text_element_close(pos, byte) {
            self.flush_text_sink(pos, sink);
            self.begin_tag(pos);
        }
    }

    /// Parse a complete tag from `<` at `open` to `>` at `close` (sink mode).
    ///
    /// Instead of parsing attributes into a Vec, passes the raw attribute
    /// region to the sink for direct-to-slab parsing.
    fn parse_tag_sink<S: TreeSink>(
        &mut self,
        open: usize,
        close: usize,
        self_closing: bool,
        sink: &mut S,
    ) {
        if self.input[open + 1] == b'/' {
            let (tag, name) = self.close_tag_name(open, close);
            self.leave_text_element();
            sink.close_tag(tag, name);
            return;
        }

        let (name_end, tag, name) = self.open_tag_name(open, close);
        // Attribute region: from after tag name to before `>` (or `/>`).
        let attr_end = if self_closing { close - 1 } else { close };
        sink.open_tag(tag, name, self.str_slice(name_end, attr_end), self_closing);
        self.enter_text_element(tag);
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

    /// Emit whatever the current mode holds at end of input (sink mode).
    pub(crate) fn flush_trailing_sink<S: TreeSink>(&mut self, sink: &mut S) {
        let end = self.input.len();
        match self.mode {
            Mode::Data | Mode::InRcData | Mode::InRawText | Mode::InPlainText => {
                self.flush_text_sink(end, sink);
            }
            // EOF inside a tag drops the tag (WHATWG "eof-in-tag").
            Mode::InTag => {}
            Mode::InComment => sink.comment(self.eof_comment_content()),
            Mode::InCData => sink.cdata(self.str_slice(self.special_open_pos + 9, end)),
            Mode::InDoctype => sink.doctype(self.doctype_content(end)),
            Mode::InBogusComment => sink.comment(self.str_slice(self.bogus_data_start, end)),
        }
    }

    // ---- Shared state transitions ----

    /// The markup kind started by a data-state delimiter, or `None` when it
    /// is text.
    #[inline(always)]
    fn data_markup(&self, pos: usize, byte: u8) -> Option<LtKind> {
        if byte != b'<' {
            return None;
        }
        // The whole input is available, so classification never waits.
        match classify_lt(&self.input[pos..], true) {
            None | Some(LtKind::Text) => None,
            kind => kind,
        }
    }

    /// Enter the mode for markup starting at `pos` (text before `pos` has
    /// already been flushed).
    #[inline(always)]
    fn enter_markup(&mut self, pos: usize, kind: LtKind) {
        match kind {
            LtKind::Text => {}
            LtKind::Tag => self.begin_tag(pos),
            LtKind::Comment => {
                self.mode = Mode::InComment;
                self.special_open_pos = pos;
            }
            LtKind::CData => {
                self.mode = Mode::InCData;
                self.special_open_pos = pos;
            }
            LtKind::Doctype => {
                self.mode = Mode::InDoctype;
                self.special_open_pos = pos;
            }
            LtKind::BogusComment { data_offset } => {
                self.mode = Mode::InBogusComment;
                self.bogus_data_start = pos + data_offset;
            }
            LtKind::EmptyEndTag => self.cursor = pos + 3,
        }
    }

    /// Enter tag mode for the `<` at `pos`.
    #[inline(always)]
    fn begin_tag(&mut self, pos: usize) {
        self.mode = Mode::InTag;
        self.tag_open_pos = pos;
        self.tag_scan = TagScanState::START;
        self.tag_scan_pos = pos + tag_name_offset(&self.input[pos..]);
    }

    /// Advance `tag_scan` over the bytes up to (excluding) `pos`.
    #[inline(always)]
    fn tag_scan_to(&mut self, pos: usize) {
        self.tag_scan = self.tag_scan.advance(&self.input[self.tag_scan_pos..pos]);
        self.tag_scan_pos = pos;
    }

    /// Feed a tag-mode delimiter into the attribute grammar. Returns
    /// `Some(self_closing)` when `byte` is the `>` that ends the tag.
    #[inline(always)]
    fn tag_delimiter(&mut self, pos: usize, byte: u8) -> Option<bool> {
        match byte {
            b'"' | b'\'' => {
                if let TagScanState::Quoted(quote) = self.tag_scan {
                    // The other quote character is literal inside a value.
                    if byte == quote {
                        self.tag_scan = TagScanState::BeforeAttrName;
                    }
                } else {
                    self.tag_scan_to(pos);
                    self.tag_scan = self.tag_scan.step(byte);
                }
                self.tag_scan_pos = pos + 1;
                None
            }
            // Between quote delimiters the state cannot become `Quoted`, so
            // the possibly stale state still answers this exactly.
            b'>' if !self.tag_scan.gt_is_literal() => {
                // Only a tag ending in `/` needs the exact state: a `/` inside
                // an unquoted value belongs to the value.
                let self_closing = self.input[pos - 1] == b'/' && {
                    self.tag_scan_to(pos);
                    self.tag_scan != TagScanState::Unquoted
                };
                Some(self_closing)
            }
            _ => None,
        }
    }

    /// Leave tag mode after the tag ending at `close` was emitted.
    #[inline(always)]
    fn finish_tag(&mut self, close: usize) {
        self.cursor = close + 1;
        // The tag may have entered a text-element mode — don't override.
        if self.mode == Mode::InTag {
            self.mode = Mode::Data;
        }
    }

    /// Return to data mode after a comment, doctype, CDATA section, or bogus
    /// comment ending at `close`.
    #[inline(always)]
    fn end_special(&mut self, close: usize) {
        self.cursor = close + 1;
        self.mode = Mode::Data;
    }

    /// Content end of the current comment if the `>` at `pos` closes it.
    ///
    /// `-->` may reuse the opening dashes (`<!-->`, `<!--->`); `--!>` may not.
    #[inline]
    fn comment_close(&self, pos: usize, byte: u8) -> Option<usize> {
        if byte != b'>' {
            return None;
        }
        // `<!--` contains no delimiter, so `pos >= special_open_pos + 4`.
        if self.input[pos - 2..pos] == *b"--" {
            return Some(pos - 2);
        }
        if pos - 3 >= self.special_open_pos + 4 && self.input[pos - 3..pos] == *b"--!" {
            return Some(pos - 3);
        }
        None
    }

    /// Whether the `>` at `pos` ends the current CDATA section.
    #[inline]
    fn cdata_close(&self, pos: usize, byte: u8) -> bool {
        // `<![CDATA[` contains no delimiter, so `pos >= special_open_pos + 9`.
        byte == b'>' && self.input[pos - 2..pos] == *b"]]"
    }

    /// Doctype content between `<!` and `end`, without the `DOCTYPE` keyword
    /// or surrounding HTML whitespace.
    fn doctype_content(&self, end: usize) -> &'a str {
        let is_html_space = |ch: char| ch.is_ascii() && is_html_whitespace(ch as u8);
        let content = self
            .str_slice(self.special_open_pos + 2, end)
            .trim_matches(is_html_space);
        if content.len() >= 7 && content.as_bytes()[..7].eq_ignore_ascii_case(b"DOCTYPE") {
            content[7..].trim_start_matches(is_html_space)
        } else {
            content
        }
    }

    /// Content of a comment cut off by end of input: a trailing partial
    /// terminator (`-`, `--`, or `--!`) is not part of the data.
    fn eof_comment_content(&self) -> &'a str {
        let start = self.special_open_pos + 4;
        let data = &self.input[start..];
        let trim = if data.ends_with(b"--!") {
            3
        } else {
            data.iter()
                .rev()
                .take(2)
                .take_while(|&&byte| byte == b'-')
                .count()
        };
        self.str_slice(start, self.input.len() - trim)
    }

    /// Whether the `<` at `pos` inside a text element starts its end tag.
    /// Inside `<script>` this also tracks the script-data escape states.
    #[inline]
    fn text_element_close(&mut self, pos: usize, byte: u8) -> bool {
        if self.raw_text_tag == Tag::Script {
            match byte {
                b'<' => {
                    let (state, closes) = script_lt_event(self.script_escape, self.input, pos);
                    self.script_escape = state;
                    closes
                }
                b'>' => {
                    self.script_escape = script_gt_event(self.script_escape, self.input, pos);
                    false
                }
                _ => false,
            }
        } else {
            byte == b'<' && self.is_raw_text_close(pos)
        }
    }

    /// Enter the text mode a start tag implies, if any.
    #[inline]
    fn enter_text_element(&mut self, tag: Tag) {
        if tag == Tag::Plaintext {
            self.mode = Mode::InPlainText;
        } else if !tag.is_void() && tag.is_raw_text() {
            self.mode = Mode::InRawText;
            self.raw_text_tag = tag;
            self.script_escape = ScriptEscape::None;
        } else if !tag.is_void() && tag.is_rcdata() {
            self.mode = Mode::InRcData;
            self.raw_text_tag = tag;
        }
    }

    /// Forget the text element after an end tag.
    #[inline]
    fn leave_text_element(&mut self) {
        self.raw_text_tag = Tag::Unknown;
        self.script_escape = ScriptEscape::None;
    }

    /// Tag and name of the end tag `</name ...>` spanning `open..=close`.
    #[inline]
    fn close_tag_name(&self, open: usize, close: usize) -> (Tag, &'a str) {
        let start = open + 2;
        let mut end = start;
        while end < close && !is_html_whitespace(self.input[end]) && self.input[end] != b'/' {
            end += 1;
        }
        (
            Tag::from_bytes(&self.input[start..end]),
            self.str_slice(start, end),
        )
    }

    /// Name end, tag, and name of the start tag `<name ...>` spanning
    /// `open..=close`.
    #[inline]
    fn open_tag_name(&self, open: usize, close: usize) -> (usize, Tag, &'a str) {
        let start = open + 1;
        let mut end = start;
        while end < close && !is_html_whitespace(self.input[end]) && self.input[end] != b'/' {
            end += 1;
        }
        (
            end,
            Tag::from_bytes(&self.input[start..end]),
            self.str_slice(start, end),
        )
    }

    /// Check if `<` at `pos` starts the close tag for the current raw text element.
    #[inline]
    fn is_raw_text_close(&self, pos: usize) -> bool {
        let name = self.raw_text_tag.as_str().unwrap_or("");
        is_end_tag_at(self.input, pos, name.as_bytes())
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

    #[test]
    fn doctype_content_trims_only_html_whitespace() {
        // Vertical tab, NBSP, and U+3000 are not HTML whitespace.
        for (input, expected) in [
            ("<!DOCTYPE\u{b}>", "\u{b}"),
            ("<!DOCTYPE html\u{a0}>", "html\u{a0}"),
            ("<!DOCTYPE\u{3000}html>", "\u{3000}html"),
            ("<!DOCTYPE \t\n\x0c\rhtml \t\n\x0c\r>", "html"),
        ] {
            match tokenize(input).as_slice() {
                [Token::Doctype { content }] => assert_eq!(content, expected, "{input:?}"),
                other => panic!("expected one Doctype for {input:?}, got {other:?}"),
            }
        }
    }
}
