//! Streaming, incremental, and predicate-based HTML parsing.
//!
//! [`StreamParser`](crate::streaming::StreamParser) buffers at most the first
//! 1 KiB for encoding prescan and processes caller-provided chunks in bounded
//! 64 KiB blocks. [`EarlyStopParser`](crate::streaming::EarlyStopParser) can
//! stop either when a matching node is created or after that element's subtree
//! is complete.

use encoding_rs::Encoding;
use fhp_core::error::{EncodingError, ParseError};
use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token;

use crate::builder::TreeBuilder;
use crate::node::NodeId;
use crate::{Document, HtmlError, MAX_INPUT_SIZE, NodeRef};

/// Maximum bytes retained for HTML encoding prescan.
const PRESCAN_LIMIT: usize = 1024;

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
    encoding: &'static Encoding,
    raw_offset: usize,
    decoded_size: usize,
    max_input_size: usize,
}

impl DecoderState {
    fn detect(sample: &[u8], max_input_size: usize) -> (Self, usize) {
        let encoding = fhp_encoding::detect(sample);
        let bom_len = bom_length(sample, encoding);
        (
            Self {
                decoder: encoding.new_decoder_without_bom_handling(),
                encoding,
                raw_offset: 0,
                decoded_size: 0,
                max_input_size,
            },
            bom_len,
        )
    }

    fn decode(&mut self, bytes: &[u8], last: bool) -> Result<String, HtmlError> {
        let max_len = self
            .decoder
            .max_utf8_buffer_length(bytes.len())
            .unwrap_or_else(|| bytes.len().saturating_mul(4).saturating_add(4));
        let mut output = String::with_capacity(max_len);
        let mut pos = 0;
        let mut malformed_offset = None;

        loop {
            let (result, read, had_errors) =
                self.decoder
                    .decode_to_string(&bytes[pos..], &mut output, last);
            if had_errors && malformed_offset.is_none() {
                malformed_offset = Some(self.raw_offset.saturating_add(pos));
            }
            pos += read;
            match result {
                encoding_rs::CoderResult::InputEmpty => break,
                encoding_rs::CoderResult::OutputFull => {
                    let additional = self
                        .decoder
                        .max_utf8_buffer_length(bytes.len().saturating_sub(pos))
                        .unwrap_or(16);
                    output.reserve(additional.max(1));
                }
            }
        }

        self.raw_offset = self.raw_offset.saturating_add(bytes.len());
        if let Some(offset) = malformed_offset {
            return Err(HtmlError::Encoding(EncodingError::MalformedInput {
                encoding: self.encoding.name(),
                offset,
            }));
        }

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
    /// Limits above `u32::MAX` are capped because arena offsets use `u32`.
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
    predicate: Box<NodePredicate>,
    mode: EarlyStopMode,
    pending_match: Option<NodeId>,
    found: Option<(NodeId, MatchCompleteness)>,
    seen_input_size: usize,
    max_input_size: usize,
    terminal: bool,
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
            builder: TreeBuilder::new(),
            decoder: None,
            predicate: Box::new(predicate),
            mode,
            pending_match: None,
            found: None,
            seen_input_size: 0,
            max_input_size: effective_max(MAX_INPUT_SIZE),
            terminal: false,
        }
    }

    /// Set the maximum raw and decoded input size.
    pub fn max_input_size(mut self, max_input_size: usize) -> Self {
        self.max_input_size = effective_max(max_input_size);
        self
    }

    /// Feed bytes and report whether a match is ready.
    ///
    /// Encoding detection is intentionally eager: at most the first 1 KiB of
    /// the first non-empty chunk is inspected so early termination is not
    /// delayed until the prescan window fills.
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

    fn feed_active(&mut self, chunk: &[u8]) -> Result<EarlyStopProgress, HtmlError> {
        let mut input = chunk;
        if self.decoder.is_none() {
            let sample = &chunk[..chunk.len().min(PRESCAN_LIMIT)];
            let (decoder, bom_len) = DecoderState::detect(sample, self.max_input_size);
            self.decoder = Some(decoder);
            input = &chunk[bom_len.min(chunk.len())..];
        }

        for block in input.chunks(PROCESS_BLOCK_SIZE) {
            let text = self
                .decoder
                .as_mut()
                .expect("decoder initialized before processing")
                .decode(block, false)?;
            self.process_and_check(&text)?;
            if self.found.is_some() {
                return Ok(EarlyStopProgress::Matched);
            }
        }
        Ok(EarlyStopProgress::NeedMore)
    }

    /// Finish parsing and take ownership of the match or complete document.
    pub fn finish(mut self) -> Result<EarlyStopOutcome, HtmlError> {
        if self.terminal {
            return Err(HtmlError::ParserTerminated);
        }
        if self.found.is_none() {
            if self.decoder.is_none() {
                let (decoder, _) = DecoderState::detect(&[], self.max_input_size);
                self.decoder = Some(decoder);
            }
            let trailing = self
                .decoder
                .as_mut()
                .expect("decoder initialized before finish")
                .decode(&[], true)?;
            self.process_and_check(&trailing)?;
        }

        if self.found.is_none() {
            self.finish_tokenizer_and_check()?;
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
    let created = builder.process(token)?;
    if pending_match.is_none() {
        if let Some(node_id) = created {
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
        }
    }

    if found.is_none() {
        if let Some(node_id) = *pending_match {
            if !builder.is_open(node_id) {
                *found = Some((node_id, MatchCompleteness::SubtreeComplete));
            }
        }
    }
    Ok(())
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
    max_input_size.min(usize::try_from(u32::MAX).unwrap_or(usize::MAX))
}

fn bom_length(input: &[u8], encoding: &'static Encoding) -> usize {
    if encoding == encoding_rs::UTF_8 && input.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else if (encoding == encoding_rs::UTF_16LE && input.starts_with(&[0xFF, 0xFE]))
        || (encoding == encoding_rs::UTF_16BE && input.starts_with(&[0xFE, 0xFF]))
    {
        2
    } else {
        0
    }
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
}
