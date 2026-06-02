//! Portable scalar fallback for all SIMD operations.
//!
//! Every function here processes bytes one at a time. It is the baseline
//! that SIMD backends must match semantically (and beat in throughput).

use crate::{DelimiterResult, classify_byte};

/// Scan `haystack` for the first occurrence of any HTML delimiter.
///
/// Delimiters: `<`, `>`, `&`, `"`, `'`, `=`, `/`.
///
/// # Safety
///
/// This function is safe. The `unsafe fn` signature exists so that it
/// can be stored in the same function-pointer slot as the SIMD variants
/// (which require `unsafe` due to target-feature intrinsics).
pub unsafe fn find_delimiters(haystack: &[u8]) -> DelimiterResult {
    find_delimiters_safe(haystack)
}

/// Safe inner implementation of [`find_delimiters`].
#[inline]
pub fn find_delimiters_safe(haystack: &[u8]) -> DelimiterResult {
    for (i, &b) in haystack.iter().enumerate() {
        if is_delimiter(b) {
            return DelimiterResult::Found { pos: i, byte: b };
        }
    }
    DelimiterResult::NotFound
}

/// Classify each byte in `input` into a category bitmask.
///
/// Returns a `Vec<u8>` of the same length as `input`, where each element
/// is one of the constants from [`crate::class`].
///
/// # Safety
///
/// This function is safe. The `unsafe fn` signature matches the SIMD
/// dispatch slot.
pub unsafe fn classify_bytes(input: &[u8]) -> Vec<u8> {
    classify_bytes_safe(input)
}

/// Safe inner implementation of [`classify_bytes`].
#[inline]
pub fn classify_bytes_safe(input: &[u8]) -> Vec<u8> {
    input.iter().map(|&b| classify_byte(b)).collect()
}

/// Skip leading whitespace bytes and return the byte offset of the first
/// non-whitespace byte (or `input.len()` if the entire slice is whitespace).
///
/// # Safety
///
/// This function is safe. The `unsafe fn` signature matches the SIMD
/// dispatch slot.
pub unsafe fn skip_whitespace(input: &[u8]) -> usize {
    skip_whitespace_safe(input)
}

/// Safe inner implementation of [`skip_whitespace`].
#[inline]
pub fn skip_whitespace_safe(input: &[u8]) -> usize {
    input
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(input.len())
}

/// Produce a bitmask where bit `i` is set if `block[i] == byte`.
///
/// Processes up to 64 bytes. Bits beyond `block.len()` are always 0.
///
/// # Safety
///
/// This function is safe. The `unsafe fn` signature matches the SIMD
/// dispatch slot.
pub unsafe fn compute_byte_mask(block: &[u8], byte: u8) -> u64 {
    compute_byte_mask_safe(block, byte)
}

/// Safe inner implementation of [`compute_byte_mask`].
///
/// The result is a 64-bit mask, so at most the first 64 bytes of `block` are
/// considered; any bytes beyond offset 64 are ignored.
#[inline]
pub fn compute_byte_mask_safe(block: &[u8], byte: u8) -> u64 {
    let mut mask = 0u64;
    for (i, &b) in block.iter().take(64).enumerate() {
        if b == byte {
            mask |= 1u64 << i;
        }
    }
    mask
}

/// Compute all seven delimiter bitmasks in a single pass over the block.
///
/// # Safety
///
/// This function is safe. The `unsafe fn` signature matches the SIMD
/// dispatch slot.
pub unsafe fn compute_all_masks(block: &[u8]) -> crate::AllMasks {
    compute_all_masks_safe(block)
}

/// Safe inner implementation of [`compute_all_masks`].
///
/// The masks are 64-bit, so at most the first 64 bytes of `block` are
/// considered; any bytes beyond offset 64 are ignored.
#[inline]
pub fn compute_all_masks_safe(block: &[u8]) -> crate::AllMasks {
    let mut masks = crate::AllMasks::default();
    for (i, &b) in block.iter().take(64).enumerate() {
        let bit = 1u64 << i;
        match b {
            b'<' => masks.lt |= bit,
            b'>' => masks.gt |= bit,
            b'"' => masks.quot |= bit,
            b'\'' => masks.apos |= bit,
            _ => {}
        }
    }
    masks
}

