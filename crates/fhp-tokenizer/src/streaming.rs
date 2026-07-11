//! Streaming (chunk-based) tokenizer.
//!
//! [`StreamTokenizer`](crate::streaming::StreamTokenizer) processes input in arbitrary-sized chunks, carrying
//! state across chunk boundaries. This enables parsing large files or
//! network streams without loading the entire document into memory.

use crate::extract::extract_tokens;
use crate::structural::StructuralIndexer;
use crate::token::Token;
use fhp_core::tag::Tag;

/// Maximum size of the residual buffer.
///
/// When a chunk boundary falls in the middle of a tag, we buffer up to
/// this many bytes and prepend them to the next chunk.
const MAX_RESIDUAL: usize = 4096;

/// Suffix retained when streaming text so split entities and close tags can
/// still be recognized on the next feed.
const TEXT_TAIL: usize = 80;

/// Hard upper bound on the residual buffer, enforced even inside raw-text
/// (`<script>`/`<style>`) context.
///
/// Without it, an unterminated raw-text element would let the residual grow
/// without limit (a denial-of-service vector). Legitimate inline scripts and
/// styles are far smaller than this. If a single comment, CDATA section,
/// doctype, or quoted tag crosses the bound, its bytes are emitted as inert
/// text while the scanner preserves syntax state through the real terminator.
pub const MAX_RAW_TEXT_RESIDUAL: usize = 2 * 1024 * 1024;

/// A streaming tokenizer that processes input chunk by chunk.
///
/// Maintains internal state so that token boundaries that span chunk
/// boundaries are handled correctly.
///
/// # Example
///
/// ```
/// use fhp_tokenizer::streaming::StreamTokenizer;
/// use fhp_tokenizer::token::Token;
///
/// let mut tokenizer = StreamTokenizer::new();
/// let mut all_tokens: Vec<Token<'static>> = Vec::new();
///
/// let html = b"<div>hello</div>";
/// // Feed in small chunks.
/// let owned = tokenizer.feed(&html[..5]);
/// all_tokens.extend(owned);
/// let owned = tokenizer.feed(&html[5..]);
/// all_tokens.extend(owned);
/// let owned = tokenizer.finish();
/// all_tokens.extend(owned);
///
/// assert!(all_tokens.iter().any(|t| matches!(t, Token::OpenTag { .. })));
/// ```
pub struct StreamTokenizer {
    indexer: StructuralIndexer,
    /// Residual bytes from the previous chunk (partial tag).
    residual: Vec<u8>,
    /// Incremental safe-boundary scanner. Keeping this state avoids rescanning
    /// and recopying the complete residual on every small feed.
    scanner: SplitScanner,
    /// An oversized non-text syntactic token is being emitted as inert text.
    /// The scanner remains in its original mode until the real terminator so
    /// nested markup cannot escape and become active tokens.
    syntax_overflow: bool,
    /// Matching raw-text/RCDATA close tag whose oversized attribute/whitespace
    /// suffix is being discarded while its token identity is retained.
    raw_close_overflow: Option<Tag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SplitScan {
    split: usize,
    in_raw_text_context: bool,
    /// Start of a matching raw-text/RCDATA close tag within `split`, together
    /// with the element whose preceding bytes are text.
    text_close: Option<(usize, Tag)>,
}

impl StreamTokenizer {
    /// Create a new streaming tokenizer.
    pub fn new() -> Self {
        Self {
            indexer: StructuralIndexer::new(),
            residual: Vec::with_capacity(256),
            scanner: SplitScanner::default(),
            syntax_overflow: false,
            raw_close_overflow: None,
        }
    }

    /// Number of bytes currently buffered as residual (not yet processed).
    ///
    /// Bounded by [`MAX_RAW_TEXT_RESIDUAL`] plus the size of one fed chunk.
    pub fn buffered_len(&self) -> usize {
        self.residual.len()
    }

    /// Feed a chunk of UTF-8 input and return any complete tokens.
    ///
    /// Tokens are returned as owned (`'static` lifetime) since the chunk
    /// data may not live long enough. Text content is cloned into `Cow::Owned`.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Token<'static>> {
        if chunk.is_empty() {
            return Vec::new();
        }

        self.residual.extend_from_slice(chunk);
        let mut tokens = Vec::new();

