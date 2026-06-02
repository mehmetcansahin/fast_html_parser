//! SIMD-accelerated HTML tokenizer.
//!
//! Uses a two-stage pipeline inspired by simdjson:
//!
//! 1. **Structural indexing** (SIMD): scan input in 64-byte blocks, produce
//!    per-delimiter bitmasks, then apply quote-aware masking.
//! 2. **Token extraction** (scalar): walk the structural index to emit tokens
//!    via a branchless state machine.
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
/// Branchless state machine for token extraction.
pub mod state_machine;
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
    /// An opening tag was encountered.
    ///
    /// `attr_raw` is the raw attribute region between the tag name and `>`.
    /// The implementor is responsible for parsing attributes from this slice.
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
/// Fuses SIMD structural indexing, quote-aware masking, and token extraction
/// into a single streaming pass. No `Vec<BlockBitmaps>` or `Vec<Token>` is
/// allocated.
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
pub fn tokenize_with<'a>(input: &'a str, mut on_token: impl FnMut(Token<'a>)) {
    let bytes = input.as_bytes();
    let len = bytes.len();

    if len == 0 {
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
            parser.on_delimiter_cb(abs_pos, byte, &mut on_token);
        }

        block_start = block_end;
    }

    // Flush trailing text.
    parser.flush_trailing_cb(&mut on_token);
}

/// Tokenize HTML directly into a [`TreeSink`], bypassing all intermediate allocations.
///
/// Fuses SIMD structural indexing, quote-aware masking, and token extraction
/// into a single streaming pass. Instead of constructing `Token` enums, calls
/// sink methods directly — eliminating `Vec<Attribute>`, `Cow` wrappers, and
/// entity-decode `String` allocations in the hot loop.
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

    if len == 0 {
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
        }

        block_start = block_end;
    }

    // Flush trailing text.
    parser.flush_trailing_sink(sink);
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

    /// Compare `tokenize` and `tokenize_with` for equivalence.
    fn assert_equivalent(html: &str) {
        let vec_tokens = tokenize(html);
        let cb_tokens = collect_with(html);
        assert_eq!(
            vec_tokens.len(),
            cb_tokens.len(),
            "token count mismatch for {:?}",
            html
        );
        for (i, (a, b)) in vec_tokens.iter().zip(cb_tokens.iter()).enumerate() {
            assert_eq!(
                std::mem::discriminant(a),
                std::mem::discriminant(b),
                "token type mismatch at index {i} for {:?}: {a:?} vs {b:?}",
                html
            );
        }
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
}
