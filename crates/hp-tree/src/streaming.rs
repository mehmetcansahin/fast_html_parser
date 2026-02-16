//! Streaming and incremental HTML parsing.
//!
//! [`StreamParser`] builds a DOM tree incrementally from byte chunks,
//! handling encoding detection automatically. It buffers the first 1 KB
//! of input (matching the meta prescan limit) before detecting encoding,
//! then decodes and tokenizes all data into the tree.
//!
//! [`EarlyStopParser`] adds predicate-based early termination — the parse
//! stops as soon as a matching node is found, saving work on large documents.
//!
//! # Example
//!
//! ```
//! use hp_tree::streaming::{StreamParser, parse_stream};
//!
//! let html = b"<div><p>Hello</p></div>";
//! let doc = parse_stream(html.chunks(7)).unwrap();
//! assert_eq!(doc.root().text_content(), "Hello");
//! ```

use encoding_rs::Encoding;
use hp_core::error::EncodingError;
use hp_tokenizer::streaming::StreamTokenizer;

use crate::builder::TreeBuilder;
use crate::node::{Node, NodeId};
use crate::{Document, HtmlError, MAX_INPUT_SIZE};

/// Maximum bytes to buffer before encoding detection (matches prescan limit).
const PRESCAN_LIMIT: usize = 1024;

/// Status returned by [`EarlyStopParser::feed`].
pub enum ParseStatus {
    /// More data is needed to satisfy the predicate.
    NeedMore,
    /// A node matching the predicate was found.
    Found(NodeId),
    /// Parsing finished without finding a match.
    Done(Document),
}

impl core::fmt::Debug for ParseStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NeedMore => write!(f, "NeedMore"),
            Self::Found(id) => write!(f, "Found({id:?})"),
            Self::Done(doc) => write!(f, "Done(Document {{ nodes: {} }})", doc.node_count()),
        }
    }
}

/// A streaming HTML parser that builds a DOM tree incrementally.
///
/// Buffers the first 1 KB of input for encoding detection (matching the
/// HTML spec's meta prescan limit), then decodes and tokenizes all
/// subsequent chunks into the tree.
///
/// # Example
///
/// ```
/// use hp_tree::streaming::StreamParser;
///
/// let mut parser = StreamParser::new();
/// parser.feed(b"<div>");
/// parser.feed(b"<p>Hello</p>");
/// parser.feed(b"</div>");
/// let doc = parser.finish().unwrap();
/// assert_eq!(doc.root().text_content(), "Hello");
/// ```
pub struct StreamParser {
    tokenizer: StreamTokenizer,
    builder: TreeBuilder,
    decoder: Option<encoding_rs::Decoder>,
    detected_encoding: Option<&'static Encoding>,
    /// Buffered initial bytes before encoding detection.
    initial_buf: Vec<u8>,
    /// Whether encoding has been detected and initial buffer flushed.
    encoding_detected: bool,
    /// Total raw input bytes received.
    seen_input_size: usize,
    /// Maximum allowed input size.
    max_input_size: usize,
    /// Input exceeded `max_input_size`.
    input_too_large: bool,
    /// First decoding error seen during streaming decode.
    decode_error: Option<EncodingError>,
    /// Approximate input byte offset consumed by decoder.
    decoded_input_offset: usize,
}

impl StreamParser {
    /// Create a new streaming parser.
    pub fn new() -> Self {
        Self {
            tokenizer: StreamTokenizer::new(),
            builder: TreeBuilder::new(),
            decoder: None,
            detected_encoding: None,
            initial_buf: Vec::with_capacity(PRESCAN_LIMIT),
            encoding_detected: false,
            seen_input_size: 0,
            max_input_size: MAX_INPUT_SIZE,
            input_too_large: false,
            decode_error: None,
            decoded_input_offset: 0,
        }
    }

    /// Feed a chunk of raw bytes into the parser.
    ///
    /// Initial chunks are buffered until 1 KB is reached for encoding
    /// detection. After that, each chunk is decoded and processed
    /// immediately.
    pub fn feed(&mut self, chunk: &[u8]) {
        if chunk.is_empty() || self.input_too_large {
            return;
        }

        self.seen_input_size = self.seen_input_size.saturating_add(chunk.len());
        if self.seen_input_size > self.max_input_size {
            self.input_too_large = true;
            return;
        }

        if !self.encoding_detected {
            self.initial_buf.extend_from_slice(chunk);
            if self.initial_buf.len() >= PRESCAN_LIMIT {
                self.flush_initial_buf();
            }
            return;
        }

        let text = self.decode_bytes(chunk, false);
        self.process_text(&text);
    }

