//! Streaming, incremental, and predicate-based HTML parsing.
//!
//! [`StreamParser`] buffers at most the first 1 KiB for encoding prescan and
//! processes caller-provided chunks in bounded 64 KiB blocks.
//! [`EarlyStopParser`] can stop either when a matching node is created or
//! after that element's subtree is complete.
//!
//! [`StreamParser`]: crate::streaming::StreamParser
//! [`EarlyStopParser`]: crate::streaming::EarlyStopParser

use encoding_rs::Encoding;
use fhp_core::error::ParseError;
use fhp_encoding::PRESCAN_LIMIT;
use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token;

use crate::builder::{TreeBuilder, recycle_buffer};
use crate::node::NodeId;
use crate::{Document, HtmlError, MAX_INPUT_SIZE, NodeRef};

/// Maximum amount of raw input processed as one internal unit.
const PROCESS_BLOCK_SIZE: usize = 64 * 1024;

/// Progress reported by [`EarlyStopParser::feed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarlyStopProgress {
    /// No match is complete yet; more input may be supplied.
    NeedMore,
    /// A match is ready. Call [`EarlyStopParser::finish`] to take ownership of it.
    Matched,
}

/// Describes how much of an early-stop match was parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchCompleteness {
    /// The start tag and attributes are present, but descendants may be absent.
    Created,
    /// The matching element and its complete subtree are present.
    SubtreeComplete,
}

/// An owned early-stop result.
pub struct EarlyStopMatch {
    document: Document,
    node_id: NodeId,
    completeness: MatchCompleteness,
}

impl EarlyStopMatch {
    /// The partial document that owns the matched node.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Consume the match and return its partial document.
    pub fn into_document(self) -> Document {
        self.document
    }

    /// The matched node id, valid within [`Self::document`].
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Borrow the matched node.
    pub fn node(&self) -> NodeRef<'_> {
        self.document.get(self.node_id)
    }

    /// Whether the match was returned at creation or after its subtree closed.
    pub fn completeness(&self) -> MatchCompleteness {
        self.completeness
    }
}

impl core::fmt::Debug for EarlyStopMatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EarlyStopMatch")
            .field("node_id", &self.node_id)
            .field("completeness", &self.completeness)
            .field("document_nodes", &self.document.node_count())
            .finish()
    }
}

/// Final result from [`EarlyStopParser`].
pub enum EarlyStopOutcome {
    /// The predicate matched and parsing stopped early.
    Matched(EarlyStopMatch),
    /// End of input was reached without a match.
    Done(Document),
}

impl core::fmt::Debug for EarlyStopOutcome {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Matched(found) => f.debug_tuple("Matched").field(found).finish(),
            Self::Done(document) => f
                .debug_struct("Done")
                .field("document_nodes", &document.node_count())
                .finish(),
        }
    }
}

/// Stateful decoder shared by both streaming parser variants.
struct DecoderState {
    decoder: encoding_rs::Decoder,
    decoded_size: usize,
    max_input_size: usize,
}

impl DecoderState {
    fn detect(sample: &[u8], max_input_size: usize) -> (Self, usize) {
        let encoding = fhp_encoding::detect(sample);
        let bom_len = fhp_encoding::bom_length(sample, encoding);
        (
            Self {
                decoder: encoding.new_decoder_without_bom_handling(),
                decoded_size: 0,
                max_input_size,
            },
            bom_len,
        )
    }

    /// Decode `bytes` and commit the output to the decoded-size accounting.
    fn decode(&mut self, bytes: &[u8], last: bool) -> Result<String, HtmlError> {
        let output = self.decode_lossy(bytes, last);
        let decoded_size = self.decoded_size.saturating_add(output.len());
        if decoded_size > self.max_input_size {
            return Err(HtmlError::InputTooLarge {
                size: decoded_size,
                max: self.max_input_size,
            });
        }
        self.decoded_size = decoded_size;
        Ok(output)
    }

    /// Decode `bytes`, replacing malformed sequences with U+FFFD. Unlike
    /// [`Self::decode`], decoded-size accounting is committed by the caller
    /// after it has checked for an early match.
    fn decode_lossy(&mut self, bytes: &[u8], last: bool) -> String {
        let mut output = String::new();
        self.decode_lossy_into(bytes, last, &mut output);
        output
    }

    /// [`Self::decode_lossy`] into `output`, replacing its contents but
    /// reusing its allocation.
    fn decode_lossy_into(&mut self, bytes: &[u8], last: bool, output: &mut String) {
        let max_len = self
            .decoder
            .max_utf8_buffer_length(bytes.len())
            .unwrap_or_else(|| bytes.len().saturating_mul(4).saturating_add(4));
        output.clear();
        output.reserve(max_len);
        let mut pos = 0;

        loop {
            let (result, read, _had_errors) =
                self.decoder.decode_to_string(&bytes[pos..], output, last);
            pos += read;
            match result {
                encoding_rs::CoderResult::InputEmpty => return,
                encoding_rs::CoderResult::OutputFull => {
                    let additional = self
                        .decoder
                        .max_utf8_buffer_length(bytes.len().saturating_sub(pos))
                        .unwrap_or(16);
                    output.reserve(additional.max(1));
                }
            }
        }
    }
}

