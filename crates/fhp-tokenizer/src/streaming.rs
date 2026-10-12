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
/// Oversized matching raw-text/RCDATA close-tag candidates instead discard
/// their prefix, retaining only the tag identity and a bounded text tail.
/// If EOF arrives before the terminator, discarded bytes are not recovered.
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
    /// Identity of a matching raw-text/RCDATA close tag whose oversized source
    /// has been discarded while its scanner state is retained.
    raw_close_overflow: Option<RawCloseOverflow>,
    /// An overlong numeric character reference compacted out of `residual`.
    /// This prevents an unterminated digit run from defeating the buffer bound.
    numeric_entity: Option<NumericEntityState>,
}

#[derive(Clone, Copy, Debug)]
struct NumericEntityState {
    is_hex: bool,
    value: u32,
    overflow: bool,
}

struct RawCloseOverflow {
    tag: Tag,
    /// Recognized text-element names are at most eight ASCII bytes.
    name: [u8; 8],
}

impl RawCloseOverflow {
    fn source_name(&self) -> &str {
        let len = self.tag.as_str().expect("text-element tag is known").len();
        std::str::from_utf8(&self.name[..len]).expect("text-element name is ASCII")
    }
}

impl NumericEntityState {
    fn new(is_hex: bool) -> Self {
        Self {
            is_hex,
            value: 0,
            overflow: false,
        }
    }

    fn accepts(self, byte: u8) -> bool {
        if self.is_hex {
            byte.is_ascii_hexdigit()
        } else {
            byte.is_ascii_digit()
        }
    }

    fn push(&mut self, byte: u8) {
        if self.overflow {
            return;
        }
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' if self.is_hex => u32::from(byte - b'a') + 10,
            b'A'..=b'F' if self.is_hex => u32::from(byte - b'A') + 10,
            _ => return,
        };
        let radix = if self.is_hex { 16 } else { 10 };
        match self
            .value
            .checked_mul(radix)
            .and_then(|value| value.checked_add(digit))
        {
            Some(value) => self.value = value,
            None => self.overflow = true,
        }
    }

    fn decoded(self) -> char {
        if self.overflow {
            return '\u{FFFD}';
        }
        let digits = if self.is_hex {
            format!("{:X}", self.value)
        } else {
            self.value.to_string()
        };
        fhp_core::entity::decode_numeric(&digits, self.is_hex).unwrap_or('\u{FFFD}')
    }
}