    /// Finish parsing and return the completed document.
    ///
    /// If encoding hasn't been detected yet (total input < 1 KB), detection
    /// happens now.
    pub fn finish(mut self) -> Result<Document, HtmlError> {
        if self.input_too_large {
            return Err(HtmlError::InputTooLarge {
                size: self.seen_input_size,
                max: self.max_input_size,
            });
        }

        // If encoding was never detected, do it now with whatever we have.
        if !self.encoding_detected {
            self.flush_initial_buf();
        }

        if let Some(err) = self.decode_error.take() {
            return Err(HtmlError::Encoding(err));
        }

        // Signal end-of-stream to the decoder for any trailing bytes.
        let trailing = self.decode_bytes(&[], true);
        if !trailing.is_empty() {
            self.process_text(&trailing);
        }

        if let Some(err) = self.decode_error.take() {
            return Err(HtmlError::Encoding(err));
        }

        // Flush the tokenizer.
        {
            let tokenizer = &mut self.tokenizer;
            let builder = &mut self.builder;
            tokenizer.finish_with(|token| {
                builder.process(token);
            });
        }

        let (arena, root) = self.builder.finish();
        Ok(Document { arena, root })
    }

    /// Detect encoding from the initial buffer and process its contents.
    fn flush_initial_buf(&mut self) {
        let buf = std::mem::take(&mut self.initial_buf);
        let encoding = hp_encoding::detect(&buf);
        let bom_len = bom_length(&buf, encoding);
        self.decoder = Some(encoding.new_decoder_without_bom_handling());
        self.detected_encoding = Some(encoding);
        self.encoding_detected = true;

        let data = &buf[bom_len..];
        if !data.is_empty() {
            let text = self.decode_bytes(data, false);
            self.process_text(&text);
        }
    }

    /// Decode raw bytes to a UTF-8 string using the stateful decoder.
    fn decode_bytes(&mut self, bytes: &[u8], last: bool) -> String {
        let decoder = self.decoder.as_mut().expect("decoder not initialized");
        let max_len = decoder
            .max_utf8_buffer_length(bytes.len())
            .unwrap_or(bytes.len() * 4 + 4);
        let mut output = String::with_capacity(max_len);

        let mut pos = 0;
        loop {
            let (result, read, had_errors) =
                decoder.decode_to_string(&bytes[pos..], &mut output, last);
            if had_errors && self.decode_error.is_none() {
                let encoding = self.detected_encoding.unwrap_or(encoding_rs::UTF_8);
                self.decode_error = Some(EncodingError::MalformedInput {
                    encoding: encoding.name(),
                    offset: self.decoded_input_offset.saturating_add(pos),
                });
            }
            pos += read;
            match result {
                encoding_rs::CoderResult::InputEmpty => break,
                encoding_rs::CoderResult::OutputFull => {
                    let additional = decoder
                        .max_utf8_buffer_length(bytes.len() - pos)
                        .unwrap_or(16);
                    output.reserve(additional);
                }
            }
        }

        self.decoded_input_offset = self.decoded_input_offset.saturating_add(bytes.len());
        output
    }

    /// Feed decoded text to the tokenizer, then process tokens with the builder.
    fn process_text(&mut self, text: &str) {
        let tokenizer = &mut self.tokenizer;
        let builder = &mut self.builder;
        tokenizer.feed_str_with(text, |token| {
            builder.process(token);
        });
    }
}

impl Default for StreamParser {
    fn default() -> Self {
        Self::new()
    }
}

/// A streaming parser that stops as soon as a predicate matches a node.
///
/// Useful for extracting a single element from a large HTML document
/// without parsing the entire thing.
///
/// # Example
///
/// ```
/// use hp_tree::streaming::EarlyStopParser;
/// use hp_tree::streaming::ParseStatus;
/// use hp_core::tag::Tag;
///
/// let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
/// let html = b"<div><p>text</p><a href=\"#\">link</a><span>more</span></div>";
///
/// match parser.feed(html) {
///     ParseStatus::Found(id) => {
///         // Found the <a> tag — no need to parse <span>more</span>.
///     }
///     _ => panic!("expected Found"),
/// }
/// ```
pub struct EarlyStopParser {
    tokenizer: StreamTokenizer,
    builder: TreeBuilder,
    decoder: Option<encoding_rs::Decoder>,
    encoding_detected: bool,
    predicate: Box<dyn Fn(&Node) -> bool>,
    found: Option<NodeId>,
}

impl EarlyStopParser {
    /// Create a parser that stops when `predicate` returns `true` for a node.
    pub fn stop_when(predicate: impl Fn(&Node) -> bool + 'static) -> Self {
        Self {
            tokenizer: StreamTokenizer::new(),
            builder: TreeBuilder::new(),
            decoder: None,
            encoding_detected: false,
            predicate: Box::new(predicate),
            found: None,
        }
    }