/// A streaming HTML parser that incrementally builds a DOM.
pub struct StreamParser {
    tokenizer: StreamTokenizer,
    builder: TreeBuilder,
    decoder: Option<DecoderState>,
    initial_buf: Vec<u8>,
    seen_input_size: usize,
    max_input_size: usize,
    terminal: bool,
}

impl StreamParser {
    /// Create a parser with the default 256 MiB input limit.
    pub fn new() -> Self {
        Self::with_max_input_size(MAX_INPUT_SIZE)
    }

    /// Create a parser with a caller-provided raw and decoded input limit.
    ///
    /// Limits above 2 GiB are capped because arena offsets use `u32`.
    pub fn with_max_input_size(max_input_size: usize) -> Self {
        let max_input_size = effective_max(max_input_size);
        Self {
            tokenizer: StreamTokenizer::new(),
            builder: TreeBuilder::new(),
            decoder: None,
            initial_buf: Vec::with_capacity(PRESCAN_LIMIT),
            seen_input_size: 0,
            max_input_size,
            terminal: false,
        }
    }

    /// Feed raw bytes into the parser.
    ///
    /// The first failure is returned directly and makes the parser terminal.
    /// Later calls return [`HtmlError::ParserTerminated`] without consuming data.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), HtmlError> {
        if self.terminal {
            return Err(HtmlError::ParserTerminated);
        }
        if chunk.is_empty() {
            return Ok(());
        }

        let seen_input_size = self.seen_input_size.saturating_add(chunk.len());
        if seen_input_size > self.max_input_size {
            self.seen_input_size = seen_input_size;
            self.terminal = true;
            return Err(HtmlError::InputTooLarge {
                size: seen_input_size,
                max: self.max_input_size,
            });
        }
        self.seen_input_size = seen_input_size;

        let result = self.feed_active(chunk);
        if result.is_err() {
            self.terminal = true;
        }
        result
    }

    fn feed_active(&mut self, mut chunk: &[u8]) -> Result<(), HtmlError> {
        if self.decoder.is_none() {
            let needed = PRESCAN_LIMIT.saturating_sub(self.initial_buf.len());
            let take = needed.min(chunk.len());
            self.initial_buf.extend_from_slice(&chunk[..take]);
            chunk = &chunk[take..];

            if self.initial_buf.len() < PRESCAN_LIMIT {
                return Ok(());
            }
            self.flush_initial_buf()?;
        }

        for block in chunk.chunks(PROCESS_BLOCK_SIZE) {
            self.decode_and_process(block, false)?;
        }
        Ok(())
    }

    /// Finish parsing and return the completed document.
    pub fn finish(mut self) -> Result<Document, HtmlError> {
        if self.terminal {
            return Err(HtmlError::ParserTerminated);
        }
        if self.decoder.is_none() {
            self.flush_initial_buf()?;
        }
        self.decode_and_process(&[], true)?;
        self.finish_tokenizer()?;
        let (arena, root) = self.builder.finish()?;
        Ok(Document { arena, root })
    }

    fn flush_initial_buf(&mut self) -> Result<(), HtmlError> {
        let buf = core::mem::take(&mut self.initial_buf);
        let (decoder, bom_len) = DecoderState::detect(&buf, self.max_input_size);
        self.decoder = Some(decoder);
        self.decode_and_process(&buf[bom_len..], false)
    }

    fn decode_and_process(&mut self, bytes: &[u8], last: bool) -> Result<(), HtmlError> {
        let text = self
            .decoder
            .as_mut()
            .expect("decoder initialized before processing")
            .decode(bytes, last)?;
        self.process_text(&text)
    }

    fn process_text(&mut self, text: &str) -> Result<(), HtmlError> {
        let tokenizer = &mut self.tokenizer;
        let builder = &mut self.builder;
        let mut parse_error = None;
        tokenizer.feed_str_with(text, |token| {
            if parse_error.is_none() {
                if let Err(error) = builder.process(token) {
                    parse_error = Some(error);
                }
            }
        });
        match parse_error {
            Some(error) => Err(HtmlError::Parse(error)),
            None => Ok(()),
        }
    }

    fn finish_tokenizer(&mut self) -> Result<(), HtmlError> {
        let tokenizer = &mut self.tokenizer;
        let builder = &mut self.builder;
        let mut parse_error = None;
        tokenizer.finish_with(|token| {
            if parse_error.is_none() {
                if let Err(error) = builder.process(token) {
                    parse_error = Some(error);
                }
            }
        });
        match parse_error {
            Some(error) => Err(HtmlError::Parse(error)),
            None => Ok(()),
        }
    }
}

impl Default for StreamParser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EarlyStopMode {
    OnCreate,
    AfterElement,
}

type NodePredicate = dyn for<'a> Fn(NodeRef<'a>) -> bool;

/// A parser that stops at the first node matching a predicate.
pub struct EarlyStopParser {
    tokenizer: StreamTokenizer,
    builder: TreeBuilder,
    decoder: Option<DecoderState>,
    initial_buf: Vec<u8>,
    /// ASCII bytes from `initial_buf` already fed to the tokenizer before the
    /// encoding decision. The selected decoder is later primed over them.
    prescan_processed_ascii: usize,
    predicate: Box<NodePredicate>,
    mode: EarlyStopMode,
    pending_match: Option<NodeId>,
    found: Option<(NodeId, MatchCompleteness)>,
    seen_input_size: usize,
    max_input_size: usize,
    terminal: bool,
    /// Decoded text of the current segment. Input is decoded one `>` at a
    /// time, so the allocation is reused across segments.
    decode_buffer: String,
}

