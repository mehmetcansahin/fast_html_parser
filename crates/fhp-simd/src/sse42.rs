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
        // Splat each of the 7 delimiter bytes into its own register. We use
        // explicit-length equality compares (`_mm_cmpeq_epi8`) rather than the
        // implicit-length string intrinsic `_mm_cmpistri`: the latter treats a
        // NUL byte as a string terminator and would miss any delimiter at or
        // after a `\0`, diverging from the scalar and NEON backends (a `&str`
        // may legally contain NUL bytes).
        let lt = _mm_set1_epi8(b'<' as i8);
        let gt = _mm_set1_epi8(b'>' as i8);
        let amp = _mm_set1_epi8(b'&' as i8);
        let quot = _mm_set1_epi8(b'"' as i8);
        let apos = _mm_set1_epi8(b'\'' as i8);
        let eq = _mm_set1_epi8(b'=' as i8);
        let slash = _mm_set1_epi8(b'/' as i8);

        while offset + 16 <= len {
            // Load 16 bytes from haystack (unaligned).
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

            // OR the per-delimiter equality masks together: 0xFF in any lane
            // that matches one of the seven delimiters.
            let m = _mm_or_si128(
                _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, lt), _mm_cmpeq_epi8(chunk, gt)),
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, amp), _mm_cmpeq_epi8(chunk, quot)),
                ),
                _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, apos), _mm_cmpeq_epi8(chunk, eq)),
                    _mm_cmpeq_epi8(chunk, slash),
                ),
            );
            let mask = _mm_movemask_epi8(m) as u16;
            if mask != 0 {
                let pos = offset + mask.trailing_zeros() as usize;
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

            // HTML whitespace: space, tab, newline, form feed, CR.
            let space = _mm_set1_epi8(b' ' as i8);
            let tab = _mm_set1_epi8(b'\t' as i8);
            let nl = _mm_set1_epi8(b'\n' as i8);
            let ff = _mm_set1_epi8(b'\x0C' as i8);
            let cr = _mm_set1_epi8(b'\r' as i8);

            // _mm_cmpeq_epi8: 0xFF where equal, 0x00 where not.
            let ws_mask = _mm_or_si128(
                _mm_or_si128(_mm_cmpeq_epi8(chunk, space), _mm_cmpeq_epi8(chunk, tab)),
                _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, nl), _mm_cmpeq_epi8(chunk, ff)),
                    _mm_cmpeq_epi8(chunk, cr),
                ),
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
        // HTML whitespace: space, tab, newline, form feed, CR. As in
        // `find_delimiters`,
        // we use explicit-length equality compares instead of `_mm_cmpistri`,
        // whose NUL-termination semantics would treat a `\0` (a non-whitespace
        // byte) as still inside the run and over-skip past it.
        let space = _mm_set1_epi8(b' ' as i8);
        let tab = _mm_set1_epi8(b'\t' as i8);
        let nl = _mm_set1_epi8(b'\n' as i8);
        let ff = _mm_set1_epi8(b'\x0C' as i8);
        let cr = _mm_set1_epi8(b'\r' as i8);

        while offset + 16 <= len {
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

            // 0xFF in lanes that ARE whitespace.
            let ws = _mm_or_si128(
                _mm_or_si128(_mm_cmpeq_epi8(chunk, space), _mm_cmpeq_epi8(chunk, tab)),
                _mm_or_si128(
                    _mm_or_si128(_mm_cmpeq_epi8(chunk, nl), _mm_cmpeq_epi8(chunk, ff)),
                    _mm_cmpeq_epi8(chunk, cr),
                ),
            );
            let mask = _mm_movemask_epi8(ws) as u16;
            // First non-whitespace byte = first zero bit in the mask.
            if mask != 0xFFFF {
                return offset + (!mask).trailing_zeros() as usize;
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
    // Mask is 64-bit; only the first 64 bytes can be represented.
    let len = block.len().min(64);
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

/// Compute four delimiter bitmasks in a single pass over the block.
///
/// Loads each 16-byte chunk once and produces all 4 masks simultaneously.
/// Only `<`, `>`, `"`, `'` are needed by the fused tokenizer pipeline.
///
/// # Safety
///
/// Caller must ensure the CPU supports SSE4.2.
#[target_feature(enable = "sse4.2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn compute_all_masks(block: &[u8]) -> crate::AllMasks {
    // Masks are 64-bit; only the first 64 bytes can be represented.
    let len = block.len().min(64);
    let ptr = block.as_ptr();
    let mut masks = crate::AllMasks::default();
    let mut offset = 0;

    // SAFETY: all intrinsics below require SSE4.2, guaranteed by #[target_feature].
    unsafe {
        let lt = _mm_set1_epi8(b'<' as i8);
        let gt = _mm_set1_epi8(b'>' as i8);
        let quot = _mm_set1_epi8(b'"' as i8);
        let apos = _mm_set1_epi8(b'\'' as i8);

        while offset + 16 <= len {
            let chunk = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

            let m_lt = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, lt)) as u64;
            let m_gt = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, gt)) as u64;
            let m_quot = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, quot)) as u64;
            let m_apos = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, apos)) as u64;

            masks.lt |= m_lt << offset;
            masks.gt |= m_gt << offset;
            masks.quot |= m_quot << offset;
            masks.apos |= m_apos << offset;

            offset += 16;
        }
    }

    // Scalar tail.
    while offset < len {
        let bit = 1u64 << offset;
        match block[offset] {
            b'<' => masks.lt |= bit,
            b'>' => masks.gt |= bit,
            b'"' => masks.quot |= bit,
            b'\'' => masks.apos |= bit,
            _ => {}
        }
        offset += 1;
    }

    masks
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

    #[test]
    fn skip_whitespace_matches_scalar_at_every_short_length() {
        if !has_sse42() {
            return;
        }
        const HTML_WHITESPACE: &[u8] = b" \t\n\r\x0C";

        for len in 0..=128 {
            let mut input = Vec::with_capacity(len + 1);
            input.extend((0..len).map(|index| HTML_WHITESPACE[index % HTML_WHITESPACE.len()]));
            input.push(b'X');

            assert_eq!(
                unsafe { skip_whitespace(&input) },
                crate::scalar::skip_whitespace_safe(&input),
                "mismatch at whitespace length {len}"
            );
        }
    }

    #[test]
    fn compute_all_masks_matches_scalar() {
        if !has_sse42() {
            return;
        }
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        let sse_masks = unsafe { compute_all_masks(input) };
        let scalar_masks = crate::scalar::compute_all_masks_safe(input);
        assert_eq!(sse_masks.lt, scalar_masks.lt, "lt mismatch");
        assert_eq!(sse_masks.gt, scalar_masks.gt, "gt mismatch");
        assert_eq!(sse_masks.quot, scalar_masks.quot, "quot mismatch");
        assert_eq!(sse_masks.apos, scalar_masks.apos, "apos mismatch");
    }

    #[test]
    fn compute_all_masks_64_byte_boundaries() {
        if !has_sse42() {
            return;
        }
        let mut input = vec![b'x'; 64];
        input[0] = b'<';
        input[15] = b'>';
        input[16] = b'"';
        input[63] = b'\'';
        let masks = unsafe { compute_all_masks(&input) };
        assert_eq!(masks.lt, 1u64 << 0);
        assert_eq!(masks.gt, 1u64 << 15);
        assert_eq!(masks.quot, 1u64 << 16);
        assert_eq!(masks.apos, 1u64 << 63);
    }

    #[test]
    fn compute_all_masks_ignores_bytes_after_64() {
        if !has_sse42() {
            return;
        }
        let mut input = vec![b'x'; 80];
        input[64] = b'<';
        input[65] = b'>';
        input[66] = b'"';
        input[67] = b'\'';
        let masks = unsafe { compute_all_masks(&input) };
        assert_eq!(masks.lt, 0);
        assert_eq!(masks.gt, 0);
        assert_eq!(masks.quot, 0);
        assert_eq!(masks.apos, 0);
    }

    #[test]
    fn find_delimiters_after_nul_in_simd_block() {
        // A NUL byte before a delimiter, both within the first 16-byte SIMD
        // block. The implicit-length string intrinsic would treat the NUL as a
        // terminator and miss the `<`; equality compares do not.
        if !has_sse42() {
            return;
        }
        let input = b"abc\0def<ghijklmn"; // 16 bytes, '<' at index 7
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::Found { pos: 7, byte: b'<' });
    }

    #[test]
    fn skip_whitespace_stops_at_nul() {
        // A NUL is not whitespace, so it must stop the skip, not be skipped.
        if !has_sse42() {
            return;
        }
        let input = b"   \0Xhelloworld!"; // 16 bytes, first non-ws (NUL) at 3
        let result = unsafe { skip_whitespace(input) };
        assert_eq!(result, 3);
    }
}