/// Returns `true` if `b` is one of the 7 HTML delimiters.
#[inline(always)]
fn is_delimiter(b: u8) -> bool {
    matches!(b, b'<' | b'>' | b'&' | b'"' | b'\'' | b'=' | b'/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class;

    #[test]
    fn find_delimiters_lt() {
        let input = b"hello <world>";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::Found { pos: 6, byte: b'<' });
    }

    #[test]
    fn find_delimiters_amp() {
        let input = b"a &amp; b";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::Found { pos: 2, byte: b'&' });
    }

    #[test]
    fn find_delimiters_none() {
        let input = b"hello world";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::NotFound);
    }

    #[test]
    fn find_delimiters_empty() {
        let result = unsafe { find_delimiters(b"") };
        assert_eq!(result, DelimiterResult::NotFound);
    }

    #[test]
    fn find_delimiters_first_byte() {
        let result = unsafe { find_delimiters(b"<html>") };
        assert_eq!(result, DelimiterResult::Found { pos: 0, byte: b'<' });
    }

    #[test]
    fn find_delimiters_all_types() {
        for &delim in b"<>&\"'=/" {
            let input = [b'x', b'x', delim, b'x'];
            let result = unsafe { find_delimiters(&input) };
            assert_eq!(
                result,
                DelimiterResult::Found {
                    pos: 2,
                    byte: delim
                },
                "failed for delimiter 0x{delim:02X}"
            );
        }
    }

    #[test]
    fn classify_bytes_mixed() {
        let input = b"a1 <";
        let result = unsafe { classify_bytes(input) };
        assert_eq!(result[0], class::ALPHA); // 'a'
        assert_eq!(result[1], class::DIGIT); // '1'
        assert_eq!(result[2], class::WHITESPACE); // ' '
        assert_eq!(result[3], class::DELIMITER); // '<'
    }

    #[test]
    fn classify_bytes_empty() {
        let result = unsafe { classify_bytes(b"") };
        assert!(result.is_empty());
    }

    #[test]
    fn skip_whitespace_leading() {
        let result = unsafe { skip_whitespace(b"   hello") };
        assert_eq!(result, 3);
    }

    #[test]
    fn skip_whitespace_mixed() {
        let result = unsafe { skip_whitespace(b" \t\n\rX") };
        assert_eq!(result, 4);
    }

    #[test]
    fn skip_whitespace_all() {
        let result = unsafe { skip_whitespace(b"   ") };
        assert_eq!(result, 3);
    }

    #[test]
    fn skip_whitespace_none() {
        let result = unsafe { skip_whitespace(b"hello") };
        assert_eq!(result, 0);
    }

    #[test]
    fn skip_whitespace_empty() {
        let result = unsafe { skip_whitespace(b"") };
        assert_eq!(result, 0);
    }

    #[test]
    fn compute_byte_mask_basic() {
        let input = b"hello <world>";
        let mask = unsafe { compute_byte_mask(input, b'<') };
        assert_eq!(mask, 1 << 6);
    }

    #[test]
    fn compute_byte_mask_multiple() {
        let input = b"a<b<c";
        let mask = unsafe { compute_byte_mask(input, b'<') };
        assert_eq!(mask, (1 << 1) | (1 << 3));
    }

    #[test]
    fn compute_byte_mask_none() {
        let input = b"hello world";
        let mask = unsafe { compute_byte_mask(input, b'<') };
        assert_eq!(mask, 0);
    }

    #[test]
    fn compute_byte_mask_empty() {
        let mask = unsafe { compute_byte_mask(b"", b'<') };
        assert_eq!(mask, 0);
    }

    #[test]
    fn compute_byte_mask_over_64_bytes_does_not_overflow() {
        // A u64 mask can only represent 64 positions. An over-long block must
        // be capped at 64 rather than overflowing the shift (UB/panic).
        let input = vec![b'<'; 80];
        let mask = compute_byte_mask_safe(&input, b'<');
        assert_eq!(mask, u64::MAX, "first 64 '<' set, rest ignored");
    }

    #[test]
    fn compute_all_masks_over_64_bytes_does_not_overflow() {
        let input = vec![b'<'; 80];
        let masks = compute_all_masks_safe(&input);
        assert_eq!(masks.lt, u64::MAX);
    }

    #[test]
    fn compute_all_masks_basic() {
        let input = b"<div class=\"foo\">";
        let masks = unsafe { compute_all_masks(input) };
        assert_eq!(masks.lt, 1 << 0); // '<' at 0
        assert_eq!(masks.gt, 1 << 16); // '>' at 16
        assert_eq!(masks.quot, (1 << 11) | (1 << 15)); // '"' at 11 and 15
    }

    #[test]
    fn compute_all_masks_empty() {
        let masks = unsafe { compute_all_masks(b"") };
        assert_eq!(masks.lt, 0);
        assert_eq!(masks.gt, 0);
    }

    #[test]
    fn compute_all_masks_matches_individual() {
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        let masks = unsafe { compute_all_masks(input) };
        assert_eq!(masks.lt, unsafe { compute_byte_mask(input, b'<') });
        assert_eq!(masks.gt, unsafe { compute_byte_mask(input, b'>') });
        assert_eq!(masks.quot, unsafe { compute_byte_mask(input, b'"') });
        assert_eq!(masks.apos, unsafe { compute_byte_mask(input, b'\'') });
    }
}