impl EarlyStopParser {
    /// Stop immediately after a matching node's start tag and attributes exist.
    pub fn stop_on_create(predicate: impl for<'a> Fn(NodeRef<'a>) -> bool + 'static) -> Self {
        Self::new(predicate, EarlyStopMode::OnCreate)
    }

    /// Stop after a matching element is closed, preserving its complete subtree.
    pub fn stop_after_element(predicate: impl for<'a> Fn(NodeRef<'a>) -> bool + 'static) -> Self {
        Self::new(predicate, EarlyStopMode::AfterElement)
    }

    fn new(predicate: impl for<'a> Fn(NodeRef<'a>) -> bool + 'static, mode: EarlyStopMode) -> Self {
        Self {
            tokenizer: StreamTokenizer::new(),
            // The predicate reads nodes, depths included, before `finish`.
            builder: TreeBuilder::new_observed(),
            decoder: None,
            initial_buf: Vec::with_capacity(PRESCAN_LIMIT),
            prescan_processed_ascii: 0,
            predicate: Box::new(predicate),
            mode,
            pending_match: None,
            found: None,
            seen_input_size: 0,
            max_input_size: effective_max(MAX_INPUT_SIZE),
            terminal: false,
            decode_buffer: String::new(),
        }
    }

    /// Set the maximum raw and decoded input size.
    pub fn max_input_size(mut self, max_input_size: usize) -> Self {
        self.max_input_size = effective_max(max_input_size);
        self
    }

    /// Feed bytes and report whether a match is ready.
    ///
    /// Encoding detection uses the same first-1-KiB prescan regardless of how
    /// callers divide the byte stream. Encoding-independent ASCII prefixes are
    /// still processed eagerly so common tag predicates can stop immediately.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<EarlyStopProgress, HtmlError> {
        if self.terminal {
            return Err(HtmlError::ParserTerminated);
        }
        if self.found.is_some() {
            return Ok(EarlyStopProgress::Matched);
        }
        if chunk.is_empty() {
            return Ok(EarlyStopProgress::NeedMore);
        }

        let seen_input_size = self.seen_input_size.saturating_add(chunk.len());
        let remaining = self.max_input_size.saturating_sub(self.seen_input_size);
        let consumed = chunk.len().min(remaining);
        let result = if consumed == 0 {
            Ok(EarlyStopProgress::NeedMore)
        } else {
            self.feed_active(&chunk[..consumed])
        };

        match result {
            Ok(EarlyStopProgress::Matched) => {
                // The unconsumed suffix is outside the early-stop result. In
                // particular, it cannot trigger raw-size errors.
                self.seen_input_size = self.seen_input_size.saturating_add(consumed);
                Ok(EarlyStopProgress::Matched)
            }
            Ok(EarlyStopProgress::NeedMore) if consumed < chunk.len() => {
                self.seen_input_size = seen_input_size;
                self.terminal = true;
                Err(HtmlError::InputTooLarge {
                    size: seen_input_size,
                    max: self.max_input_size,
                })
            }
            Ok(EarlyStopProgress::NeedMore) => {
                self.seen_input_size = seen_input_size;
                Ok(EarlyStopProgress::NeedMore)
            }
            Err(error) => {
                self.seen_input_size = self.seen_input_size.saturating_add(consumed);
                self.terminal = true;
                Err(error)
            }
        }
    }

    fn feed_active(&mut self, mut chunk: &[u8]) -> Result<EarlyStopProgress, HtmlError> {
        if self.decoder.is_none() {
            let needed = PRESCAN_LIMIT.saturating_sub(self.initial_buf.len());
            let take = needed.min(chunk.len());
            self.initial_buf.extend_from_slice(&chunk[..take]);
            chunk = &chunk[take..];

            if Encoding::for_bom(&self.initial_buf).is_some() {
                self.flush_initial_buf()?;
                if self.found.is_some() {
                    return Ok(EarlyStopProgress::Matched);
                }
            }

            if self.decoder.is_none() {
                self.process_prescan_ascii()?;
                if self.found.is_some() && self.prescan_prefix_survives_decoding() {
                    // Do not decode bytes after the match: early-stop
                    // intentionally leaves that suffix unobserved.
                    return Ok(EarlyStopProgress::Matched);
                }
                if self.found.is_none() && self.initial_buf.len() < PRESCAN_LIMIT {
                    return Ok(EarlyStopProgress::NeedMore);
                }
                // The prescan window is full, or the encoding selected by the
                // matched prefix does not decode it to the processed ASCII; the
                // flush then discards that eager work and decodes from the start.
                self.flush_initial_buf()?;
                if self.found.is_some() {
                    return Ok(EarlyStopProgress::Matched);
                }
            }
            // A BOM is an immediate, authoritative encoding decision. Once
            // initialized, continue below with bytes beyond the initial buffer.
        }

        for block in chunk.chunks(PROCESS_BLOCK_SIZE) {
            self.decode_segments_and_check(block, false)?;
            if self.found.is_some() {
                return Ok(EarlyStopProgress::Matched);
            }
        }
        Ok(EarlyStopProgress::NeedMore)
    }

    /// Process the newly available ASCII prefix before encoding is known.
    /// Every encoding selectable by an in-band HTML declaration preserves
    /// these bytes, except the replacement encoding (which decodes the whole
    /// input to one U+FFFD); ESC is excluded because it can change stateful
    /// decoders.
    fn process_prescan_ascii(&mut self) -> Result<(), HtmlError> {
        while let Some(&byte) = self.initial_buf.get(self.prescan_processed_ascii) {
            if !byte.is_ascii() || byte == 0x1B {
                break;
            }

            // A token can complete at any ASCII byte. Advance one byte at a
            // time within the bounded prescan so the encoding validator sees
            // exactly through the match, never a later declaration.
            let one_byte = [byte];
            let text = core::str::from_utf8(&one_byte).expect("an ASCII byte is valid UTF-8");
            self.process_and_check(text)?;
            self.prescan_processed_ascii += 1;
            if self.found.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// Whether the encoding selected by the consumed prefix decodes it to the
    /// ASCII text already processed. Only the replacement encoding does not.
    fn prescan_prefix_survives_decoding(&self) -> bool {
        let consumed = &self.initial_buf[..self.prescan_processed_ascii];
        let (mut decoder, bom_len) = DecoderState::detect(consumed, self.max_input_size);
        let ascii = &consumed[bom_len..];
        decoder.decode_lossy(ascii, false).as_bytes() == ascii
    }

    fn flush_initial_buf(&mut self) -> Result<(), HtmlError> {
        let buf = core::mem::take(&mut self.initial_buf);
        let (mut decoder, bom_len) = DecoderState::detect(&buf, self.max_input_size);
        let mut processed = self.prescan_processed_ascii.max(bom_len);

        if processed > bom_len {
            let ascii = &buf[bom_len..processed];
            if decoder.decode(ascii, false)?.as_bytes() != ascii {
                // The replacement encoding does not preserve the eagerly
                // processed ASCII: rebuild from the start of the input.
                self.discard_prescan_work();
                (decoder, _) = DecoderState::detect(&buf, self.max_input_size);
                processed = bom_len;
            }
        }
        self.decoder = Some(decoder);
        self.decode_segments_and_check(&buf[processed..], false)
    }

    /// Reset the tokenizer, tree, and match state built from the eagerly
    /// processed ASCII prefix.
    fn discard_prescan_work(&mut self) {
        self.tokenizer = StreamTokenizer::new();
        self.builder = TreeBuilder::new_observed();
        self.prescan_processed_ascii = 0;
        self.pending_match = None;
        self.found = None;
    }

    /// Decode one raw syntactic segment at a time. `>` is safe as a decoder
    /// input boundary even when it is part of a multibyte/stateful sequence;
    /// the decoder buffers incomplete state. This lets ordinary HTML tokens
    /// become observable, and an early match win, before a size error from
    /// later raw input is surfaced.
    fn decode_segments_and_check(&mut self, bytes: &[u8], last: bool) -> Result<(), HtmlError> {
        let mut start = 0;
        while let Some(relative) = memchr::memchr(b'>', &bytes[start..]) {
            let end = start + relative + 1;
            self.decode_and_check(&bytes[start..end], last && end == bytes.len())?;
            if self.found.is_some() {
                return Ok(());
            }
            start = end;
        }

        if start < bytes.len() {
            self.decode_and_check(&bytes[start..], last)?;
        } else if last && bytes.is_empty() {
            self.decode_and_check(bytes, true)?;
        }
        Ok(())
    }

    fn decode_and_check(&mut self, bytes: &[u8], last: bool) -> Result<(), HtmlError> {
        let mut text = core::mem::take(&mut self.decode_buffer);
        let result = self.decode_into_and_check(bytes, last, &mut text);
        self.decode_buffer = recycle_buffer(text);
        result
    }

    fn decode_into_and_check(
        &mut self,
        bytes: &[u8],
        last: bool,
        text: &mut String,
    ) -> Result<(), HtmlError> {
        let decoder = self
            .decoder
            .as_mut()
            .expect("decoder initialized before processing");
        decoder.decode_lossy_into(bytes, last, text);
        let decoded_before = decoder.decoded_size;
        let decoded_size = decoded_before.saturating_add(text.len());
        let remaining = self.max_input_size.saturating_sub(decoded_before);
        let mut process_len = text.len().min(remaining);
        while !text.is_char_boundary(process_len) {
            process_len -= 1;
        }

        self.process_and_check(&text[..process_len])?;
        if self.found.is_some() {
            return Ok(());
        }
        if decoded_size > self.max_input_size {
            return Err(HtmlError::InputTooLarge {
                size: decoded_size,
                max: self.max_input_size,
            });
        }

        self.decoder
            .as_mut()
            .expect("decoder remains initialized")
            .decoded_size = decoded_size;
        Ok(())
    }

    /// Finish parsing and take ownership of the match or complete document.
    pub fn finish(mut self) -> Result<EarlyStopOutcome, HtmlError> {
        if self.terminal {
            return Err(HtmlError::ParserTerminated);
        }
        if self.found.is_none() {
            if self.decoder.is_none() {
                self.flush_initial_buf()?;
            }
            if self.found.is_none() {
                self.decode_segments_and_check(&[], true)?;
            }
        }

        if self.found.is_none() {
            self.finish_tokenizer_and_check()?;
        }

        if self.found.is_none() {
            let flushed = self.builder.finish_observed_text_run();
            check_early_nodes(
                &self.builder,
                flushed,
                self.predicate.as_ref(),
                self.mode,
                &mut self.pending_match,
                &mut self.found,
            );
        }

        if self.found.is_none() && self.mode == EarlyStopMode::AfterElement {
            if let Some(id) = self.pending_match.take() {
                // EOF implicitly closes every remaining open element.
                self.found = Some((id, MatchCompleteness::SubtreeComplete));
            }
        }

        let found = self.found;
        let (arena, root) = self.builder.finish()?;
        let document = Document { arena, root };
        match found {
            Some((node_id, completeness)) => Ok(EarlyStopOutcome::Matched(EarlyStopMatch {
                document,
                node_id,
                completeness,
            })),
            None => Ok(EarlyStopOutcome::Done(document)),
        }
    }

    fn process_and_check(&mut self, text: &str) -> Result<(), HtmlError> {
        let tokenizer = &mut self.tokenizer;
        let builder = &mut self.builder;
        let predicate = &self.predicate;
        let mode = self.mode;
        let pending_match = &mut self.pending_match;
        let found = &mut self.found;
        let mut parse_error = None;

        tokenizer.feed_str_with(text, |token| {
            if found.is_some() || parse_error.is_some() {
                return;
            }
            if let Err(error) = process_early_token(
                builder,
                token,
                predicate.as_ref(),
                mode,
                pending_match,
                found,
            ) {
                parse_error = Some(error);
            }
        });

        match parse_error {
            Some(error) => Err(HtmlError::Parse(error)),
            None => Ok(()),
        }
    }

    fn finish_tokenizer_and_check(&mut self) -> Result<(), HtmlError> {
        let tokenizer = &mut self.tokenizer;
        let builder = &mut self.builder;
        let predicate = &self.predicate;
        let mode = self.mode;
        let pending_match = &mut self.pending_match;
        let found = &mut self.found;
        let mut parse_error = None;

        tokenizer.finish_with(|token| {
            if found.is_some() || parse_error.is_some() {
                return;
            }
            if let Err(error) = process_early_token(
                builder,
                token,
                predicate.as_ref(),
                mode,
                pending_match,
                found,
            ) {
                parse_error = Some(error);
            }
        });

        match parse_error {
            Some(error) => Err(HtmlError::Parse(error)),
            None => Ok(()),
        }
    }
}

fn process_early_token(
    builder: &mut TreeBuilder,
    token: &Token<'_>,
    predicate: &NodePredicate,
    mode: EarlyStopMode,
    pending_match: &mut Option<NodeId>,
    found: &mut Option<(NodeId, MatchCompleteness)>,
) -> Result<(), ParseError> {
    let is_text = matches!(token, Token::Text { .. } | Token::CData { .. });
    if !is_text {
        let flushed = builder.finish_observed_text_run();
        check_early_nodes(builder, flushed, predicate, mode, pending_match, found);
        if found.is_some() {
            return Ok(());
        }
    }

    let created = builder.process(token)?.filter(|_| !is_text);
    let flushed = builder.take_flushed_text_node();
    // Implied `tbody`, `tr`, and `colgroup` elements precede the element the
    // token itself created.
    let builder: &TreeBuilder = builder;
    let nodes = flushed
        .into_iter()
        .chain(builder.implied_elements().iter().copied())
        .chain(created);
    check_early_nodes(builder, nodes, predicate, mode, pending_match, found);
    Ok(())
}

fn check_early_nodes(
    builder: &TreeBuilder,
    created: impl IntoIterator<Item = NodeId>,
    predicate: &NodePredicate,
    mode: EarlyStopMode,
    pending_match: &mut Option<NodeId>,
    found: &mut Option<(NodeId, MatchCompleteness)>,
) {
    if pending_match.is_none() {
        for node_id in created {
            let node = NodeRef {
                arena: &builder.arena,
                id: node_id,
            };
            if predicate(node) {
                match mode {
                    EarlyStopMode::OnCreate => {
                        *found = Some((node_id, MatchCompleteness::Created));
                    }
                    EarlyStopMode::AfterElement => {
                        *pending_match = Some(node_id);
                    }
                }
            }
            if found.is_some() || pending_match.is_some() {
                break;
            }
        }
    }

    if found.is_none() {
        if let Some(node_id) = *pending_match {
            if !builder.is_open(node_id) {
                *found = Some((node_id, MatchCompleteness::SubtreeComplete));
            }
        }
    }
}

/// Parse byte chunks with the default input limit.
pub fn parse_stream<'a>(chunks: impl Iterator<Item = &'a [u8]>) -> Result<Document, HtmlError> {
    parse_stream_with_limit(chunks, MAX_INPUT_SIZE)
}