enum TextFlushPlan {
    Prefix(usize),
    NumericOverflow {
        amp: usize,
        state: NumericEntityState,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SplitScan {
    split: usize,
    in_raw_text_context: bool,
    /// Start of a matching raw-text/RCDATA close tag within `split`, together
    /// with the element whose preceding bytes are text.
    text_close: Option<(usize, Tag)>,
}

/// Text-tokenization context retained across chunk boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextContext {
    Element(Tag),
    Plaintext,
}

impl TextContext {
    #[inline]
    fn decodes_entities(self) -> bool {
        matches!(self, Self::Element(tag) if tag.is_rcdata())
    }
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
            numeric_entity: None,
        }
    }

    /// Number of input bytes currently buffered and not yet resolved.
    ///
    /// Bounded by [`MAX_RAW_TEXT_RESIDUAL`] plus the size of one fed chunk.
    /// Oversized matching close-tag prefixes are discarded; only their fixed-size
    /// tag identity and scanner state are retained separately.
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

        let mut tokens = Vec::new();
        let chunk = self.resume_numeric_entity(chunk, &mut tokens);
        if chunk.is_empty() {
            return tokens;
        }
        self.residual.extend_from_slice(chunk);

        loop {
            let scan = self.scanner.scan(&self.residual, self.syntax_overflow);
            if scan.split == 0 {
                break;
            }

            let split = scan.split;
            let mut buffered = std::mem::take(&mut self.residual);
            if let Some(overflow) = self.raw_close_overflow.take() {
                debug_assert_eq!(
                    scan.text_close.map(|(_, close_tag)| close_tag),
                    Some(overflow.tag)
                );
                tokens.push(owned_close_tag(overflow.tag, overflow.source_name()));
            } else if self.syntax_overflow {
                tokens.push(owned_text_token(&buffered[..split], None));
                self.syntax_overflow = false;
            } else if let Some((close_start, tag)) = scan.text_close {
                if close_start > 0 {
                    tokens.push(owned_text_token(
                        &buffered[..close_start],
                        Some(TextContext::Element(tag)),
                    ));
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

        let chunk = self.resume_numeric_entity_with(chunk.as_bytes(), &mut on_token);
        if chunk.is_empty() {
            return;
        }
        self.residual.extend_from_slice(chunk);

        loop {
            let scan = self.scanner.scan(&self.residual, self.syntax_overflow);
            if scan.split == 0 {
                break;
            }

            let split = scan.split;
            let mut buffered = std::mem::take(&mut self.residual);
            if let Some(overflow) = self.raw_close_overflow.take() {
                debug_assert_eq!(
                    scan.text_close.map(|(_, close_tag)| close_tag),
                    Some(overflow.tag)
                );
                emit_close_tag_with(overflow.tag, overflow.source_name(), &mut on_token);
            } else if self.syntax_overflow {
                emit_text_token_with(&buffered[..split], None, &mut on_token);
                self.syntax_overflow = false;
            } else if let Some((close_start, tag)) = scan.text_close {
                if close_start > 0 {
                    emit_text_token_with(
                        &buffered[..close_start],
                        Some(TextContext::Element(tag)),
                        &mut on_token,
                    );
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
        let mut prefix = Vec::new();
        if let Some(state) = self.numeric_entity.take() {
            prefix.push(owned_numeric_entity_token(state));
        }
        if self.residual.is_empty() && self.raw_close_overflow.is_none() {
            if !prefix.is_empty() {
                self.scanner.reset();
                self.syntax_overflow = false;
            }
            return prefix;
        }
        let remaining = std::mem::take(&mut self.residual);
        let mut tokens = if let Some(overflow) = self.raw_close_overflow.take() {
            vec![owned_text_token(
                &remaining,
                Some(TextContext::Element(overflow.tag)),
            )]
        } else if self.syntax_overflow {
            vec![owned_text_token(&remaining, None)]
        } else if let Some(context) = self.scanner.text_context() {
            vec![owned_text_token(&remaining, Some(context))]
        } else {
            self.process_chunk(&remaining)
        };
        self.scanner.reset();
        self.syntax_overflow = false;
        prefix.append(&mut tokens);
        prefix
    }

    /// Signal end of input and flush buffered tokens via callback without cloning.
    pub fn finish_with(&mut self, mut on_token: impl FnMut(&Token<'_>)) {
        let mut finished_numeric_entity = false;
        if let Some(state) = self.numeric_entity.take() {
            emit_numeric_entity_with(state, &mut on_token);
            finished_numeric_entity = true;
        }
        if self.residual.is_empty() && self.raw_close_overflow.is_none() {
            if finished_numeric_entity {
                self.scanner.reset();
                self.syntax_overflow = false;
            }
            return;
        }
        let remaining = std::mem::take(&mut self.residual);
        if let Some(overflow) = self.raw_close_overflow.take() {
            emit_text_token_with(
                &remaining,
                Some(TextContext::Element(overflow.tag)),
                &mut on_token,
            );
        } else if self.syntax_overflow {
            emit_text_token_with(&remaining, None, &mut on_token);
        } else if let Some(context) = self.scanner.text_context() {
            emit_text_token_with(&remaining, Some(context), &mut on_token);
        } else {
            self.process_chunk_with(&remaining, &mut on_token);
        }
        self.scanner.reset();
        self.syntax_overflow = false;
    }

    fn resume_numeric_entity<'a>(
        &mut self,
        chunk: &'a [u8],
        tokens: &mut Vec<Token<'static>>,
    ) -> &'a [u8] {
        let Some(mut state) = self.numeric_entity.take() else {
            return chunk;
        };
        let mut consumed = 0;
        while consumed < chunk.len() && state.accepts(chunk[consumed]) {
            state.push(chunk[consumed]);
            consumed += 1;
        }
        if consumed == chunk.len() {
            self.numeric_entity = Some(state);
            return &chunk[chunk.len()..];
        }

        tokens.push(owned_numeric_entity_token(state));
        if chunk[consumed] == b';' {
            consumed += 1;
        }
        &chunk[consumed..]
    }

    fn resume_numeric_entity_with<'a>(
        &mut self,
        chunk: &'a [u8],
        on_token: &mut impl FnMut(&Token<'_>),
    ) -> &'a [u8] {
        let Some(mut state) = self.numeric_entity.take() else {
            return chunk;
        };
        let mut consumed = 0;
        while consumed < chunk.len() && state.accepts(chunk[consumed]) {
            state.push(chunk[consumed]);
            consumed += 1;
        }
        if consumed == chunk.len() {
            self.numeric_entity = Some(state);
            return &chunk[chunk.len()..];
        }

        emit_numeric_entity_with(state, on_token);
        if chunk[consumed] == b';' {
            consumed += 1;
        }
        &chunk[consumed..]
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
        let context = self.scanner.text_context();
        let limit = if context.is_some() {
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
                tokens.push(owned_text_token(
                    &self.residual[..open],
                    Some(TextContext::Element(close_tag)),
                ));
                discard_prefix(&mut self.residual, open);
                self.scanner.discard_prefix(open);
            }
            if self.residual.len() > TEXT_TAIL {
                let discard_len =
                    utf8_prefix_boundary(&self.residual, self.residual.len() - TEXT_TAIL);
                if discard_len > 0 {
                    self.discard_raw_close_prefix(close_tag, discard_len);
                }
            }
            return;
        }

        let desired = self.residual.len() - TEXT_TAIL;
        let prefix_len = match text_flush_plan(&self.residual, desired, context) {
            TextFlushPlan::Prefix(prefix_len) => prefix_len,
            TextFlushPlan::NumericOverflow { amp, state } => {
                if amp > 0 {
                    tokens.push(owned_text_token(&self.residual[..amp], context));
                }
                let consumed = self.residual.len();
                self.residual.clear();
                self.scanner.discard_prefix(consumed);
                self.numeric_entity = Some(state);
                return;
            }
        };
        if prefix_len == 0 {
            return;
        }

        tokens.push(owned_text_token(&self.residual[..prefix_len], context));
        discard_prefix(&mut self.residual, prefix_len);
        self.scanner.discard_prefix(prefix_len);
    }

    /// Callback counterpart of [`Self::flush_bounded_text`] that keeps valid
    /// UTF-8 content borrowed instead of allocating owned token strings.
    fn flush_bounded_text_with(&mut self, on_token: &mut impl FnMut(&Token<'_>)) {
        let context = self.scanner.text_context();
        let limit = if context.is_some() {
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
                emit_text_token_with(
                    &self.residual[..open],
                    Some(TextContext::Element(close_tag)),
                    on_token,
                );
                discard_prefix(&mut self.residual, open);
                self.scanner.discard_prefix(open);
            }
            if self.residual.len() > TEXT_TAIL {
                let discard_len =
                    utf8_prefix_boundary(&self.residual, self.residual.len() - TEXT_TAIL);
                if discard_len > 0 {
                    self.discard_raw_close_prefix(close_tag, discard_len);
                }
            }
            return;
        }

        let desired = self.residual.len() - TEXT_TAIL;
        let prefix_len = match text_flush_plan(&self.residual, desired, context) {
            TextFlushPlan::Prefix(prefix_len) => prefix_len,
            TextFlushPlan::NumericOverflow { amp, state } => {
                if amp > 0 {
                    emit_text_token_with(&self.residual[..amp], context, on_token);
                }
                let consumed = self.residual.len();
                self.residual.clear();
                self.scanner.discard_prefix(consumed);
                self.numeric_entity = Some(state);
                return;
            }
        };
        if prefix_len == 0 {
            return;
        }

        emit_text_token_with(&self.residual[..prefix_len], context, on_token);
        discard_prefix(&mut self.residual, prefix_len);
        self.scanner.discard_prefix(prefix_len);
    }

    fn discard_raw_close_prefix(&mut self, tag: Tag, len: usize) {
        let overflow = self.raw_close_overflow.get_or_insert_with(|| {
            let mut name = [0; 8];
            let name_len = tag.as_str().expect("text-element tag is known").len();
            name[..name_len].copy_from_slice(&self.residual[2..2 + name_len]);
            RawCloseOverflow { tag, name }
        });
        debug_assert_eq!(overflow.tag, tag);
        discard_prefix(&mut self.residual, len);
        self.scanner.discard_prefix(len);
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
    text_context: Option<TextContext>,
    scanned: usize,
    last_safe: usize,
    text_close: Option<(usize, Tag)>,
}

impl Default for SplitScanner {
    fn default() -> Self {
        Self {
            mode: ScanMode::Data,
            text_context: None,
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
                    if let Some(context) = self.text_context {
                        if let TextContext::Element(tag) = context {
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
                            // Revisit the opening dashes: the extractor accepts
                            // their overlap with `>` in `<!-->` and `<!--->`.
                            i += 2;
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
                            if crate::extract::starts_markup_after_lt(
                                Some(next),
                                data.get(i + 2).copied(),
                            ) {
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
                                self.text_context = None;
                                self.text_close = Some((open, tag));
                                break;
                            }
                            if let Some(context) = text_open_context(&data[open + 1..i - 1]) {
                                self.text_context = Some(context);
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
        self.text_context.is_some()
            || matches!(
                self.mode,
                ScanMode::Tag {
                    text_close: Some(_),
                    ..
                }
            )
    }

    fn text_context(&self) -> Option<TextContext> {
        self.text_context.or(match self.mode {
            ScanMode::Tag {
                text_close: Some(tag),
                ..
            } => Some(TextContext::Element(tag)),
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
        self.text_context.is_none() && matches!(self.mode, ScanMode::Data)
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

/// Choose a bounded text flush without bisecting a character reference.
/// Named references have a bounded name length; an unterminated numeric
/// reference can have arbitrarily many digits and is therefore compacted into
/// [`NumericEntityState`] once retaining it would exceed the tail allowance.
fn text_flush_plan(data: &[u8], desired: usize, context: Option<TextContext>) -> TextFlushPlan {
    let boundary = utf8_prefix_boundary(data, desired);
    if boundary == 0
        || !cfg!(feature = "entity-decode")
        || !context.is_none_or(TextContext::decodes_entities)
    {
        return TextFlushPlan::Prefix(boundary);
    }

    let Some(amp) = data[..boundary].iter().rposition(|byte| *byte == b'&') else {
        return TextFlushPlan::Prefix(boundary);
    };

    if let Some((state, digits_end)) = numeric_entity_prefix(data, amp) {
        let has_semicolon = data.get(digits_end) == Some(&b';');
        if digits_end > boundary || (digits_end == boundary && has_semicolon) {
            if digits_end < data.len() {
                let reference_end = digits_end + usize::from(has_semicolon);
                return TextFlushPlan::Prefix(reference_end);
            }
            if boundary - amp > TEXT_TAIL {
                return TextFlushPlan::NumericOverflow { amp, state };
            }
            return TextFlushPlan::Prefix(amp);
        }
    }

    // This covers every named reference (the longest WHATWG name is 31
    // bytes) and preserves the following-alphanumeric lookahead needed by
    // legacy names. Farther ampersands have already reached a stable result.
    if boundary - amp <= TEXT_TAIL {
        TextFlushPlan::Prefix(amp)
    } else {
        TextFlushPlan::Prefix(boundary)
    }
}

fn numeric_entity_prefix(data: &[u8], amp: usize) -> Option<(NumericEntityState, usize)> {
    if data.get(amp + 1) != Some(&b'#') {
        return None;
    }
    let mut cursor = amp + 2;
    let is_hex = matches!(data.get(cursor), Some(b'x' | b'X'));
    if is_hex {
        cursor += 1;
    }
    let digits_start = cursor;
    let mut state = NumericEntityState::new(is_hex);
    while let Some(&byte) = data.get(cursor) {
        if !state.accepts(byte) {
            break;
        }
        state.push(byte);
        cursor += 1;
    }
    (cursor > digits_start).then_some((state, cursor))
}

fn owned_text_token(data: &[u8], context: Option<TextContext>) -> Token<'static> {
    let text = String::from_utf8_lossy(data);
    let content = if context.is_none_or(TextContext::decodes_entities) {
        decode_stream_entities(&text).into_owned()
    } else {
        text.into_owned()
    };
    Token::Text {
        content: std::borrow::Cow::Owned(content),
    }
}

fn owned_close_tag(tag: Tag, name: &str) -> Token<'static> {
    Token::CloseTag {
        tag,
        name: std::borrow::Cow::Owned(name.to_owned()),
    }
}

fn owned_numeric_entity_token(state: NumericEntityState) -> Token<'static> {
    Token::Text {
        content: std::borrow::Cow::Owned(state.decoded().to_string()),
    }
}

fn emit_close_tag_with(tag: Tag, name: &str, on_token: &mut impl FnMut(&Token<'_>)) {
    on_token(&Token::CloseTag {
        tag,
        name: std::borrow::Cow::Borrowed(name),
    });
}

fn emit_numeric_entity_with(state: NumericEntityState, on_token: &mut impl FnMut(&Token<'_>)) {
    let content = state.decoded().to_string();
    on_token(&Token::Text {
        content: std::borrow::Cow::Borrowed(&content),
    });
}

fn emit_text_token_with(
    data: &[u8],
    context: Option<TextContext>,
    on_token: &mut impl FnMut(&Token<'_>),
) {
    match std::str::from_utf8(data) {
        Ok(text) => {
            let content = if context.is_none_or(TextContext::decodes_entities) {
                decode_stream_entities(text)
            } else {
                std::borrow::Cow::Borrowed(text)
            };
            on_token(&Token::Text { content });
        }
        Err(_) => {
            let text = String::from_utf8_lossy(data).into_owned();
            let content = if context.is_none_or(TextContext::decodes_entities) {
                decode_stream_entities(&text)
            } else {
                std::borrow::Cow::Borrowed(text.as_str())
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

fn text_open_context(tag_body: &[u8]) -> Option<TextContext> {
    if tag_body.is_empty() || tag_body[0] == b'/' {
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

    let name = &tag_body[..name_end];
    if name.eq_ignore_ascii_case(b"plaintext") {
        return Some(TextContext::Plaintext);
    }
    let tag = Tag::from_bytes(name);
    (tag.is_raw_text() || tag.is_rcdata()).then_some(TextContext::Element(tag))
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
    fn malformed_close_tag_prefix_matches_one_shot_across_chunk_sizes() {
        let html = b"</\"<Y\">\"\0>";
        let expected = crate::tokenize(std::str::from_utf8(html).unwrap());

        for chunk_size in [1, 9, html.len()] {
            let mut tokenizer = StreamTokenizer::new();
            let mut actual = Vec::new();
            for chunk in html.chunks(chunk_size) {
                actual.extend(tokenizer.feed(chunk));
            }
            actual.extend(tokenizer.finish());

            assert_eq!(actual, expected, "chunk_size={chunk_size}");
        }
    }

    #[test]
    fn overlapping_comment_terminator_matches_one_shot_across_chunk_sizes() {
        let html = b"<!--><!'-->'>";
        let expected = crate::tokenize(std::str::from_utf8(html).unwrap());

        for chunk_size in 1..=html.len() {
            let mut tokenizer = StreamTokenizer::new();
            let mut actual = Vec::new();
            for chunk in html.chunks(chunk_size) {
                actual.extend(tokenizer.feed(chunk));
            }
            actual.extend(tokenizer.finish());

            assert_eq!(actual, expected, "chunk_size={chunk_size}");
        }
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
    fn unfinished_raw_close_keeps_text_context_at_eof() {
        for (tag, decodes_entities) in [
            ("ScRiPt", false),
            ("StYlE", false),
            ("IfRaMe", false),
            ("TiTlE", true),
            ("TeXtArEa", true),
        ] {
            let html = format!("<{tag}>x</{tag} &amp;");
            let reference = if decodes_entities && cfg!(feature = "entity-decode") {
                "&"
            } else {
                "&amp;"
            };
            let expected = format!("x</{tag} {reference}");
            let check_tokens = |tokens: &[Token<'_>]| {
                let text: String = tokens
                    .iter()
                    .filter_map(|token| match token {
                        Token::Text { content } => Some(content.as_ref()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(text, expected, "tag={tag}");
                assert!(
                    !tokens
                        .iter()
                        .any(|token| matches!(token, Token::CloseTag { .. }))
                );
            };
            check_tokens(&crate::tokenize(&html));
            let mut one_shot_callback = Vec::new();
            crate::tokenize_with(&html, |token| one_shot_callback.push(token));
            check_tokens(&one_shot_callback);
            for chunk_size in [1, 7, html.len()] {
                let mut owned = StreamTokenizer::new();
                let mut owned_tokens = Vec::new();
                let mut callback = StreamTokenizer::new();
                let mut callback_tokens = Vec::new();
                for chunk in html.as_bytes().chunks(chunk_size) {
                    owned_tokens.extend(owned.feed(chunk));
                    callback.feed_str_with(std::str::from_utf8(chunk).unwrap(), |token| {
                        callback_tokens.push(to_owned_token(token.clone()));
                    });
                }
                owned_tokens.extend(owned.finish());
                callback.finish_with(|token| {
                    callback_tokens.push(to_owned_token(token.clone()));
                });

                for tokens in [owned_tokens, callback_tokens] {
                    check_tokens(&tokens);
                }
            }
        }
    }

    #[test]
    fn completed_raw_close_restores_entity_decoding_for_later_markup() {
        let html = "<script></script>outside</script &amp;";
        let expected = if cfg!(feature = "entity-decode") {
            "outside</script &"
        } else {
            "outside</script &amp;"
        };
        let one_shot = crate::tokenize(html);
        let mut one_shot_callback = Vec::new();
        crate::tokenize_with(html, |token| one_shot_callback.push(token));
        let mut tokenizer = StreamTokenizer::new();
        let mut streamed = Vec::new();
        for byte in html.as_bytes() {
            streamed.extend(tokenizer.feed(std::slice::from_ref(byte)));
        }
        streamed.extend(tokenizer.finish());

        for tokens in [one_shot, one_shot_callback, streamed] {
            let text: String = tokens
                .iter()
                .filter_map(|token| match token {
                    Token::Text { content } => Some(content.as_ref()),
                    _ => None,
                })
                .collect();
            assert_eq!(text, expected);
        }
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
        let first = format!(
            "<ScRiPt>x</ScRiPt{}",
            " ".repeat(MAX_RAW_TEXT_RESIDUAL + 1024)
        );
        let suffix = "><p>ok</p>";
        let html = format!("{first}{suffix}");
        let expected: Vec<_> = crate::tokenize(&html)
            .into_iter()
            .map(to_owned_token)
            .collect();

        let mut owned = StreamTokenizer::new();
        let mut owned_tokens = owned.feed(first.as_bytes());
        assert!(owned.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
        owned_tokens.extend(owned.feed(suffix.as_bytes()));
        owned_tokens.extend(owned.finish());

        let mut callback = StreamTokenizer::new();
        let mut callback_tokens = Vec::new();
        callback.feed_str_with(&first, |token| {
            callback_tokens.push(to_owned_token(token.clone()));
        });
        assert!(callback.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
        callback.feed_str_with(suffix, |token| {
            callback_tokens.push(to_owned_token(token.clone()));
        });
        callback.finish_with(|token| callback_tokens.push(to_owned_token(token.clone())));

        assert_eq!(owned_tokens, expected);
        assert_eq!(callback_tokens, expected);
        assert_eq!(owned.buffered_len(), 0);
        assert_eq!(callback.buffered_len(), 0);
    }

    #[test]
    fn oversized_raw_close_stays_bounded_and_preserves_quote_state() {
        let mut owned = StreamTokenizer::new();
        let mut owned_tokens = owned.feed(b"<ScRiPt>x</ScRiPt \"");
        let mut callback = StreamTokenizer::new();
        let mut callback_tokens = Vec::new();
        callback.feed_str_with("<ScRiPt>x</ScRiPt \"", |token| {
            callback_tokens.push(to_owned_token(token.clone()));
        });
        let chunk = format!("{}<img src=x>", " ".repeat(64 * 1024));
        for _ in 0..128 {
            owned_tokens.extend(owned.feed(chunk.as_bytes()));
            callback.feed_str_with(&chunk, |token| {
                callback_tokens.push(to_owned_token(token.clone()));
            });
            assert!(owned.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
            assert!(callback.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
        }
        let suffix = "\"><p>ok</p>";
        owned_tokens.extend(owned.feed(suffix.as_bytes()));
        owned_tokens.extend(owned.finish());
        callback.feed_str_with(suffix, |token| {
            callback_tokens.push(to_owned_token(token.clone()));
        });
        callback.finish_with(|token| callback_tokens.push(to_owned_token(token.clone())));

        let expected: Vec<_> = crate::tokenize("<ScRiPt>x</ScRiPt><p>ok</p>")
            .into_iter()
            .map(to_owned_token)
            .collect();
        assert_eq!(owned_tokens, expected);
        assert_eq!(callback_tokens, expected);
        assert_eq!(owned.buffered_len(), 0);
        assert_eq!(callback.buffered_len(), 0);
    }

    #[test]
    fn oversized_unfinished_raw_close_keeps_its_tail_in_text_context() {
        for (tag, decodes_entities) in [("script", false), ("title", true)] {
            let first = format!(
                "<{tag}>x</{tag} \"{}",
                "a".repeat(MAX_RAW_TEXT_RESIDUAL + 1)
            );
            let tail = "&amp;<b>EOF</b>";
            let expected_tail = if decodes_entities && cfg!(feature = "entity-decode") {
                "&<b>EOF</b>"
            } else {
                tail
            };
            let mut owned = StreamTokenizer::new();
            let mut owned_tokens = owned.feed(first.as_bytes());
            owned_tokens.extend(owned.feed(tail.as_bytes()));
            assert!(owned.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
            owned_tokens.extend(owned.finish());

            let mut callback = StreamTokenizer::new();
            let mut callback_tokens = Vec::new();
            for chunk in [&first[..], tail] {
                callback.feed_str_with(chunk, |token| {
                    callback_tokens.push(to_owned_token(token.clone()));
                });
            }
            assert!(callback.buffered_len() <= MAX_RAW_TEXT_RESIDUAL);
            callback.finish_with(|token| callback_tokens.push(to_owned_token(token.clone())));

            for tokens in [owned_tokens, callback_tokens] {
                let text: String = tokens
                    .iter()
                    .filter_map(|token| match token {
                        Token::Text { content } => Some(content.as_ref()),
                        _ => None,
                    })
                    .collect();
                assert!(text.ends_with(expected_tail), "tag={tag}");
                let open_tags: Vec<_> = tokens
                    .iter()
                    .filter_map(|token| match token {
                        Token::OpenTag { name, .. } => Some(name.as_ref()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(open_tags, [tag]);
                assert!(
                    !tokens
                        .iter()
                        .any(|token| matches!(token, Token::CloseTag { .. }))
                );
            }
        }
    }

    #[test]
    fn bounded_plain_text_flush_does_not_split_an_entity() {
        // The desired bounded-flush point falls inside `&amp;` (after `&a`).
        let input = format!("{}&amp;{}", "a".repeat(4090), "b".repeat(77));

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
        assert!(owned_tokenizer.buffered_len() <= TEXT_TAIL * 2);
        assert!(callback_tokenizer.buffered_len() <= TEXT_TAIL * 2);
    }

    #[cfg(feature = "entity-decode")]
    #[test]
    fn bounded_flush_preserves_named_and_legacy_entity_lookahead() {
        for reference in ["&notin;", "&notit;", "&copyx"] {
            let input = format!("{}{reference}{}", "a".repeat(4090), "b".repeat(80));
            let expected: String = crate::tokenize(&input)
                .iter()
                .filter_map(|token| match token {
                    Token::Text { content } => Some(content.as_ref()),
                    _ => None,
                })
                .collect();

            let mut tokenizer = StreamTokenizer::new();
            let mut tokens = tokenizer.feed(input.as_bytes());
            tokens.extend(tokenizer.finish());
            let actual: String = tokens
                .iter()
                .filter_map(|token| match token {
                    Token::Text { content } => Some(content.as_ref()),
                    _ => None,
                })
                .collect();
            assert_eq!(actual, expected, "reference={reference}");
        }
    }

    #[cfg(feature = "entity-decode")]
    #[test]
    fn overlong_numeric_entity_is_compacted_without_changing_value() {
        let first = format!("&#{}", "0".repeat(MAX_RESIDUAL + 256));
        let second = "60;tail";
        let complete = format!("{first}{second}");
        let expected = crate::entity::decode_entities(&complete);

        let mut owned = StreamTokenizer::new();
        let mut tokens = owned.feed(first.as_bytes());
        assert_eq!(owned.buffered_len(), 0);
        assert!(owned.numeric_entity.is_some());
        tokens.extend(owned.feed(second.as_bytes()));
        tokens.extend(owned.finish());
        let owned_text: String = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Text { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();

        let mut callback = StreamTokenizer::new();
        let mut callback_text = String::new();
        callback.feed_str_with(&first, |token| {
            if let Token::Text { content } = token {
                callback_text.push_str(content);
            }
        });
        callback.feed_str_with(second, |token| {
            if let Token::Text { content } = token {
                callback_text.push_str(content);
            }
        });
        callback.finish_with(|token| {
            if let Token::Text { content } = token {
                callback_text.push_str(content);
            }
        });

        assert_eq!(owned_text, expected);
        assert_eq!(callback_text, expected);
        assert_eq!(owned_text, "<tail");
    }

    #[cfg(feature = "entity-decode")]
    #[test]
    fn numeric_semicolon_exactly_at_flush_boundary_is_consumed() {
        let reference = format!("&#{}60;", "0".repeat(98));
        let input = format!(
            "{}{reference}{}",
            "a".repeat(4090),
            "b".repeat(TEXT_TAIL - 1)
        );
        let semicolon = input.find(';').unwrap();
        assert_eq!(semicolon, input.len() - TEXT_TAIL);
        let expected = crate::entity::decode_entities(&input);

        let mut tokenizer = StreamTokenizer::new();
        let mut tokens = tokenizer.feed(input.as_bytes());
        tokens.extend(tokenizer.finish());
        let actual: String = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Text { content } => Some(content.as_ref()),
                _ => None,
            })
            .collect();

        assert_eq!(actual, expected);
        assert!(!actual.contains("<;"));
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
