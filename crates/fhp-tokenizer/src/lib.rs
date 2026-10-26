//! SIMD-accelerated HTML tokenizer.
//!
//! Uses a two-stage pipeline inspired by simdjson:
//!
//! 1. **SIMD delimiter index**: scan input in 64-byte blocks and produce
//!    bitmasks of every `<`, `>`, `"` and `'` byte.
//! 2. **Scalar extraction**: walk the indexed delimiters in order and emit
//!    tokens, tracking each tag body with an incremental attribute grammar.
//!
//! # Quick Start
//!
//! ```
//! use fhp_tokenizer::tokenize;
//!
//! let tokens = tokenize("<div>hello</div>");
//! assert!(tokens.len() >= 3);
//! ```

/// Entity decoding with SIMD fast-path.
pub mod entity;
/// Token extraction — stage 2 (scalar state machine).
pub mod extract;
/// Streaming (chunk-based) tokenizer — see [`crate::streaming::StreamTokenizer`].
pub mod streaming;
/// Structural character indexer — SIMD-powered bitmask generation (stage 1).
pub mod structural;
/// Token types emitted by the tokenizer.
pub mod token;

use extract::extract_tokens;
use structural::StructuralIndexer;
use token::Token;

use fhp_core::tag::Tag;
use fhp_simd::AllMasks;
use fhp_simd::dispatch::ops;

/// Trait for receiving parsed HTML events directly, bypassing Token allocation.
///
/// Implementors receive raw parse events (open/close tags, text, etc.) and
/// can build their output structure in a single pass without intermediate
/// `Token` enum or `Vec<Attribute>` allocations.
pub trait TreeSink {
    /// Whether tokenization should continue delivering events.
    ///
    /// The default keeps existing sinks source-compatible. A sink that enters
    /// a terminal error state can return `false` to stop scanning immediately
    /// and suppress the trailing-text flush.
    #[inline]
    fn should_continue(&self) -> bool {
        true
    }

    /// An opening tag was encountered.
    ///
    /// `attr_raw` is the raw attribute region between the tag name and `>`.
    /// The implementor is responsible for parsing attributes from this slice.
    /// `self_closing` records only whether the source start tag contained a
    /// trailing slash. It does not make a non-void HTML element void; sinks
    /// must use [`Tag::is_void`] for tree-construction semantics.
    fn open_tag(&mut self, tag: Tag, name: &str, attr_raw: &str, self_closing: bool);

    /// A closing tag was encountered.
    fn close_tag(&mut self, tag: Tag, name: &str);

    /// Raw text content (not entity-decoded).
    ///
    /// The implementor is responsible for entity decoding if desired.
    fn text(&mut self, content: &str);

    /// A comment was encountered.
    fn comment(&mut self, content: &str);

    /// A DOCTYPE declaration was encountered.
    fn doctype(&mut self, content: &str);

    /// A CDATA section was encountered.
    fn cdata(&mut self, content: &str);
}

/// Tokenize an HTML string into a sequence of tokens.
///
/// Convenience wrapper that runs both pipeline stages:
/// 1. SIMD structural indexing
/// 2. State-machine token extraction
///
/// # Example
///
/// ```
/// use fhp_tokenizer::tokenize;
/// use fhp_tokenizer::token::Token;
///
/// let tokens = tokenize("<p>Hello &amp; world</p>");
///
/// // Should contain OpenTag, Text, CloseTag
/// assert!(tokens.iter().any(|t| matches!(t, Token::OpenTag { .. })));
/// assert!(tokens.iter().any(|t| matches!(t, Token::Text { .. })));
/// assert!(tokens.iter().any(|t| matches!(t, Token::CloseTag { .. })));
/// ```
pub fn tokenize<'a>(input: &'a str) -> Vec<Token<'a>> {
    let indexer = StructuralIndexer::new();
    let index = indexer.index(input.as_bytes());
    extract_tokens(input, &index)
}

/// Size of each processing block in bytes.
const BLOCK_SIZE: usize = 64;

/// Tokenize HTML and feed each token to a callback — zero intermediate allocation.
///
/// Fuses the SIMD delimiter index and scalar extraction (with its incremental
/// attribute grammar) into a single streaming pass. No `Vec<BlockBitmaps>` or
/// `Vec<Token>` is allocated.
///
/// # Example
///
/// ```
/// use fhp_tokenizer::tokenize_with;
/// use fhp_tokenizer::token::Token;
///
/// let mut count = 0;
/// tokenize_with("<div>hello</div>", |_token| { count += 1; });
/// assert!(count >= 3); // OpenTag, Text, CloseTag
/// ```
#[inline]
pub fn tokenize_with<'a>(input: &'a str, on_token: impl FnMut(Token<'a>)) {
    tokenize_with_text_decoding(input, true, on_token);
}