/// Parse byte chunks with a caller-provided raw and decoded input limit.
///
/// Iteration stops immediately when [`StreamParser::feed`] returns an error.
pub fn parse_stream_with_limit<'a>(
    chunks: impl Iterator<Item = &'a [u8]>,
    max_input_size: usize,
) -> Result<Document, HtmlError> {
    let mut parser = StreamParser::with_max_input_size(max_input_size);
    for chunk in chunks {
        parser.feed(chunk)?;
    }
    parser.finish()
}

fn effective_max(max_input_size: usize) -> usize {
    max_input_size.min(crate::MAX_REPRESENTABLE_INPUT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fhp_core::tag::Tag;

    #[test]
    fn large_first_chunk_is_processed_after_bounded_prescan() {
        let mut input = vec![b'x'; PRESCAN_LIMIT + PROCESS_BLOCK_SIZE * 2];
        input.extend_from_slice(b"<p>done</p>");
        let mut parser = StreamParser::new();
        parser.feed(&input).unwrap();
        assert!(parser.initial_buf.len() <= PRESCAN_LIMIT);
        let document = parser.finish().unwrap();
        assert!(document.root().text_content().ends_with("done"));
    }

    #[test]
    fn size_failure_is_immediate_and_terminal() {
        let mut parser = StreamParser::with_max_input_size(4);
        assert!(matches!(
            parser.feed(b"12345"),
            Err(HtmlError::InputTooLarge { size: 5, max: 4 })
        ));
        assert!(matches!(
            parser.feed(b"<p>ignored</p>"),
            Err(HtmlError::ParserTerminated)
        ));
        assert!(matches!(parser.finish(), Err(HtmlError::ParserTerminated)));
    }

    #[test]
    fn parse_stream_stops_pulling_after_error() {
        let pulls = core::cell::Cell::new(0);
        let chunks: [&[u8]; 3] = [b"1234", b"5678", b"ignored"];
        let iterator = chunks.into_iter().inspect(|_| pulls.set(pulls.get() + 1));
        assert!(matches!(
            parse_stream_with_limit(iterator, 4),
            Err(HtmlError::InputTooLarge { size: 8, max: 4 })
        ));
        assert_eq!(pulls.get(), 2);
    }

    #[test]
    fn early_stop_on_create_returns_owned_match() {
        let mut parser = EarlyStopParser::stop_on_create(|node| {
            node.tag() == Tag::A && node.attr("href") == Some("/target")
        });
        assert_eq!(
            parser
                .feed(b"<div><a href=\"/target\">link</a><span>after</span></div>")
                .unwrap(),
            EarlyStopProgress::Matched
        );
        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("expected a match")
        };
        assert_eq!(found.completeness(), MatchCompleteness::Created);
        assert_eq!(found.node().attr("href"), Some("/target"));
        assert!(!found.document().to_html().contains("after"));
    }

    #[test]
    fn early_stop_after_element_keeps_complete_subtree() {
        let mut parser = EarlyStopParser::stop_after_element(|node| node.tag() == Tag::Article);
        assert_eq!(
            parser
                .feed(b"<main><article><b>complete</b></article><p>after</p></main>")
                .unwrap(),
            EarlyStopProgress::Matched
        );
        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("expected a match")
        };
        assert_eq!(found.completeness(), MatchCompleteness::SubtreeComplete);
        assert_eq!(found.node().text_content(), "complete");
        assert!(!found.document().to_html().contains("after"));
    }

    #[test]
    fn replacement_labels_decode_to_one_replacement_character() {
        const REPLACEMENT_LABELS: [&str; 6] = [
            "csiso2022kr",
            "hz-gb-2312",
            "iso-2022-cn",
            "iso-2022-cn-ext",
            "iso-2022-kr",
            "replacement",
        ];

        fn assert_replacement_document(parser: EarlyStopParser, case: &str) {
            match parser.finish() {
                Ok(EarlyStopOutcome::Done(document)) => {
                    assert_eq!(document.root().text_content(), "\u{FFFD}", "case={case}");
                }
                other => panic!("expected an unmatched U+FFFD document for {case}, got {other:?}"),
            }
        }

        for label in REPLACEMENT_LABELS {
            let input = format!("<meta charset={label}><p id=hit>x</p>");
            let one_shot = crate::parse_bytes(input.as_bytes()).unwrap();
            assert_eq!(one_shot.root().text_content(), "\u{FFFD}", "label={label}");
            let streamed = parse_stream(input.as_bytes().chunks(1)).unwrap();
            assert_eq!(streamed.root().text_content(), "\u{FFFD}", "label={label}");

            for mode in ["on-create", "after-element", "never"] {
                let case = format!("label={label}, mode={mode}");
                let parser = || match mode {
                    "on-create" => {
                        EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"))
                    }
                    "after-element" => {
                        EarlyStopParser::stop_after_element(|node| node.attr("id") == Some("hit"))
                    }
                    _ => EarlyStopParser::stop_on_create(|_| false),
                };

                let mut whole = parser();
                assert_eq!(
                    whole.feed(input.as_bytes()).unwrap(),
                    EarlyStopProgress::NeedMore,
                    "{case}, whole"
                );
                assert_replacement_document(whole, &format!("{case}, whole"));

                let mut bytewise = parser();
                for byte in input.as_bytes() {
                    assert_eq!(
                        bytewise.feed(core::slice::from_ref(byte)).unwrap(),
                        EarlyStopProgress::NeedMore,
                        "{case}, bytewise"
                    );
                }
                assert_replacement_document(bytewise, &format!("{case}, bytewise"));
            }
        }
    }

    #[test]
    fn early_stop_on_create_does_not_decode_malformed_suffix_after_match() {
        let mut parser = EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"));
        assert_eq!(
            parser.feed(b"<p id=hit>\xFF").unwrap(),
            EarlyStopProgress::Matched
        );

        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("expected the start-tag match")
        };
        assert_eq!(found.node().attr("id"), Some("hit"));
    }

    #[test]
    fn early_stop_active_decoder_ignores_same_feed_malformed_suffix() {
        for after_element in [false, true] {
            let mut parser = if after_element {
                EarlyStopParser::stop_after_element(|node| node.attr("id") == Some("hit"))
            } else {
                EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"))
            };
            assert_eq!(
                parser.feed(&vec![b' '; PRESCAN_LIMIT]).unwrap(),
                EarlyStopProgress::NeedMore
            );

            let input: &[u8] = if after_element {
                b"<p id=hit>complete</p>\xFF"
            } else {
                b"<p id=hit>\xFF"
            };
            assert_eq!(
                parser.feed(input).unwrap(),
                EarlyStopProgress::Matched,
                "after_element={after_element}"
            );
            let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
                panic!("expected match for after_element={after_element}")
            };
            assert_eq!(found.node().attr("id"), Some("hit"));
        }
    }

    #[test]
    fn early_stop_ignores_replacement_meta_after_exact_match() {
        for after_element in [false, true] {
            let mut parser = if after_element {
                EarlyStopParser::stop_after_element(|node| node.attr("id") == Some("hit"))
            } else {
                EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"))
            };
            let input: &[u8] = if after_element {
                b"<p id=hit>complete</p><meta charset=replacement>"
            } else {
                b"<p id=hit><meta charset=replacement>"
            };

            assert_eq!(
                parser.feed(input).unwrap(),
                EarlyStopProgress::Matched,
                "after_element={after_element}"
            );
            let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
                panic!("expected match for after_element={after_element}")
            };
            assert!(!found.document().to_html().contains("<meta"));
        }
    }

    #[test]
    fn early_stop_utf8_bom_ignores_same_feed_malformed_suffix() {
        for after_element in [false, true] {
            let body: &[u8] = if after_element {
                b"<p id=hit>complete</p>\xFF"
            } else {
                b"<p id=hit>\xFF"
            };
            let mut input = b"\xEF\xBB\xBF".to_vec();
            input.extend_from_slice(body);
            let mut parser = if after_element {
                EarlyStopParser::stop_after_element(|node| node.attr("id") == Some("hit"))
            } else {
                EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"))
            };

            assert_eq!(
                parser.feed(&input).unwrap(),
                EarlyStopProgress::Matched,
                "after_element={after_element}"
            );
            assert!(matches!(parser.finish(), Ok(EarlyStopOutcome::Matched(_))));
        }
    }

    #[test]
    fn early_stop_utf16_bom_ignores_odd_byte_suffix_after_match() {
        for big_endian in [false, true] {
            for after_element in [false, true] {
                let html = if after_element {
                    "<p id=hit>complete</p>"
                } else {
                    "<p id=hit>"
                };
                let mut input = if big_endian {
                    vec![0xFE, 0xFF]
                } else {
                    vec![0xFF, 0xFE]
                };
                for unit in html.encode_utf16() {
                    let bytes = if big_endian {
                        unit.to_be_bytes()
                    } else {
                        unit.to_le_bytes()
                    };
                    input.extend_from_slice(&bytes);
                }
                input.push(b'x');

                let mut parser = if after_element {
                    EarlyStopParser::stop_after_element(|node| node.attr("id") == Some("hit"))
                } else {
                    EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"))
                };
                assert_eq!(
                    parser.feed(&input).unwrap(),
                    EarlyStopProgress::Matched,
                    "big_endian={big_endian}, after_element={after_element}"
                );
                assert!(matches!(parser.finish(), Ok(EarlyStopOutcome::Matched(_))));
            }
        }
    }

    #[test]
    fn early_stop_ignores_same_feed_suffix_beyond_raw_limit() {
        for after_element in [false, true] {
            let matched: &[u8] = if after_element {
                b"<p id=hit>complete</p>"
            } else {
                b"<p id=hit>"
            };
            let mut input = matched.to_vec();
            input.extend_from_slice(b"suffix beyond the configured limit");
            let mut parser = if after_element {
                EarlyStopParser::stop_after_element(|node| node.attr("id") == Some("hit"))
            } else {
                EarlyStopParser::stop_on_create(|node| node.attr("id") == Some("hit"))
            }
            .max_input_size(matched.len());

            assert_eq!(
                parser.feed(&input).unwrap(),
                EarlyStopProgress::Matched,
                "after_element={after_element}"
            );
            assert!(matches!(parser.finish(), Ok(EarlyStopOutcome::Matched(_))));
        }
    }

    #[test]
    fn early_stop_raw_limit_without_match_is_still_terminal() {
        let mut parser = EarlyStopParser::stop_on_create(|_| false).max_input_size(4);
        assert!(matches!(
            parser.feed(b"12345"),
            Err(HtmlError::InputTooLarge { size: 5, max: 4 })
        ));
        assert!(matches!(
            parser.feed(b"ignored"),
            Err(HtmlError::ParserTerminated)
        ));
        assert!(matches!(parser.finish(), Err(HtmlError::ParserTerminated)));
    }

    #[test]
    fn early_stop_text_predicates_observe_only_stable_logical_runs() {
        let input = format!("<p>{}</p>", "a".repeat(4_097));

        // A bounded tokenizer flush produces a 4,017-byte prefix for small
        // feeds. That transient size must not become predicate-visible.
        for chunk_size in [input.len(), 1] {
            let mut parser = EarlyStopParser::stop_on_create(|node| {
                node.is_text() && node.text_content().len() == 4_017
            });
            for chunk in input.as_bytes().chunks(chunk_size) {
                assert_eq!(
                    parser.feed(chunk).unwrap(),
                    EarlyStopProgress::NeedMore,
                    "chunk_size={chunk_size}"
                );
            }
            let EarlyStopOutcome::Done(document) = parser.finish().unwrap() else {
                panic!("transient text prefix matched for chunk_size={chunk_size}")
            };
            assert_eq!(document.root().text_content().len(), 4_097);
        }

        // The complete logical text node remains observable in both modes.
        for chunk_size in [input.len(), 1] {
            let mut parser = EarlyStopParser::stop_on_create(|node| {
                node.is_text() && node.text_content().len() == 4_097
            });
            let mut progress = EarlyStopProgress::NeedMore;
            for chunk in input.as_bytes().chunks(chunk_size) {
                progress = parser.feed(chunk).unwrap();
                if progress == EarlyStopProgress::Matched {
                    break;
                }
            }
            assert_eq!(
                progress,
                EarlyStopProgress::Matched,
                "chunk_size={chunk_size}"
            );
            let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
                panic!("stable text did not match for chunk_size={chunk_size}")
            };
            assert_eq!(found.node().text_content().len(), 4_097);
        }
    }

    #[test]
    fn early_stop_observes_deferred_table_whitespace_before_next_tag() {
        let mut parser =
            EarlyStopParser::stop_on_create(|node| node.is_text() && node.text_content() == "   ");
        let _ = parser
            .feed(b"<table>   <tr><td>x</td></tr></table>")
            .unwrap();

        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("expected the deferred table text to match")
        };
        assert_eq!(found.node().text_content(), "   ");
        assert!(!found.document().to_html().contains("<tr"));
    }

    #[test]
    fn streamed_bounded_text_matches_one_shot_node_cardinality() {
        let html = format!("<p>{}&amp;{}</p>", "a".repeat(4090), "b".repeat(300));
        let one_shot = crate::parse(&html).unwrap();
        let mut parser = StreamParser::new();
        for chunk in html.as_bytes().chunks(137) {
            parser.feed(chunk).unwrap();
        }
        let streamed = parser.finish().unwrap();

        assert_eq!(streamed.to_html(), one_shot.to_html());
        assert_eq!(streamed.node_count(), one_shot.node_count());
        let paragraph = streamed.root().children().next().unwrap();
        let children: Vec<_> = streamed.get(paragraph).children().collect();
        assert_eq!(children.len(), 1);
        assert!(streamed.get(children[0]).is_text());
    }

    #[test]
    fn fostered_text_node_cardinality_is_chunk_invariant() {
        let long_mixed_run = format!("<table>{}x<tr><td>y</td></tr></table>", " ".repeat(4_200));
        for html in [
            "<table>before<tr><td>x</td></tr>after</table>",
            "a<table>&amp;<tr></tr></table>",
            &long_mixed_run,
        ] {
            let borrowed = crate::parse(html).unwrap();
            let owned = crate::parse_owned(html.to_owned()).unwrap();
            let mut parser = StreamParser::new();
            for chunk in html.as_bytes().chunks(3) {
                parser.feed(chunk).unwrap();
            }
            let streamed = parser.finish().unwrap();

            assert_eq!(borrowed.to_html(), owned.to_html(), "html={html}");
            assert_eq!(borrowed.to_html(), streamed.to_html(), "html={html}");
            assert_eq!(borrowed.node_count(), owned.node_count(), "html={html}");
            assert_eq!(borrowed.node_count(), streamed.node_count(), "html={html}");
        }
    }

    #[test]
    fn early_stop_split_utf16_bom_is_chunk_invariant() {
        let mut input = vec![0xFF, 0xFE];
        for unit in "<p>BOM text</p>".encode_utf16() {
            input.extend_from_slice(&unit.to_le_bytes());
        }

        let mut bytewise = EarlyStopParser::stop_on_create(|_| false);
        for chunk in input.chunks(1) {
            assert_eq!(bytewise.feed(chunk).unwrap(), EarlyStopProgress::NeedMore);
        }
        let EarlyStopOutcome::Done(bytewise) = bytewise.finish().unwrap() else {
            panic!("unexpected match")
        };

        let mut whole = EarlyStopParser::stop_on_create(|_| false);
        whole.feed(&input).unwrap();
        let EarlyStopOutcome::Done(whole) = whole.finish().unwrap() else {
            panic!("unexpected match")
        };

        assert_eq!(bytewise.to_html(), whole.to_html());
        assert_eq!(bytewise.root().text_content(), "BOM text");
        assert!(!bytewise.root().text_content().contains('\u{FEFF}'));
    }

    #[test]
    fn early_stop_split_meta_encoding_is_chunk_invariant() {
        let mut input = b"<meta charset=\"windows-1254\"><p>".to_vec();
        input.push(0xFE); // ş in Windows-1254
        input.extend_from_slice(b"</p>");

        let mut bytewise = EarlyStopParser::stop_on_create(|_| false);
        for chunk in input.chunks(1) {
            bytewise.feed(chunk).unwrap();
        }
        let EarlyStopOutcome::Done(bytewise) = bytewise.finish().unwrap() else {
            panic!("unexpected match")
        };

        let mut whole = EarlyStopParser::stop_on_create(|_| false);
        whole.feed(&input).unwrap();
        let EarlyStopOutcome::Done(whole) = whole.finish().unwrap() else {
            panic!("unexpected match")
        };

        assert_eq!(bytewise.to_html(), whole.to_html());
        assert_eq!(bytewise.root().text_content(), "ş");
    }
}