    /// Feed a chunk of raw bytes and check the predicate against new nodes.
    ///
    /// Returns [`ParseStatus::Found`] as soon as a matching node is created,
    /// or [`ParseStatus::NeedMore`] if the predicate hasn't matched yet.
    ///
    /// Encoding is detected eagerly from the first non-empty chunk to
    /// minimise latency. For accurate meta-charset detection with small
    /// chunks, use [`StreamParser`] instead.
    pub fn feed(&mut self, chunk: &[u8]) -> ParseStatus {
        if let Some(id) = self.found {
            return ParseStatus::Found(id);
        }

        if chunk.is_empty() {
            return ParseStatus::NeedMore;
        }

        // Detect encoding eagerly from the first chunk.
        if !self.encoding_detected {
            let encoding = hp_encoding::detect(chunk);
            let bom_len = bom_length(chunk, encoding);
            self.decoder = Some(encoding.new_decoder_without_bom_handling());
            self.encoding_detected = true;

            let data = &chunk[bom_len..];
            if data.is_empty() {
                return ParseStatus::NeedMore;
            }
            let text = self.decode_bytes(data, false);
            return self.process_and_check(&text);
        }

        let text = self.decode_bytes(chunk, false);
        self.process_and_check(&text)
    }

    /// Finish parsing and return the document.
    ///
    /// If the predicate was already matched, returns [`ParseStatus::Found`].
    /// Otherwise returns [`ParseStatus::Done`] with the complete document.
    pub fn finish(mut self) -> ParseStatus {
        if let Some(id) = self.found {
            return ParseStatus::Found(id);
        }

        // If nothing was ever fed, initialise a UTF-8 decoder.
        if !self.encoding_detected {
            self.decoder = Some(encoding_rs::UTF_8.new_decoder_without_bom_handling());
            self.encoding_detected = true;
        }

        // Signal end-of-stream to decoder.
        let trailing = self.decode_bytes(&[], true);
        if !trailing.is_empty() {
            if let ParseStatus::Found(id) = self.process_and_check(&trailing) {
                return ParseStatus::Found(id);
            }
        }

        // Flush tokenizer.
        {
            let tokenizer = &mut self.tokenizer;
            let builder = &mut self.builder;
            let predicate = &self.predicate;
            let found = &mut self.found;
            tokenizer.finish_with(|token| {
                if found.is_some() {
                    return;
                }
                if let Some(node_id) = builder.process(token) {
                    let node = builder.arena.get(node_id);
                    if predicate(node) {
                        *found = Some(node_id);
                    }
                }
            });
        }
        if let Some(id) = self.found {
            return ParseStatus::Found(id);
        }

        let (arena, root) = self.builder.finish();
        ParseStatus::Done(Document { arena, root })
    }

    /// Decode, tokenize, build, and check predicate. Returns Found or NeedMore.
    fn process_and_check(&mut self, text: &str) -> ParseStatus {
        {
            let tokenizer = &mut self.tokenizer;
            let builder = &mut self.builder;
            let predicate = &self.predicate;
            let found = &mut self.found;
            tokenizer.feed_str_with(text, |token| {
                if found.is_some() {
                    return;
                }
                if let Some(node_id) = builder.process(token) {
                    let node = builder.arena.get(node_id);
                    if predicate(node) {
                        *found = Some(node_id);
                    }
                }
            });
        }
        match self.found {
            Some(id) => ParseStatus::Found(id),
            None => ParseStatus::NeedMore,
        }
    }

    /// Decode raw bytes using the stateful decoder.
    fn decode_bytes(&mut self, bytes: &[u8], last: bool) -> String {
        let decoder = self.decoder.as_mut().expect("decoder not initialized");
        let max_len = decoder
            .max_utf8_buffer_length(bytes.len())
            .unwrap_or(bytes.len() * 4 + 4);
        let mut output = String::with_capacity(max_len);

        let mut pos = 0;
        loop {
            let (result, read, _had_errors) =
                decoder.decode_to_string(&bytes[pos..], &mut output, last);
            pos += read;
            match result {
                encoding_rs::CoderResult::InputEmpty => break,
                encoding_rs::CoderResult::OutputFull => {
                    let additional = decoder
                        .max_utf8_buffer_length(bytes.len() - pos)
                        .unwrap_or(16);
                    output.reserve(additional);
                }
            }
        }

        output
    }
}