/// [`tokenize_with`], optionally emitting data and RCDATA text as written
/// instead of decoding its character references.
pub(crate) fn tokenize_with_text_decoding<'a>(
    input: &'a str,
    decode_text: bool,
    mut on_token: impl FnMut(Token<'a>),
) {
    let bytes = input.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return;
    }

    let dispatch = ops();
    let compute = dispatch.compute_all_masks;
    let mut parser = extract::Parser::new(input);
    if !decode_text {
        parser.keep_text_undecoded();
    }

    let mut block_start = 0;

    while block_start < len {
        let block_end = (block_start + BLOCK_SIZE).min(len);
        let chunk = &bytes[block_start..block_end];

        // Step 1: Compute all bitmasks in a single SIMD pass.
        // SAFETY: dispatch function pointer matches detected CPU features.
        let masks: AllMasks = unsafe { compute(chunk) };

        // Step 2: Iterate `<`, `>`, and quote bytes. The parser tracks
        // attribute-quote state itself, so a `>` inside a quoted attribute
        // value does not close the tag; quotes in text content are harmless
        // no-ops in Data mode. (No global string masking — that incorrectly
        // treated quotes in text/comments as attribute strings.)
        let combined = masks.lt | masks.gt | masks.quot | masks.apos;

        // Pre-mask: clear bits beyond the valid range for this chunk,
        // eliminating the per-iteration bounds check inside the loop.
        let valid_bits = block_end - block_start;
        let mut bits = if valid_bits < 64 {
            combined & ((1u64 << valid_bits) - 1)
        } else {
            combined
        };
        while bits != 0 {
            let bit_pos = bits.trailing_zeros() as usize;
            bits &= bits - 1; // clear lowest set bit

            let abs_pos = block_start + bit_pos;
            let byte = bytes[abs_pos];
            parser.on_delimiter_cb(abs_pos, byte, &mut on_token);
        }

        block_start = block_end;
    }

    // Flush trailing text.
    parser.flush_trailing_cb(&mut on_token);
}

/// Tokenize HTML directly into a [`TreeSink`], bypassing all intermediate allocations.
///
/// Fuses the SIMD delimiter index and scalar extraction (with its incremental
/// attribute grammar) into a single streaming pass. Instead of constructing
/// `Token` enums, calls sink methods directly — eliminating `Vec<Attribute>`,
/// `Cow` wrappers, and entity-decode `String` allocations in the hot loop.
///
/// # Example
///
/// ```
/// use fhp_tokenizer::{TreeSink, tokenize_into};
/// use fhp_core::tag::Tag;
///
/// struct Counter { tags: usize, texts: usize }
/// impl TreeSink for Counter {
///     fn open_tag(&mut self, _: Tag, _: &str, _: &str, _: bool) { self.tags += 1; }
///     fn close_tag(&mut self, _: Tag, _: &str) {}
///     fn text(&mut self, _: &str) { self.texts += 1; }
///     fn comment(&mut self, _: &str) {}
///     fn doctype(&mut self, _: &str) {}
///     fn cdata(&mut self, _: &str) {}
/// }
///
/// let mut c = Counter { tags: 0, texts: 0 };
/// tokenize_into("<div>hello</div>", &mut c);
/// assert_eq!(c.tags, 1);
/// assert_eq!(c.texts, 1);
/// ```
pub fn tokenize_into<S: TreeSink>(input: &str, sink: &mut S) {
    let bytes = input.as_bytes();
    let len = bytes.len();

    if len == 0 || !sink.should_continue() {
        return;
    }

    let dispatch = ops();
    let compute = dispatch.compute_all_masks;
    let mut parser = extract::Parser::new(input);

    let mut block_start = 0;

    while block_start < len {
        let block_end = (block_start + BLOCK_SIZE).min(len);
        let chunk = &bytes[block_start..block_end];

        // Step 1: Compute all bitmasks in a single SIMD pass.
        // SAFETY: dispatch function pointer matches detected CPU features.
        let masks: AllMasks = unsafe { compute(chunk) };

        // Step 2: Iterate `<`, `>`, and quote bytes. The parser tracks
        // attribute-quote state itself, so a `>` inside a quoted attribute
        // value does not close the tag; quotes in text content are harmless
        // no-ops in Data mode. (No global string masking — that incorrectly
        // treated quotes in text/comments as attribute strings.)
        let combined = masks.lt | masks.gt | masks.quot | masks.apos;

        // Pre-mask: clear bits beyond the valid range for this chunk,
        // eliminating the per-iteration bounds check inside the loop.
        let valid_bits = block_end - block_start;
        let mut bits = if valid_bits < 64 {
            combined & ((1u64 << valid_bits) - 1)
        } else {
            combined
        };
        while bits != 0 {
            let bit_pos = bits.trailing_zeros() as usize;
            bits &= bits - 1; // clear lowest set bit

            let abs_pos = block_start + bit_pos;
            let byte = bytes[abs_pos];
            parser.on_delimiter_sink(abs_pos, byte, sink);
            if !sink.should_continue() {
                return;
            }
        }

        block_start = block_end;
    }

    // Flush trailing text.
    if sink.should_continue() {
        parser.flush_trailing_sink(sink);
    }
}

