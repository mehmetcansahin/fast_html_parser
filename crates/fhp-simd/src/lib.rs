/// Runtime SIMD feature detection and dispatch.
pub mod dispatch;
/// Portable scalar fallback — works on every platform.
pub mod scalar;

#[cfg(target_arch = "x86_64")]
/// SSE4.2 accelerated operations (128-bit, x86_64).
pub mod sse42;

#[cfg(target_arch = "x86_64")]
/// AVX2 accelerated operations (256-bit, x86_64).
pub mod avx2;

#[cfg(target_arch = "aarch64")]
/// ARM NEON accelerated operations (128-bit, aarch64).
pub mod neon;

/// The set of delimiters scanned by [`find_delimiters`](dispatch::SimdOps).
///
/// These are the bytes that have structural significance in HTML:
/// `<`, `>`, `&`, `"`, `'`, `=`, `/`.
pub const DELIMITERS: [u8; 7] = [b'<', b'>', b'&', b'"', b'\'', b'=', b'/'];

/// Result of a multi-delimiter scan over a byte slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelimiterResult {
    /// A delimiter was found at the given position.
    Found {
        /// Byte offset within the searched slice.
        pos: usize,
        /// The delimiter byte that was matched.
        byte: u8,
    },
    /// No delimiter was found in the slice.
    NotFound,
}

impl DelimiterResult {
    /// Shift the position by `offset`, used when falling back to scalar for
    /// a tail slice.
    #[inline]
    pub fn offset_by(self, offset: usize) -> DelimiterResult {
        match self {
            DelimiterResult::Found { pos, byte } => DelimiterResult::Found {
                pos: pos + offset,
                byte,
            },
            DelimiterResult::NotFound => DelimiterResult::NotFound,
        }
    }
}

/// Byte classification categories produced by [`classify_bytes`](dispatch::SimdOps).
///
/// Each bit represents a category. A byte may belong to multiple categories
/// (e.g. `<` is both `DELIMITER` and `OTHER` is absent).
pub mod class {
    /// ASCII whitespace: space, tab, newline, carriage return.
    pub const WHITESPACE: u8 = 0b0000_0001;
    /// ASCII alphabetic: `a-z`, `A-Z`.
    pub const ALPHA: u8 = 0b0000_0010;
    /// ASCII digit: `0-9`.
    pub const DIGIT: u8 = 0b0000_0100;
    /// HTML structural delimiter: `<`, `>`, `&`, `"`, `'`, `=`, `/`.
    pub const DELIMITER: u8 = 0b0000_1000;
    /// None of the above.
    pub const OTHER: u8 = 0b0000_0000;
}

/// All seven delimiter bitmasks for a block of up to 64 bytes.
///
/// Produced by [`compute_all_masks`](dispatch::SimdOps) which loads each
/// 16-byte chunk only once, computing all masks in a single pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllMasks {
    /// `<` positions.
    pub lt: u64,
    /// `>` positions.
    pub gt: u64,
    /// `&` positions.
    pub amp: u64,
    /// `"` positions.
    pub quot: u64,
    /// `'` positions.
    pub apos: u64,
    /// `=` positions.
    pub eq: u64,
    /// `/` positions.
    pub slash: u64,
}

/// Classify a single byte into one of the [`class`] categories.
#[inline(always)]
pub fn classify_byte(b: u8) -> u8 {
    match b {
        b' ' | b'\t' | b'\n' | b'\r' => class::WHITESPACE,
        b'a'..=b'z' | b'A'..=b'Z' => class::ALPHA,
        b'0'..=b'9' => class::DIGIT,
        b'<' | b'>' | b'&' | b'"' | b'\'' | b'=' | b'/' => class::DELIMITER,
        _ => class::OTHER,
    }
}
