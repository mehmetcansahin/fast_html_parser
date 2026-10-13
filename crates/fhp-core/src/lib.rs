pub mod attr;
/// HTML named and numeric entity decoding.
pub mod entity;
/// Error types for the HTML parser.
pub mod error;
/// Fast hash functions for selector matching.
pub mod hash;
/// Interned HTML tag names with PHF lookup.
pub mod tag;

/// Returns `true` for the five HTML ASCII whitespace bytes: space, tab, LF,
/// form feed, and CR.
#[inline(always)]
pub const fn is_html_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0C' | b'\r')
}