        loop {
            let scan = self.scanner.scan(&self.residual, self.syntax_overflow);
            if scan.split == 0 {
                break;
            }

            let split = scan.split;
            let mut buffered = std::mem::take(&mut self.residual);
            if let Some(tag) = self.raw_close_overflow.take() {
                debug_assert_eq!(scan.text_close.map(|(_, close_tag)| close_tag), Some(tag));
                tokens.push(owned_close_tag(tag));
            } else if self.syntax_overflow {
                tokens.push(owned_text_token(&buffered[..split], None));
                self.syntax_overflow = false;
            } else if let Some((close_start, tag)) = scan.text_close {
                if close_start > 0 {
                    tokens.push(owned_text_token(&buffered[..close_start], Some(tag)));
                }
                tokens.extend(self.process_chunk(&buffered[close_start..split]));
            } else {
                tokens.extend(self.process_chunk(&buffered[..split]));
            }

            discard_prefix(&mut buffered, split);
            self.scanner.discard_prefix(split);
            self.residual = buffered;
        }

        self.flush_bounded_text(&mut tokens);
        self.flush_overlong_syntax(&mut tokens);

        tokens
    }

    /// Feed a UTF-8 chunk and process complete tokens via callback without cloning.
    ///
    /// This path is intended for internal high-throughput consumers (e.g. tree
    /// building) that can consume tokens immediately.
    pub fn feed_str_with(&mut self, chunk: &str, mut on_token: impl FnMut(&Token<'_>)) {
        if chunk.is_empty() {
            return;
        }

        self.residual.extend_from_slice(chunk.as_bytes());

        loop {
            let scan = self.scanner.scan(&self.residual, self.syntax_overflow);
            if scan.split == 0 {
                break;
            }

            let split = scan.split;
            let mut buffered = std::mem::take(&mut self.residual);
            if let Some(tag) = self.raw_close_overflow.take() {
                debug_assert_eq!(scan.text_close.map(|(_, close_tag)| close_tag), Some(tag));
                emit_close_tag_with(tag, &mut on_token);
            } else if self.syntax_overflow {
                emit_text_token_with(&buffered[..split], None, &mut on_token);
                self.syntax_overflow = false;
            } else if let Some((close_start, tag)) = scan.text_close {
                if close_start > 0 {
                    emit_text_token_with(&buffered[..close_start], Some(tag), &mut on_token);
                }
                self.process_chunk_with(&buffered[close_start..split], &mut on_token);
            } else {
                self.process_chunk_with(&buffered[..split], &mut on_token);
            }

            discard_prefix(&mut buffered, split);
            self.scanner.discard_prefix(split);
            self.residual = buffered;
        }

        self.flush_bounded_text_with(&mut on_token);
        self.flush_overlong_syntax_with(&mut on_token);
    }

    /// Signal end of input and flush any remaining buffered data.
    pub fn finish(&mut self) -> Vec<Token<'static>> {
        if self.residual.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.residual);
        let tokens = if self.syntax_overflow {
            vec![owned_text_token(&remaining, None)]
        } else if let Some(tag) = self.scanner.text_element() {
            vec![owned_text_token(&remaining, Some(tag))]
        } else {
            self.process_chunk(&remaining)
        };
        self.scanner.reset();
        self.syntax_overflow = false;
        self.raw_close_overflow = None;
        tokens
    }

    /// Signal end of input and flush buffered tokens via callback without cloning.
    pub fn finish_with(&mut self, mut on_token: impl FnMut(&Token<'_>)) {
        if self.residual.is_empty() {
            return;
        }
        let remaining = std::mem::take(&mut self.residual);
        if self.syntax_overflow {
            emit_text_token_with(&remaining, None, &mut on_token);
        } else if let Some(tag) = self.scanner.text_element() {
            emit_text_token_with(&remaining, Some(tag), &mut on_token);
        } else {
            self.process_chunk_with(&remaining, &mut on_token);
        }
        self.scanner.reset();
        self.syntax_overflow = false;
        self.raw_close_overflow = None;
    }

    /// Process a complete chunk through the structural indexer + extractor.
    fn process_chunk(&mut self, data: &[u8]) -> Vec<Token<'static>> {
        match std::str::from_utf8(data) {
            Ok(text) => {
                let index = self.indexer.index(text.as_bytes());
                let tokens = extract_tokens(text, &index);
                tokens.into_iter().map(to_owned_token).collect()
            }
            Err(_) => {
                let text = String::from_utf8_lossy(data).into_owned();
                let index = self.indexer.index(text.as_bytes());
                let tokens = extract_tokens(&text, &index);
                tokens.into_iter().map(to_owned_token).collect()
            }
        }
    }

    /// Process a complete chunk and expose borrowed tokens for the duration of
    /// the callback. Valid UTF-8 takes the zero-clone path; mixed use with the
    /// byte-oriented API still degrades safely through lossy decoding.
    fn process_chunk_with(&mut self, data: &[u8], on_token: &mut impl FnMut(&Token<'_>)) {
        match std::str::from_utf8(data) {
            Ok(text) => {
                let index = self.indexer.index(text.as_bytes());
                let tokens = extract_tokens(text, &index);
                for token in &tokens {
                    on_token(token);
                }
            }
            Err(_) => {
                let text = String::from_utf8_lossy(data).into_owned();
                let index = self.indexer.index(text.as_bytes());
                let tokens = extract_tokens(&text, &index);
                for token in &tokens {
                    on_token(token);
                }
            }
        }
    }

    /// Emit bounded text while retaining enough suffix bytes to recognize a
    /// split character reference or closing raw-text tag.
    fn flush_bounded_text(&mut self, tokens: &mut Vec<Token<'static>>) {
        let tag = self.scanner.text_element();
        let limit = if tag.is_some() {
            MAX_RAW_TEXT_RESIDUAL
        } else if self.scanner.is_plain_data() {
            MAX_RESIDUAL
        } else {
            return;
        };

        if self.residual.len() <= limit || self.residual.len() <= TEXT_TAIL {
            return;
        }

        if let Some((open, close_tag)) = self.scanner.text_close_candidate() {
            if open > 0 {
                tokens.push(owned_text_token(&self.residual[..open], Some(close_tag)));
                discard_prefix(&mut self.residual, open);
                self.scanner.discard_prefix(open);
            }
            if self.residual.len() > TEXT_TAIL {
                let discard_len =
                    utf8_prefix_boundary(&self.residual, self.residual.len() - TEXT_TAIL);
                if discard_len > 0 {
                    discard_prefix(&mut self.residual, discard_len);
                    self.scanner.discard_prefix(discard_len);
                    self.raw_close_overflow = Some(close_tag);
                }
            }
            return;
        }

        let desired = self.residual.len() - TEXT_TAIL;
        let prefix_len = utf8_prefix_boundary(&self.residual, desired);
        if prefix_len == 0 {
            return;
        }

        tokens.push(owned_text_token(&self.residual[..prefix_len], tag));
        discard_prefix(&mut self.residual, prefix_len);
        self.scanner.discard_prefix(prefix_len);
    }

    /// Callback counterpart of [`Self::flush_bounded_text`] that keeps valid
    /// UTF-8 content borrowed instead of allocating owned token strings.
    fn flush_bounded_text_with(&mut self, on_token: &mut impl FnMut(&Token<'_>)) {
        let tag = self.scanner.text_element();
        let limit = if tag.is_some() {
            MAX_RAW_TEXT_RESIDUAL
        } else if self.scanner.is_plain_data() {
            MAX_RESIDUAL
        } else {
            return;
        };

        if self.residual.len() <= limit || self.residual.len() <= TEXT_TAIL {
            return;
        }

        if let Some((open, close_tag)) = self.scanner.text_close_candidate() {
            if open > 0 {
                emit_text_token_with(&self.residual[..open], Some(close_tag), on_token);
                discard_prefix(&mut self.residual, open);
                self.scanner.discard_prefix(open);
            }
            if self.residual.len() > TEXT_TAIL {
                let discard_len =
                    utf8_prefix_boundary(&self.residual, self.residual.len() - TEXT_TAIL);
                if discard_len > 0 {
                    discard_prefix(&mut self.residual, discard_len);
                    self.scanner.discard_prefix(discard_len);
                    self.raw_close_overflow = Some(close_tag);
                }
            }
            return;
        }

        let desired = self.residual.len() - TEXT_TAIL;
        let prefix_len = utf8_prefix_boundary(&self.residual, desired);
        if prefix_len == 0 {
            return;
        }

        emit_text_token_with(&self.residual[..prefix_len], tag, on_token);
        discard_prefix(&mut self.residual, prefix_len);
        self.scanner.discard_prefix(prefix_len);
    }

    /// Bound an oversized comment/CDATA/doctype/quoted-tag token without
    /// resetting scanner state. The emitted text is inert in the DOM, and the
    /// scanner stops at the original token's terminator before normal parsing
    /// resumes.
    fn flush_overlong_syntax(&mut self, tokens: &mut Vec<Token<'static>>) {
        if !self.syntax_overflow {
            if self.residual.len() <= MAX_RAW_TEXT_RESIDUAL
                || self.scanner.in_text_context()
                || self.scanner.is_plain_data()
            {
                return;
            }
            self.syntax_overflow = true;
        }

        if self.residual.len() <= TEXT_TAIL {
            return;
        }
        let prefix_len = utf8_prefix_boundary(&self.residual, self.residual.len() - TEXT_TAIL);
        if prefix_len == 0 {
            return;
        }
        tokens.push(owned_text_token(&self.residual[..prefix_len], None));
        discard_prefix(&mut self.residual, prefix_len);
        self.scanner.discard_prefix(prefix_len);
    }

    fn flush_overlong_syntax_with(&mut self, on_token: &mut impl FnMut(&Token<'_>)) {
        if !self.syntax_overflow {
            if self.residual.len() <= MAX_RAW_TEXT_RESIDUAL
                || self.scanner.in_text_context()
                || self.scanner.is_plain_data()
            {
                return;
            }
            self.syntax_overflow = true;
        }

        if self.residual.len() <= TEXT_TAIL {
            return;
        }
        let prefix_len = utf8_prefix_boundary(&self.residual, self.residual.len() - TEXT_TAIL);
        if prefix_len == 0 {
            return;
        }
        emit_text_token_with(&self.residual[..prefix_len], None, on_token);
        discard_prefix(&mut self.residual, prefix_len);
        self.scanner.discard_prefix(prefix_len);
    }
}

impl Default for StreamTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
enum ScanMode {
    Data,
    Tag {
        quote: Option<u8>,
        open: usize,
        text_close: Option<Tag>,
    },
    Doctype {
        quote: Option<u8>,
    },
    Comment,
    CData,
}

#[derive(Debug)]
struct SplitScanner {
    mode: ScanMode,
    text_element: Option<Tag>,
    scanned: usize,
    last_safe: usize,
    text_close: Option<(usize, Tag)>,
}

impl Default for SplitScanner {
    fn default() -> Self {
        Self {
            mode: ScanMode::Data,
            text_element: None,
            scanned: 0,
            last_safe: 0,
            text_close: None,
        }
    }
}

impl SplitScanner {
    fn scan(&mut self, data: &[u8], stop_at_next_boundary: bool) -> SplitScan {
        let mut i = self.scanned.min(data.len());
        self.text_close = None;

        while i < data.len() {
            match self.mode {
                ScanMode::Data => {
                    if let Some(tag) = self.text_element {
                        if data[i] == b'<' {
                            if is_raw_text_close(data, i, tag) {
                                self.mode = ScanMode::Tag {
                                    quote: None,
                                    open: i,
                                    text_close: Some(tag),
                                };
                            } else if is_incomplete_raw_text_close(data, i, tag) {
                                break;
                            }
                        }
                        i += 1;
                        continue;
                    }

                    if data[i] == b'<' {
                        if is_incomplete_markup_prefix(&data[i..]) {
                            break;
                        }
                        if i + 3 < data.len() && &data[i..i + 4] == b"<!--" {
                            self.mode = ScanMode::Comment;
                            i += 4;
                            continue;
                        }
                        if i + 8 < data.len() && &data[i..i + 9] == b"<![CDATA[" {
                            self.mode = ScanMode::CData;
                            i += 9;
                            continue;
                        }
                        if i + 1 < data.len() {
                            let next = data[i + 1];
                            if next == b'!' {
                                self.mode = ScanMode::Doctype { quote: None };
                                i += 2;
                                continue;
                            }
                            if next == b'/'
                                || next.is_ascii_alphabetic()
                                || next == b'_'
                                || next == b'?'
                            {
                                self.mode = ScanMode::Tag {
                                    quote: None,
                                    open: i,
                                    text_close: None,
                                };
                                i += 1;
                                continue;
                            }
                        }
                    }
                    i += 1;
                }
                ScanMode::Tag {
                    mut quote,
                    open,
                    text_close,
                } => {
                    if let Some(q) = quote {
                        if data[i] == q {
                            quote = None;
                        }
                        self.mode = ScanMode::Tag {
                            quote,
                            open,
                            text_close,
                        };
                        i += 1;
                        continue;
                    }
                    match data[i] {
                        b'"' | b'\'' => {
                            self.mode = ScanMode::Tag {
                                quote: Some(data[i]),
                                open,
                                text_close,
                            };
                            i += 1;
                        }
                        b'>' => {
                            self.mode = ScanMode::Data;
                            i += 1;
                            self.last_safe = i;
                            if let Some(tag) = text_close {
                                self.text_element = None;
                                self.text_close = Some((open, tag));
                                break;
                            }
                            if let Some(tag) = raw_text_open_tag(&data[open + 1..i - 1]) {
                                self.text_element = Some(tag);
                                // Process the opening tag before buffering its
                                // potentially very large text body.
                                break;
                            }
                            if stop_at_next_boundary {
                                break;
                            }
                        }
                        _ => i += 1,
                    }
                }
                ScanMode::Doctype { mut quote } => {
                    if let Some(q) = quote {
                        if data[i] == q {
                            quote = None;
                        }
                        self.mode = ScanMode::Doctype { quote };
                        i += 1;
                        continue;
                    }
                    match data[i] {
                        b'"' | b'\'' => {
                            self.mode = ScanMode::Doctype {
                                quote: Some(data[i]),
                            };
                            i += 1;
                        }
                        b'>' => {
                            self.last_safe = i + 1;
                            self.mode = ScanMode::Data;
                            i += 1;
                            if stop_at_next_boundary {
                                break;
                            }
                        }
                        _ => i += 1,
                    }
                }
                ScanMode::Comment => {
                    if i + 2 < data.len()
                        && data[i] == b'-'
                        && data[i + 1] == b'-'
                        && data[i + 2] == b'>'
                    {
                        self.last_safe = i + 3;
                        self.mode = ScanMode::Data;
                        i += 3;
                        if stop_at_next_boundary {
                            break;
                        }
                    } else if data[i] == b'-' && data.len() - i < 3 {
                        // Keep a possible split `-->` terminator for the next
                        // feed instead of scanning past its leading dashes.
                        break;
                    } else {
                        i += 1;
                    }
                }
                ScanMode::CData => {
                    if i + 2 < data.len()
                        && data[i] == b']'
                        && data[i + 1] == b']'
                        && data[i + 2] == b'>'
                    {
                        self.last_safe = i + 3;
                        self.mode = ScanMode::Data;
                        i += 3;
                        if stop_at_next_boundary {
                            break;
                        }
                    } else if data[i] == b']' && data.len() - i < 3 {
                        // Keep a possible split `]]>` terminator.
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
        }

        self.scanned = i;
        SplitScan {
            split: self.last_safe,
            in_raw_text_context: self.in_text_context(),
            text_close: self.text_close,
        }
    }

    fn discard_prefix(&mut self, len: usize) {
        self.scanned = self.scanned.saturating_sub(len);
        self.last_safe = self.last_safe.saturating_sub(len);
        self.text_close = None;
        if let ScanMode::Tag {
            quote,
            open,
            text_close,
        } = self.mode
        {
            self.mode = ScanMode::Tag {
                quote,
                open: open.saturating_sub(len),
                text_close,
            };
        }
    }

    fn in_text_context(&self) -> bool {
        self.text_element.is_some()
            || matches!(
                self.mode,
                ScanMode::Tag {
                    text_close: Some(_),
                    ..
                }
            )
    }

    fn text_element(&self) -> Option<Tag> {
        self.text_element.or(match self.mode {
            ScanMode::Tag {
                text_close: Some(tag),
                ..
            } => Some(tag),
            _ => None,
        })
    }

    fn text_close_candidate(&self) -> Option<(usize, Tag)> {
        match self.mode {
            ScanMode::Tag {
                open,
                text_close: Some(tag),
                ..
            } => Some((open, tag)),
            _ => None,
        }
    }

    fn is_plain_data(&self) -> bool {
        self.text_element.is_none() && matches!(self.mode, ScanMode::Data)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
fn scan_safe_split(data: &[u8]) -> SplitScan {
    SplitScanner::default().scan(data, false)
}

fn discard_prefix(buffer: &mut Vec<u8>, len: usize) {
    let remaining = buffer.len() - len;
    buffer.copy_within(len.., 0);
    buffer.truncate(remaining);
}

fn utf8_prefix_boundary(data: &[u8], desired: usize) -> usize {
    let mut boundary = desired.min(data.len());
    while boundary > 0 && boundary < data.len() && data[boundary] & 0b1100_0000 == 0b1000_0000 {
        boundary -= 1;
    }
    boundary
}

fn owned_text_token(data: &[u8], context: Option<Tag>) -> Token<'static> {
    let text = String::from_utf8_lossy(data);
    let content = if context.is_some_and(Tag::is_raw_text) {
        text.into_owned()
    } else {
        decode_stream_entities(&text).into_owned()
    };
    Token::Text {
        content: std::borrow::Cow::Owned(content),
    }
}

fn owned_close_tag(tag: Tag) -> Token<'static> {
    Token::CloseTag {
        tag,
        name: std::borrow::Cow::Owned(tag.as_str().unwrap_or("").to_owned()),
    }
}

fn emit_close_tag_with(tag: Tag, on_token: &mut impl FnMut(&Token<'_>)) {
    on_token(&Token::CloseTag {
        tag,
        name: std::borrow::Cow::Borrowed(tag.as_str().unwrap_or("")),
    });
}

fn emit_text_token_with(data: &[u8], context: Option<Tag>, on_token: &mut impl FnMut(&Token<'_>)) {
    match std::str::from_utf8(data) {
        Ok(text) => {
            let content = if context.is_some_and(Tag::is_raw_text) {
                std::borrow::Cow::Borrowed(text)
            } else {
                decode_stream_entities(text)
            };
            on_token(&Token::Text { content });
        }
        Err(_) => {
            let text = String::from_utf8_lossy(data).into_owned();
            let content = if context.is_some_and(Tag::is_raw_text) {
                std::borrow::Cow::Borrowed(text.as_str())
            } else {
                decode_stream_entities(&text)
            };
            on_token(&Token::Text { content });
        }
    }
}

#[cfg(feature = "entity-decode")]
fn decode_stream_entities(input: &str) -> std::borrow::Cow<'_, str> {
    crate::entity::decode_entities(input)
}

#[cfg(not(feature = "entity-decode"))]
fn decode_stream_entities(input: &str) -> std::borrow::Cow<'_, str> {
    std::borrow::Cow::Borrowed(input)
}

fn is_incomplete_markup_prefix(remaining: &[u8]) -> bool {
    if remaining == b"<" || remaining == b"</" {
        return true;
    }
    [b"<!--".as_slice(), b"<![CDATA[".as_slice()]
        .iter()
        .any(|target| remaining.len() < target.len() && target.starts_with(remaining))
}

fn is_incomplete_raw_text_close(data: &[u8], pos: usize, tag: Tag) -> bool {
    let remaining = &data[pos..];
    let name = tag.as_str().unwrap_or("").as_bytes();
    if remaining.first() != Some(&b'<') {
        return false;
    }

    let target_len = 2 + name.len();
    for (idx, &actual) in remaining.iter().take(target_len).enumerate() {
        let expected = match idx {
            0 => b'<',
            1 => b'/',
            _ => name[idx - 2],
        };
        if !actual.eq_ignore_ascii_case(&expected) {
            return false;
        }
    }

    remaining.len() <= target_len
}

fn raw_text_open_tag(tag_body: &[u8]) -> Option<Tag> {
    if tag_body.is_empty() || tag_body[0] == b'/' {
        return None;
    }

    // Match the extractor's self-closing rule exactly: only a slash directly
    // before `>` closes the start tag. A spaced form such as `<script / >`
    // remains a raw-text element in the one-shot parser and must do so here.
    if tag_body.last() == Some(&b'/') {
        return None;
    }

    let mut name_end = 0usize;
    while name_end < tag_body.len()
        && !is_html_whitespace(tag_body[name_end])
        && tag_body[name_end] != b'/'
    {
        name_end += 1;
    }
    if name_end == 0 {
        return None;
    }

    let tag = Tag::from_bytes(&tag_body[..name_end]);
    (tag.is_raw_text() || tag.is_rcdata()).then_some(tag)
}

fn is_raw_text_close(data: &[u8], pos: usize, tag: Tag) -> bool {
    let remaining = &data[pos..];
    if remaining.len() < 3 || remaining[1] != b'/' {
        return false;
    }

    let tag_name = tag.as_str().unwrap_or("");
    let name_len = tag_name.len();
    if remaining.len() < 2 + name_len + 1 {
        return false;
    }

    let candidate = &remaining[2..2 + name_len];
    if !candidate.eq_ignore_ascii_case(tag_name.as_bytes()) {
        return false;
    }

    let after = remaining[2 + name_len];
    after == b'>' || after == b'/' || is_html_whitespace(after)
}

#[inline(always)]
fn is_html_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0C' | b'\r')
}

/// Convert a borrowed token to an owned ('static) token.
fn to_owned_token(token: Token<'_>) -> Token<'static> {
    match token {
        Token::OpenTag {
            tag,
            name,
            attributes,
            self_closing,
        } => Token::OpenTag {
            tag,
            name: std::borrow::Cow::Owned(name.into_owned()),
            attributes: attributes.into_iter().map(to_owned_attr).collect(),
            self_closing,
        },
        Token::CloseTag { tag, name } => Token::CloseTag {
            tag,
            name: std::borrow::Cow::Owned(name.into_owned()),
        },
        Token::Text { content } => Token::Text {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::Comment { content } => Token::Comment {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::Doctype { content } => Token::Doctype {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::CData { content } => Token::CData {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
    }
}

/// Convert a borrowed attribute to owned.
fn to_owned_attr(attr: crate::token::Attribute<'_>) -> crate::token::Attribute<'static> {
    crate::token::Attribute {
        name: std::borrow::Cow::Owned(attr.name.into_owned()),
        value: attr.value.map(|v| std::borrow::Cow::Owned(v.into_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_chunk() {
        let mut tok = StreamTokenizer::new();
        let tokens = tok.feed(b"<div>hello</div>");
        let final_tokens = tok.finish();

        let all: Vec<_> = tokens.into_iter().chain(final_tokens).collect();
        assert!(all.iter().any(|t| matches!(t, Token::OpenTag { .. })));
        assert!(all.iter().any(|t| matches!(t, Token::CloseTag { .. })));
    }

    #[test]
    fn multi_chunk() {
        let html = b"<div>hello</div>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        // Feed byte by byte.
        for &b in html.iter() {
            all.extend(tok.feed(&[b]));
        }
        all.extend(tok.finish());

        let has_open = all.iter().any(|t| matches!(t, Token::OpenTag { .. }));
        let has_close = all.iter().any(|t| matches!(t, Token::CloseTag { .. }));
        let has_text = all.iter().any(|t| matches!(t, Token::Text { .. }));

        assert!(has_open, "should have open tag");
        assert!(has_close, "should have close tag");
        assert!(has_text, "should have text");
    }

    #[test]
    fn chunk_size_7() {
        let html = b"<div class=\"test\">hello world</div>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for chunk in html.chunks(7) {
            all.extend(tok.feed(chunk));
        }
        all.extend(tok.finish());

        assert!(all.iter().any(|t| matches!(t, Token::OpenTag { .. })));
        assert!(all.iter().any(|t| matches!(t, Token::CloseTag { .. })));
    }

    #[test]
    fn chunk_size_64() {
        let html = b"<html><head><title>Test</title></head><body><div class=\"main\"><p>Hello</p></div></body></html>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for chunk in html.chunks(64) {
            all.extend(tok.feed(chunk));
        }
        all.extend(tok.finish());

        let open_count = all
            .iter()
            .filter(|t| matches!(t, Token::OpenTag { .. }))
            .count();
        assert!(open_count >= 5, "should have multiple open tags");
    }

    #[test]
    fn empty_chunks() {
        let mut tok = StreamTokenizer::new();
        let t1 = tok.feed(b"");
        let t2 = tok.feed(b"<br/>");
        let t3 = tok.feed(b"");
        let t4 = tok.finish();

        let all: Vec<_> = t1.into_iter().chain(t2).chain(t3).chain(t4).collect();
        assert!(all.iter().any(|t| matches!(t, Token::OpenTag { .. })));
    }

    #[test]
    fn find_safe_split_basic() {
        assert_eq!(scan_safe_split(b"<div>hello</div>").split, 16);
        assert_eq!(scan_safe_split(b"<div>hello").split, 5);
        assert_eq!(scan_safe_split(b"hello").split, 0);
    }

    #[test]
    fn find_safe_split_buffers_open_raw_text_context() {
        let scan = scan_safe_split(b"<div><script>if(a<b)");

        assert_eq!(scan.split, 13);
        assert!(scan.in_raw_text_context);
    }

    #[test]
    fn raw_text_split_after_script_open() {
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        all.extend(tok.feed(b"<script>"));
        all.extend(tok.feed(b"if(a<b)"));
        all.extend(tok.feed(b"{x()}</script>"));
        all.extend(tok.finish());

        let open_tags: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::OpenTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        let close_tags: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::CloseTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        let text: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::Text { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(open_tags, vec![Tag::Script]);
        assert_eq!(close_tags, vec![Tag::Script]);
        assert_eq!(text, vec!["if(a<b){x()}"]);
    }

    #[test]
    fn split_raw_text_close_accepts_a_trailing_solidus() {
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        all.extend(tok.feed(b"<script>x</scr"));
        all.extend(tok.feed(b"ipt/><img src=x>"));
        all.extend(tok.finish());

        let open_tags: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::OpenTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        let close_tags: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::CloseTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();

        assert_eq!(open_tags, [Tag::Script, Tag::Img]);
        assert_eq!(close_tags, [Tag::Script]);
    }

    #[test]
    fn spaced_slash_raw_text_open_matches_one_shot_behavior() {
        let html = b"<script / >a<b></script><p>ok</p>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for byte in html {
            all.extend(tok.feed(std::slice::from_ref(byte)));
        }
        all.extend(tok.finish());

        let open_tags: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::OpenTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        let text: String = all
            .iter()
            .filter_map(|token| match token {
                Token::Text { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(open_tags, [Tag::Script, Tag::P]);
        assert_eq!(text, "a<b>ok");
    }

    #[test]
    fn comment_larger_than_soft_residual_limit_stays_a_comment() {
        let body = "x".repeat(MAX_RESIDUAL + 256);
        let html = format!("<!--{body}--><p>ok</p>");
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for chunk in html.as_bytes().chunks(37) {
            all.extend(tok.feed(chunk));
        }
        all.extend(tok.finish());

        let comments: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::Comment { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(comments, vec![body.as_str()]);
    }

    #[test]
    fn oversized_raw_text_is_streamed_without_parsing_markup() {
        let mut body = "a".repeat(MAX_RAW_TEXT_RESIDUAL + 1024);
        body.push_str("<b>still raw</b>");
        let html = format!("<script>{body}</script><p>ok</p>");
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for chunk in html.as_bytes().chunks(64 * 1024) {
            all.extend(tok.feed(chunk));
        }
        all.extend(tok.finish());

        let open_tags: Vec<_> = all
            .iter()
            .filter_map(|token| match token {
                Token::OpenTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        let text: String = all
            .iter()
            .filter_map(|token| match token {
                Token::Text { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(open_tags, vec![Tag::Script, Tag::P]);
        assert_eq!(text, format!("{body}ok"));
        assert!(tok.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
    }

    #[test]
    fn oversized_raw_close_suffix_retains_close_token_identity() {
        let html = format!(
            "<script>x</script{}><p>ok</p>",
            " ".repeat(MAX_RAW_TEXT_RESIDUAL + 1024)
        );

        let mut owned = StreamTokenizer::new();
        let mut owned_tokens = Vec::new();
        for chunk in html.as_bytes().chunks(64 * 1024) {
            owned_tokens.extend(owned.feed(chunk));
        }
        owned_tokens.extend(owned.finish());

        let mut callback = StreamTokenizer::new();
        let mut callback_open = Vec::new();
        let mut callback_close = Vec::new();
        for chunk in html.as_bytes().chunks(64 * 1024) {
            let text = std::str::from_utf8(chunk).unwrap();
            callback.feed_str_with(text, |token| match token {
                Token::OpenTag { tag, .. } => callback_open.push(*tag),
                Token::CloseTag { tag, .. } => callback_close.push(*tag),
                _ => {}
            });
        }
        callback.finish_with(|token| match token {
            Token::OpenTag { tag, .. } => callback_open.push(*tag),
            Token::CloseTag { tag, .. } => callback_close.push(*tag),
            _ => {}
        });

        let owned_open: Vec<_> = owned_tokens
            .iter()
            .filter_map(|token| match token {
                Token::OpenTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        let owned_close: Vec<_> = owned_tokens
            .iter()
            .filter_map(|token| match token {
                Token::CloseTag { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();

        assert_eq!(owned_open, [Tag::Script, Tag::P]);
        assert_eq!(owned_close, [Tag::Script, Tag::P]);
        assert_eq!(callback_open, owned_open);
        assert_eq!(callback_close, owned_close);
        assert!(owned.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
        assert!(callback.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
    }

    #[test]
    fn bounded_plain_text_callback_matches_owned_and_respects_entity_feature() {
        let input = format!("{}&amp;{}", "a".repeat(100), "b".repeat(MAX_RESIDUAL));

        let mut owned_tokenizer = StreamTokenizer::new();
        let mut owned_tokens = owned_tokenizer.feed(input.as_bytes());
        owned_tokens.extend(owned_tokenizer.finish());
        let owned_text: String = owned_tokens
            .iter()
            .filter_map(|token| match token {
                Token::Text { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();

        let mut callback_tokenizer = StreamTokenizer::new();
        let mut callback_text = String::new();
        callback_tokenizer.feed_str_with(&input, |token| {
            if let Token::Text { content } = token {
                callback_text.push_str(content);
            }
        });
        callback_tokenizer.finish_with(|token| {
            if let Token::Text { content } = token {
                callback_text.push_str(content);
            }
        });

        let expected = if cfg!(feature = "entity-decode") {
            input.replacen("&amp;", "&", 1)
        } else {
            input
        };
        assert_eq!(owned_text, expected);
        assert_eq!(callback_text, expected);
    }

    #[test]
    fn oversized_syntax_keeps_state_until_its_real_terminator() {
        let cases = [
            ("<!--", "<script>x</script>--><p>ok</p>"),
            ("<![CDATA[", "<script>x</script>]]><p>ok</p>"),
            ("<div title=\"", "<script>x</script>\"><p>ok</p>"),
        ];

        for (prefix, suffix) in cases {
            let first = format!("{prefix}{}", "a".repeat(MAX_RAW_TEXT_RESIDUAL + 1));
            let mut tok = StreamTokenizer::new();
            let mut all = tok.feed(first.as_bytes());
            assert!(tok.buffered_len() <= TEXT_TAIL);
            all.extend(tok.feed(suffix.as_bytes()));
            all.extend(tok.finish());

            let open_tags: Vec<_> = all
                .iter()
                .filter_map(|token| match token {
                    Token::OpenTag { tag, .. } => Some(*tag),
                    _ => None,
                })
                .collect();
            assert_eq!(open_tags, [Tag::P], "prefix={prefix}, tokens={all:?}");
        }
    }

    #[test]
    fn callback_oversized_comment_cannot_release_nested_markup() {
        let first = format!("<!--{}", "a".repeat(MAX_RAW_TEXT_RESIDUAL + 1));
        let mut tok = StreamTokenizer::new();
        let mut open_tags = Vec::new();

        tok.feed_str_with(&first, |token| {
            if let Token::OpenTag { tag, .. } = token {
                open_tags.push(*tag);
            }
        });
        tok.feed_str_with("<script>x</script>--><p>ok</p>", |token| {
            if let Token::OpenTag { tag, .. } = token {
                open_tags.push(*tag);
            }
        });
        tok.finish_with(|token| {
            if let Token::OpenTag { tag, .. } = token {
                open_tags.push(*tag);
            }
        });

        assert_eq!(open_tags, [Tag::P]);
    }
}
