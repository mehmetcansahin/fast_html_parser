//! AVX2 accelerated operations (256-bit, x86_64).
//!
//! Processes 32 bytes at a time — double the throughput of SSE4.2.
//! Falls back to scalar for the tail bytes.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::{DelimiterResult, classify_byte};

/// Scan `haystack` for the first HTML delimiter using AVX2 (256-bit).
///
/// Processes 32 bytes per iteration. Uses `_mm256_cmpeq_epi8` for each
/// delimiter then combines with OR, extracting a 32-bit bitmask via
/// `_mm256_movemask_epi8`.
///
/// # Safety
///
/// Caller must ensure the CPU supports AVX2 (`is_x86_feature_detected!("avx2")`).
#[target_feature(enable = "avx2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn find_delimiters(haystack: &[u8]) -> DelimiterResult {
    let len = haystack.len();
    let ptr = haystack.as_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require AVX2, guaranteed by #[target_feature].
    unsafe {
        // Broadcast each delimiter byte into a 256-bit register.
        let lt = _mm256_set1_epi8(b'<' as i8);
        let gt = _mm256_set1_epi8(b'>' as i8);
        let amp = _mm256_set1_epi8(b'&' as i8);
        let quot = _mm256_set1_epi8(b'"' as i8);
        let apos = _mm256_set1_epi8(b'\'' as i8);
        let eq = _mm256_set1_epi8(b'=' as i8);
        let slash = _mm256_set1_epi8(b'/' as i8);

        while offset + 32 <= len {
            // Load 32 bytes (unaligned).
            let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);

            // Compare 32 bytes against each delimiter simultaneously.
            let cmp_lt = _mm256_cmpeq_epi8(chunk, lt);
            let cmp_gt = _mm256_cmpeq_epi8(chunk, gt);
            let cmp_amp = _mm256_cmpeq_epi8(chunk, amp);
            let cmp_quot = _mm256_cmpeq_epi8(chunk, quot);
            let cmp_apos = _mm256_cmpeq_epi8(chunk, apos);
            let cmp_eq = _mm256_cmpeq_epi8(chunk, eq);
            let cmp_slash = _mm256_cmpeq_epi8(chunk, slash);

            // OR all comparisons together.
            let combined = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_or_si256(cmp_lt, cmp_gt),
                    _mm256_or_si256(cmp_amp, cmp_quot),
                ),
                _mm256_or_si256(_mm256_or_si256(cmp_apos, cmp_eq), cmp_slash),
            );

            // Extract a 32-bit mask: bit i is set if byte i matched any delimiter.
            let mask = _mm256_movemask_epi8(combined) as u32;
            if mask != 0 {
                // trailing_zeros gives the index of the first set bit.
                let pos = offset + mask.trailing_zeros() as usize;
                return DelimiterResult::Found {
                    pos,
                    byte: *ptr.add(pos),
                };
            }
            offset += 32;
        }
    }

    // Scalar tail for remaining < 32 bytes.
    crate::scalar::find_delimiters_safe(&haystack[offset..]).offset_by(offset)
}

/// Classify each byte using AVX2 — 32 bytes at a time.
///
/// Uses the same algorithm as SSE4.2 but with 256-bit registers for
/// double throughput.
///
/// # Safety
///
/// Caller must ensure AVX2 support.
#[target_feature(enable = "avx2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn classify_bytes(input: &[u8]) -> Vec<u8> {
    let len = input.len();
    let mut result = Vec::with_capacity(len);
    let ptr = input.as_ptr();
    let out_ptr: *mut u8 = result.as_mut_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require AVX2, guaranteed by #[target_feature].
    // Pointer arithmetic is valid because offset < len and result has capacity >= len.
    unsafe {
        while offset + 32 <= len {
            let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);

            // Whitespace: space, tab, newline, CR.
            let ws_mask = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b' ' as i8)),
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\t' as i8)),
                ),
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\n' as i8)),
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\r' as i8)),
                ),
            );

            // Alpha: (b | 0x20) - 'a' <= 25 (unsigned)
            let lower = _mm256_or_si256(chunk, _mm256_set1_epi8(0x20));
            let sub = _mm256_sub_epi8(lower, _mm256_set1_epi8(b'a' as i8));
            let clamped = _mm256_min_epu8(sub, _mm256_set1_epi8(25));
            let alpha_mask = _mm256_cmpeq_epi8(sub, clamped);

            // Digit: b - '0' <= 9 (unsigned)
            let sub_d = _mm256_sub_epi8(chunk, _mm256_set1_epi8(b'0' as i8));
            let dclamped = _mm256_min_epu8(sub_d, _mm256_set1_epi8(9));
            let digit_mask = _mm256_cmpeq_epi8(sub_d, dclamped);

            // Delimiters: 7 equality comparisons OR'd together.
            let delim_mask = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_or_si256(
                        _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'<' as i8)),
                        _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'>' as i8)),
                    ),
                    _mm256_or_si256(
                        _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'&' as i8)),
                        _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'"' as i8)),
                    ),
                ),
                _mm256_or_si256(
                    _mm256_or_si256(
                        _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\'' as i8)),
                        _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'=' as i8)),
                    ),
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'/' as i8)),
                ),
            );

            // Map masks to class constants and combine.
            let combined = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_and_si256(ws_mask, _mm256_set1_epi8(crate::class::WHITESPACE as i8)),
                    _mm256_and_si256(alpha_mask, _mm256_set1_epi8(crate::class::ALPHA as i8)),
                ),
                _mm256_or_si256(
                    _mm256_and_si256(digit_mask, _mm256_set1_epi8(crate::class::DIGIT as i8)),
                    _mm256_and_si256(delim_mask, _mm256_set1_epi8(crate::class::DELIMITER as i8)),
                ),
            );

            _mm256_storeu_si256(out_ptr.add(offset) as *mut __m256i, combined);
            offset += 32;
        }

        // Scalar tail.
        while offset < len {
            *out_ptr.add(offset) = classify_byte(*ptr.add(offset));
            offset += 1;
        }

        result.set_len(len);
    }

    result
}