#[cfg(test)]
mod tokenize_with_tests {
    use super::*;
    use token::Token;

    /// Collect tokens from `tokenize_with` into a Vec for comparison.
    fn collect_with(input: &str) -> Vec<Token<'_>> {
        let mut tokens = Vec::new();
        tokenize_with(input, |t| tokens.push(t));
        tokens
    }

    /// Compare `tokenize` and `tokenize_with` token for token, payloads
    /// included.
    fn assert_equivalent(html: &str) {
        assert_eq!(
            tokenize(html),
            collect_with(html),
            "token mismatch for {html:?}"
        );
    }

    #[test]
    fn equiv_simple_div() {
        assert_equivalent("<div>hello</div>");
    }

    #[test]
    fn equiv_self_closing() {
        assert_equivalent("<br/>");
    }

    #[test]
    fn equiv_attributes() {
        assert_equivalent("<a href=\"url\" class=\"link\">text</a>");
    }

    #[test]
    fn equiv_comment() {
        assert_equivalent("<!-- hello -->");
    }

    #[test]
    fn equiv_doctype() {
        assert_equivalent("<!DOCTYPE html>");
    }

    #[test]
    fn equiv_script() {
        assert_equivalent("<script>var x = 1 < 2;</script>");
    }

    #[test]
    fn equiv_entities() {
        assert_equivalent("a &amp; b");
    }

    #[test]
    fn equiv_entity_in_attr() {
        assert_equivalent("<div title=\"a &amp; b\">x</div>");
    }

    #[test]
    fn equiv_empty() {
        assert_equivalent("");
    }

    #[test]
    fn equiv_text_only() {
        assert_equivalent("just plain text");
    }

    #[test]
    fn equiv_nested() {
        assert_equivalent("<div><span>text</span></div>");
    }

    #[test]
    fn equiv_quotes_spanning_blocks() {
        // Build input with attribute value spanning a 64-byte block boundary.
        let mut input = String::from("<div data=\"");
        while input.len() < 60 {
            input.push('a');
        }
        input.push('<'); // delimiter inside quotes
        while input.len() < 80 {
            input.push('b');
        }
        input.push_str("\">end</div>");
        assert_equivalent(&input);
    }

    #[test]
    fn equiv_long_input() {
        let mut input = String::new();
        for i in 0..100 {
            input.push_str(&format!("<p class=\"c{i}\">text {i}</p>"));
        }
        assert_equivalent(&input);
    }

    #[test]
    fn equiv_mixed_content() {
        assert_equivalent(
            "<!DOCTYPE html><!-- comment --><div id=\"main\" class='header' disabled>\
             <p>Hello &amp; world</p><br/><script>if(a<b){}</script></div>",
        );
    }

    #[test]
    fn tokenize_into_stops_without_flushing_after_sink_is_terminal() {
        #[derive(Default)]
        struct StopSink {
            events: Vec<&'static str>,
            terminal: bool,
        }

        impl TreeSink for StopSink {
            fn should_continue(&self) -> bool {
                !self.terminal
            }

            fn open_tag(&mut self, _: Tag, _: &str, _: &str, _: bool) {
                self.events.push("open");
                self.terminal = true;
            }

            fn close_tag(&mut self, _: Tag, _: &str) {
                self.events.push("close");
            }

            fn text(&mut self, _: &str) {
                self.events.push("text");
            }

            fn comment(&mut self, _: &str) {
                self.events.push("comment");
            }

            fn doctype(&mut self, _: &str) {
                self.events.push("doctype");
            }

            fn cdata(&mut self, _: &str) {
                self.events.push("cdata");
            }
        }

        let mut sink = StopSink::default();
        tokenize_into("<div></div>must not be flushed", &mut sink);
        assert_eq!(sink.events, ["open"]);
    }
}
