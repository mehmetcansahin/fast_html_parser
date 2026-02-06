//! SSE4.2 accelerated operations (128-bit, x86_64).
//!
//! Each function is marked `#[target_feature(enable = "sse4.2")]` so the
//! compiler generates SSE4.2 instructions without requiring the entire
//! crate to be compiled with `-C target-feature=+sse4.2`.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use crate::{DelimiterResult, classify_byte};

/// Scan `haystack` for the first HTML delimiter using SSE4.2 (128-bit).
///
/// Processes 16 bytes at a time. Falls back to scalar for the tail.
///
/// # Safety
///
/// Caller must ensure the CPU supports SSE4.2 (`is_x86_feature_detected!("sse4.2")`).
#[target_feature(enable = "sse4.2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn find_delimiters(haystack: &[u8]) -> DelimiterResult {
    let len = haystack.len();
    let ptr = haystack.as_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require SSE4.2, guaranteed by #[target_feature].
    unsafe {
        // Load all 7 delimiter bytes into a single 128-bit register.
        // _mm_cmpistri can compare against up to 16 needle bytes.
        let delims = _mm_set_epi8(
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0, // padding (unused slots)
            b'/' as i8,
            b'=' as i8,
            b'\'' as i8,
            b'"' as i8,
            b'&' as i8,
            b'>' as i8,
            b'<' as i8,
        );

        while offset + 16 <= len {
            // Load 16 bytes from haystack (unaligned).
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

            // _mm_cmpistri: compare each byte in `chunk` against the set of
            // bytes in `delims`. Returns the index of the *first* matching
            // byte (0..15), or 16 if no match.
            // Mode 0x00 = SIDD_UBYTE_OPS | SIDD_CMP_EQUAL_ANY | SIDD_LEAST_SIGNIFICANT
            let idx = _mm_cmpistri(delims, chunk, 0x00);
            if idx < 16 {
                let pos = offset + idx as usize;
                return DelimiterResult::Found {
                    pos,
                    byte: *ptr.add(pos),
                };
            }
            offset += 16;
        }
    }

    // Scalar tail for remaining < 16 bytes.
    crate::scalar::find_delimiters_safe(&haystack[offset..]).offset_by(offset)
}

/// Classify each byte using SSE4.2 SIMD — 16 bytes at a time.
///
/// # Safety
///
/// Caller must ensure SSE4.2 support.
#[target_feature(enable = "sse4.2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn classify_bytes(input: &[u8]) -> Vec<u8> {
    let len = input.len();
    let mut result = Vec::with_capacity(len);
    let ptr = input.as_ptr();
    let out_ptr: *mut u8 = result.as_mut_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require SSE4.2, guaranteed by #[target_feature].
    // Pointer arithmetic is valid because offset < len and result has capacity >= len.
    unsafe {
        while offset + 16 <= len {
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

            // Whitespace check: compare against space, tab, newline, CR.
            let space = _mm_set1_epi8(b' ' as i8);
            let tab = _mm_set1_epi8(b'\t' as i8);
            let nl = _mm_set1_epi8(b'\n' as i8);
            let cr = _mm_set1_epi8(b'\r' as i8);

            // _mm_cmpeq_epi8: 0xFF where equal, 0x00 where not.
            let ws_mask = _mm_or_si128(
                _mm_or_si128(_mm_cmpeq_epi8(chunk, space), _mm_cmpeq_epi8(chunk, tab)),
                _mm_or_si128(_mm_cmpeq_epi8(chunk, nl), _mm_cmpeq_epi8(chunk, cr)),
            );

            // Alpha check: (b | 0x20) - 'a' <= 25 (unsigned)
            let or_mask = _mm_set1_epi8(0x20);
            let lower = _mm_or_si128(chunk, or_mask); // force lowercase
            let a_val = _mm_set1_epi8(b'a' as i8);
            let sub = _mm_sub_epi8(lower, a_val); // lower - 'a'
            let bound = _mm_set1_epi8(25); // 'z' - 'a'
            // Unsigned compare: alpha if sub == min(sub, 25)
            let clamped = _mm_min_epu8(sub, bound);
            let alpha_mask = _mm_cmpeq_epi8(sub, clamped);

            // Digit check: b - '0' <= 9 (unsigned)
            let zero = _mm_set1_epi8(b'0' as i8);
            let sub_d = _mm_sub_epi8(chunk, zero);
            let dbound = _mm_set1_epi8(9);
            let dclamped = _mm_min_epu8(sub_d, dbound);
            let digit_mask = _mm_cmpeq_epi8(sub_d, dclamped);

            // Delimiter check: compare against each of the 7 delimiters.
            let lt = _mm_set1_epi8(b'<' as i8);
            let gt = _mm_set1_epi8(b'>' as i8);
            let amp = _mm_set1_epi8(b'&' as i8);
            let quot = _mm_set1_epi8(b'"' as i8);
            let apos = _mm_set1_epi8(b'\'' as i8);
            let eq = _mm_set1_epi8(b'=' as i8);
            let slash = _mm_set1_epi8(b'/' as i8);

            let delim_mask = _mm_or_si128(
                _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, lt), _mm_cmpeq_epi8(chunk, gt)),
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, amp), _mm_cmpeq_epi8(chunk, quot)),
                ),
                _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, apos), _mm_cmpeq_epi8(chunk, eq)),
                    _mm_cmpeq_epi8(chunk, slash),
                ),
            );

            // Map to class constants and combine.
            let ws_class = _mm_and_si128(ws_mask, _mm_set1_epi8(crate::class::WHITESPACE as i8));
            let al_class = _mm_and_si128(alpha_mask, _mm_set1_epi8(crate::class::ALPHA as i8));
            let di_class = _mm_and_si128(digit_mask, _mm_set1_epi8(crate::class::DIGIT as i8));
            let de_class = _mm_and_si128(delim_mask, _mm_set1_epi8(crate::class::DELIMITER as i8));

            let combined = _mm_or_si128(
                _mm_or_si128(ws_class, al_class),
                _mm_or_si128(di_class, de_class),
            );

            // Store 16 bytes of classification results.
            _mm_storeu_si128(out_ptr.add(offset) as *mut __m128i, combined);
            offset += 16;
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