/// Parse an HTML document from an iterator of byte chunks.
///
/// Detects encoding from the first 1 KB and builds the tree incrementally.
///
/// # Example
///
/// ```
/// use hp_tree::streaming::parse_stream;
///
/// let html = b"<div><p>Hello</p></div>";
/// let doc = parse_stream(html.chunks(64)).unwrap();
/// assert_eq!(doc.root().text_content(), "Hello");
/// ```
pub fn parse_stream<'a>(chunks: impl Iterator<Item = &'a [u8]>) -> Result<Document, HtmlError> {
    let mut parser = StreamParser::new();
    for chunk in chunks {
        parser.feed(chunk);
    }
    parser.finish()
}

/// Determine the BOM length to strip for a given encoding.
fn bom_length(input: &[u8], encoding: &'static Encoding) -> usize {
    if encoding == encoding_rs::UTF_8
        && input.len() >= 3
        && input[0] == 0xEF
        && input[1] == 0xBB
        && input[2] == 0xBF
    {
        return 3;
    }
    if encoding == encoding_rs::UTF_16LE && input.len() >= 2 && input[0] == 0xFF && input[1] == 0xFE
    {
        return 2;
    }
    if encoding == encoding_rs::UTF_16BE && input.len() >= 2 && input[0] == 0xFE && input[1] == 0xFF
    {
        return 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hp_core::tag::Tag;

    #[test]
    fn stream_parser_single_chunk() {
        let mut parser = StreamParser::new();
        parser.feed(b"<div><p>Hello</p></div>");
        let doc = parser.finish().unwrap();
        assert_eq!(doc.root().text_content(), "Hello");
    }

    #[test]
    fn stream_parser_multiple_chunks() {
        let mut parser = StreamParser::new();
        parser.feed(b"<div>");
        parser.feed(b"<p>Hello</p>");
        parser.feed(b"</div>");
        let doc = parser.finish().unwrap();
        assert_eq!(doc.root().text_content(), "Hello");
    }

    #[test]
    fn stream_parser_empty_chunks() {
        let mut parser = StreamParser::new();
        parser.feed(b"");
        parser.feed(b"<p>ok</p>");
        parser.feed(b"");
        let doc = parser.finish().unwrap();
        assert_eq!(doc.root().text_content(), "ok");
    }

    #[test]
    fn stream_parser_byte_by_byte() {
        let html = b"<div>hi</div>";
        let mut parser = StreamParser::new();
        for &b in html.iter() {
            parser.feed(&[b]);
        }
        let doc = parser.finish().unwrap();
        let text = doc.root().text_content();
        assert!(text.contains("hi"), "text: {text}");
    }

    #[test]
    fn parse_stream_convenience() {
        let html = b"<div><p>Hello</p></div>";
        let doc = parse_stream(html.chunks(7)).unwrap();
        assert_eq!(doc.root().text_content(), "Hello");
    }

    #[test]
    fn early_stop_finds_tag() {
        let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
        let html = b"<div><p>text</p><a href=\"#\">link</a><span>more</span></div>";
        match parser.feed(html) {
            ParseStatus::Found(_id) => {}
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn early_stop_not_found() {
        let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
        let html = b"<div><p>no links here</p></div>";
        match parser.feed(html) {
            ParseStatus::NeedMore => {}
            other => panic!("expected NeedMore, got {other:?}"),
        }
        match parser.finish() {
            ParseStatus::Done(doc) => {
                assert_eq!(doc.root().text_content(), "no links here");
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn early_stop_multi_chunk() {
        let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
        assert!(matches!(
            parser.feed(b"<div><p>text</p>"),
            ParseStatus::NeedMore
        ));
        match parser.feed(b"<a href=\"#\">link</a></div>") {
            ParseStatus::Found(_id) => {}
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn bom_length_utf8() {
        assert_eq!(bom_length(b"\xEF\xBB\xBF<html>", encoding_rs::UTF_8), 3);
        assert_eq!(bom_length(b"<html>", encoding_rs::UTF_8), 0);
    }

    #[test]
    fn bom_length_utf16() {
        assert_eq!(bom_length(b"\xFF\xFE<\x00", encoding_rs::UTF_16LE), 2);
        assert_eq!(bom_length(b"\xFE\xFF\x00<", encoding_rs::UTF_16BE), 2);
    }

    #[test]
    fn stream_encoding_windows_1254() {
        let mut html = b"<meta charset=\"windows-1254\"><body>".to_vec();
        html.extend_from_slice(&[0xFE, 0xF0]); // ş, ğ in windows-1254
        html.extend_from_slice(b"</body>");

        let doc = parse_stream(html.chunks(15)).unwrap();
        let text = doc.root().text_content();
        assert!(text.contains('ş'), "text: {text}");
        assert!(text.contains('ğ'), "text: {text}");
    }
}