/// Skip leading whitespace using AVX2 — 32 bytes at a time.
///
/// # Safety
///
/// Caller must ensure AVX2 support.
#[target_feature(enable = "avx2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn skip_whitespace(input: &[u8]) -> usize {
    let len = input.len();
    let ptr = input.as_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require AVX2, guaranteed by #[target_feature].
    unsafe {
        while offset + 32 <= len {
            let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);

            // Check all 4 whitespace bytes.
            let ws_mask = _mm256_or_si256(
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b' ' as i8)),
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\t' as i8)),
                ),
                _mm256_or_si256(
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\n' as i8)),
                    _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'\r' as i8)),
                ),
            );

            // movemask: bit i = 1 if byte i is whitespace.
            let mask = _mm256_movemask_epi8(ws_mask) as u32;

            if mask != 0xFFFF_FFFF {
                // Not all whitespace — find first non-WS byte.
                let non_ws = !mask;
                return offset + non_ws.trailing_zeros() as usize;
            }
            offset += 32;
        }
    }

    // Scalar tail.
    offset + crate::scalar::skip_whitespace_safe(&input[offset..])
}

/// Produce a bitmask where bit `i` is set if `block[i] == byte`.
///
/// Processes 32 bytes at a time using `_mm256_cmpeq_epi8` +
/// `_mm256_movemask_epi8`. Handles blocks up to 64 bytes.
///
/// # Safety
///
/// Caller must ensure the CPU supports AVX2.
#[target_feature(enable = "avx2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn compute_byte_mask(block: &[u8], byte: u8) -> u64 {
    // Mask is 64-bit; only the first 64 bytes can be represented.
    let len = block.len().min(64);
    let ptr = block.as_ptr();
    let mut result: u64 = 0;
    let mut offset = 0;

    // SAFETY: all intrinsics below require AVX2, guaranteed by #[target_feature].
    unsafe {
        let target = _mm256_set1_epi8(byte as i8);

        while offset + 32 <= len {
            let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
            let cmp = _mm256_cmpeq_epi8(chunk, target);
            let mask = _mm256_movemask_epi8(cmp) as u32;
            result |= (mask as u64) << offset;
            offset += 32;
        }
    }

    // Scalar tail.
    while offset < len {
        // SAFETY: offset < len, so ptr.add(offset) is valid.
        if unsafe { *ptr.add(offset) } == byte {
            result |= 1u64 << offset;
        }
        offset += 1;
    }

    result
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::class;

    fn has_avx2() -> bool {
        is_x86_feature_detected!("avx2")
    }

    #[test]
    fn find_delimiters_basic() {
        if !has_avx2() {
            return;
        }
        // 35 bytes — crosses the 32-byte boundary.
        let input = b"abcdefghijklmnopqrstuvwxyz12345<div>";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(
            result,
            DelimiterResult::Found {
                pos: 31,
                byte: b'<'
            },
        );
    }

    #[test]
    fn find_delimiters_not_found() {
        if !has_avx2() {
            return;
        }
        let input = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::NotFound);
    }

    #[test]
    fn classify_bytes_basic() {
        if !has_avx2() {
            return;
        }
        // 36 bytes to cross 32-byte boundary.
        let input = b"abcd 1234\t<>&&\"'/=\nABCDwxyz09......";
        let result = unsafe { classify_bytes(input) };
        assert_eq!(result[0], class::ALPHA);
        assert_eq!(result[4], class::WHITESPACE);
        assert_eq!(result[5], class::DIGIT);
        assert_eq!(result[10], class::DELIMITER); // '<'
    }

    #[test]
    fn skip_whitespace_basic() {
        if !has_avx2() {
            return;
        }
        // 40 spaces then a letter — crosses 32-byte boundary.
        let input = b"                                        X";
        let result = unsafe { skip_whitespace(input) };
        assert_eq!(result, 40);
    }
}