/// Skip leading whitespace using SSE4.2 — 16 bytes at a time.
///
/// # Safety
///
/// Caller must ensure SSE4.2 support.
#[target_feature(enable = "sse4.2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn skip_whitespace(input: &[u8]) -> usize {
    let len = input.len();
    let ptr = input.as_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require SSE4.2, guaranteed by #[target_feature].
    unsafe {
        // Whitespace needle: space, tab, newline, CR.
        let ws = _mm_set_epi8(
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            b'\r' as i8,
            b'\n' as i8,
            b'\t' as i8,
            b' ' as i8,
        );

        while offset + 16 <= len {
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

            // _mm_cmpistri with EQUAL_ANY + NEGATIVE_POLARITY:
            // returns index of first byte NOT in the whitespace set.
            // Mode 0x10 = SIDD_UBYTE_OPS | SIDD_CMP_EQUAL_ANY | SIDD_NEGATIVE_POLARITY
            let idx = _mm_cmpistri(ws, chunk, 0x10);
            if idx < 16 {
                return offset + idx as usize;
            }
            offset += 16;
        }
    }

    // Scalar tail.
    offset + crate::scalar::skip_whitespace_safe(&input[offset..])
}

/// Produce a bitmask where bit `i` is set if `block[i] == byte`.
///
/// Processes 16 bytes at a time using `_mm_cmpeq_epi8` + `_mm_movemask_epi8`.
/// Handles blocks up to 64 bytes.
///
/// # Safety
///
/// Caller must ensure the CPU supports SSE4.2.
#[target_feature(enable = "sse4.2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn compute_byte_mask(block: &[u8], byte: u8) -> u64 {
    let len = block.len();
    let ptr = block.as_ptr();
    let mut result: u64 = 0;
    let mut offset = 0;

    // SAFETY: all intrinsics below require SSE4.2, guaranteed by #[target_feature].
    unsafe {
        let target = _mm_set1_epi8(byte as i8);

        while offset + 16 <= len {
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);
            let cmp = _mm_cmpeq_epi8(chunk, target);
            let mask = _mm_movemask_epi8(cmp) as u16;
            result |= (mask as u64) << offset;
            offset += 16;
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

    fn has_sse42() -> bool {
        is_x86_feature_detected!("sse4.2")
    }

    #[test]
    fn find_delimiters_basic() {
        if !has_sse42() {
            return;
        }
        let input = b"hello world <div>";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(
            result,
            DelimiterResult::Found {
                pos: 12,
                byte: b'<'
            }
        );
    }

    #[test]
    fn find_delimiters_not_found() {
        if !has_sse42() {
            return;
        }
        let input = b"hello world no delimiters here";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::NotFound);
    }

    #[test]
    fn classify_bytes_basic() {
        if !has_sse42() {
            return;
        }
        let input = b"a1 <b2\t>Zz09&\"'/=\nhello world...";
        let result = unsafe { classify_bytes(input) };
        assert_eq!(result[0], class::ALPHA);
        assert_eq!(result[1], class::DIGIT);
        assert_eq!(result[2], class::WHITESPACE);
        assert_eq!(result[3], class::DELIMITER);
    }

    #[test]
    fn skip_whitespace_basic() {
        if !has_sse42() {
            return;
        }
        let result = unsafe { skip_whitespace(b"   \t\nhello") };
        assert_eq!(result, 5);
    }

    #[test]
    fn skip_whitespace_all_ws() {
        if !has_sse42() {
            return;
        }
        let result = unsafe { skip_whitespace(b"                    ") };
        assert_eq!(result, 20);
    }
}
