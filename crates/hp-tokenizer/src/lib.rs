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
//! use hp_tokenizer::tokenize;
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
/// Streaming (chunk-based) tokenizer.
pub mod streaming;
/// Structural character indexer — SIMD-powered bitmask generation (stage 1).
pub mod structural;
/// Token types emitted by the tokenizer.
pub mod token;

use extract::extract_tokens;
use structural::StructuralIndexer;
use token::Token;

/// Tokenize an HTML string into a sequence of tokens.
///
/// Convenience wrapper that runs both pipeline stages:
/// 1. SIMD structural indexing
/// 2. State-machine token extraction
///
/// # Example
///
/// ```
/// use hp_tokenizer::tokenize;
/// use hp_tokenizer::token::Token;
///
/// let tokens = tokenize("<p>Hello &amp; world</p>");
///
/// // Should contain OpenTag, Text, CloseTag
/// assert!(tokens.iter().any(|t| matches!(t, Token::OpenTag { .. })));
/// assert!(tokens.iter().any(|t| matches!(t, Token::Text { .. })));
/// assert!(tokens.iter().any(|t| matches!(t, Token::CloseTag { .. })));
/// ```
pub fn tokenize<'a>(input: &'a str) -> Vec<Token<'a>> {
    let bytes = input.as_bytes();
    let indexer = StructuralIndexer::new();
    let index = indexer.index(bytes);
    extract_tokens(bytes, &index)
}
