//! ARM NEON accelerated operations (128-bit, aarch64).
//!
//! NEON is always available on aarch64 — no runtime detection needed.
//! Each function processes 16 bytes at a time, matching SSE4.2 throughput.

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

use crate::{DelimiterResult, classify_byte};

/// Scan `haystack` for the first HTML delimiter using NEON (128-bit).
///
/// Processes 16 bytes at a time using `vceqq_u8` for each delimiter,
/// then OR's the results and extracts a bitmask.
///
/// # Safety
///
/// Caller must ensure the CPU supports NEON (always true on aarch64).
#[target_feature(enable = "neon")]
#[cfg(target_arch = "aarch64")]
pub unsafe fn find_delimiters(haystack: &[u8]) -> DelimiterResult {
    let len = haystack.len();
    let ptr = haystack.as_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require NEON, guaranteed by #[target_feature].
    unsafe {
        // Broadcast each delimiter into a 128-bit register (16 copies).
        let lt = vdupq_n_u8(b'<');
        let gt = vdupq_n_u8(b'>');
        let amp = vdupq_n_u8(b'&');
        let quot = vdupq_n_u8(b'"');
        let apos = vdupq_n_u8(b'\'');
        let eq = vdupq_n_u8(b'=');
        let slash = vdupq_n_u8(b'/');

        while offset + 16 <= len {
            // Load 16 bytes (unaligned load is free on aarch64).
            let chunk = vld1q_u8(ptr.add(offset));

            // vceqq_u8: 0xFF where equal, 0x00 where not.
            let cmp_lt = vceqq_u8(chunk, lt);
            let cmp_gt = vceqq_u8(chunk, gt);
            let cmp_amp = vceqq_u8(chunk, amp);
            let cmp_quot = vceqq_u8(chunk, quot);
            let cmp_apos = vceqq_u8(chunk, apos);
            let cmp_eq = vceqq_u8(chunk, eq);
            let cmp_slash = vceqq_u8(chunk, slash);

            // OR all comparisons.
            let combined = vorrq_u8(
                vorrq_u8(vorrq_u8(cmp_lt, cmp_gt), vorrq_u8(cmp_amp, cmp_quot)),
                vorrq_u8(vorrq_u8(cmp_apos, cmp_eq), cmp_slash),
            );

            // Extract bitmask: NEON doesn't have movemask, use pairwise reduction.
            let mask = neon_movemask(combined);
            if mask != 0 {
                let bit_pos = mask.trailing_zeros() as usize;
                let pos = offset + bit_pos;
                return DelimiterResult::Found {
                    pos,
                    byte: *ptr.add(pos),
                };
            }
            offset += 16;
        }
    }

    // Scalar tail.
    crate::scalar::find_delimiters_safe(&haystack[offset..]).offset_by(offset)
}

/// Classify each byte using NEON — 16 bytes at a time.
///
/// # Safety
///
/// Caller must ensure NEON support (always true on aarch64).
#[target_feature(enable = "neon")]
#[cfg(target_arch = "aarch64")]
pub unsafe fn classify_bytes(input: &[u8]) -> Vec<u8> {
    let len = input.len();
    let mut result = Vec::with_capacity(len);
    let ptr = input.as_ptr();
    let out_ptr: *mut u8 = result.as_mut_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require NEON, guaranteed by #[target_feature].
    // Pointer arithmetic is valid because offset < len and result has capacity >= len.
    unsafe {
        while offset + 16 <= len {
            let chunk = vld1q_u8(ptr.add(offset));

            // Whitespace: space, tab, newline, CR.
            let ws_mask = vorrq_u8(
                vorrq_u8(
                    vceqq_u8(chunk, vdupq_n_u8(b' ')),
                    vceqq_u8(chunk, vdupq_n_u8(b'\t')),
                ),
                vorrq_u8(
                    vceqq_u8(chunk, vdupq_n_u8(b'\n')),
                    vceqq_u8(chunk, vdupq_n_u8(b'\r')),
                ),
            );

            // Alpha: (b | 0x20) - 'a' <= 25
            let lower = vorrq_u8(chunk, vdupq_n_u8(0x20));
            let sub = vsubq_u8(lower, vdupq_n_u8(b'a'));
            // vcleq_u8: 0xFF where sub[i] <= 25
            let alpha_mask = vcleq_u8(sub, vdupq_n_u8(25));

            // Digit: b - '0' <= 9
            let sub_d = vsubq_u8(chunk, vdupq_n_u8(b'0'));
            let digit_mask = vcleq_u8(sub_d, vdupq_n_u8(9));

            // Delimiters.
            let delim_mask = vorrq_u8(
                vorrq_u8(
                    vorrq_u8(
                        vceqq_u8(chunk, vdupq_n_u8(b'<')),
                        vceqq_u8(chunk, vdupq_n_u8(b'>')),
                    ),
                    vorrq_u8(
                        vceqq_u8(chunk, vdupq_n_u8(b'&')),
                        vceqq_u8(chunk, vdupq_n_u8(b'"')),
                    ),
                ),
                vorrq_u8(
                    vorrq_u8(
                        vceqq_u8(chunk, vdupq_n_u8(b'\'')),
                        vceqq_u8(chunk, vdupq_n_u8(b'=')),
                    ),
                    vceqq_u8(chunk, vdupq_n_u8(b'/')),
                ),
            );

            // Map to class constants: AND mask with the class value, then OR all.
            let ws_class = vandq_u8(ws_mask, vdupq_n_u8(crate::class::WHITESPACE));
            let al_class = vandq_u8(alpha_mask, vdupq_n_u8(crate::class::ALPHA));
            let di_class = vandq_u8(digit_mask, vdupq_n_u8(crate::class::DIGIT));
            let de_class = vandq_u8(delim_mask, vdupq_n_u8(crate::class::DELIMITER));

            let combined = vorrq_u8(vorrq_u8(ws_class, al_class), vorrq_u8(di_class, de_class));

            // Store 16 classified bytes.
            vst1q_u8(out_ptr.add(offset), combined);
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

/// Skip leading whitespace using NEON — 16 bytes at a time.
///
/// # Safety
///
/// Caller must ensure NEON support (always true on aarch64).
#[target_feature(enable = "neon")]
#[cfg(target_arch = "aarch64")]
pub unsafe fn skip_whitespace(input: &[u8]) -> usize {
    let len = input.len();
    let ptr = input.as_ptr();
    let mut offset = 0;

    // SAFETY: all intrinsics below require NEON, guaranteed by #[target_feature].
    unsafe {
        while offset + 16 <= len {
            let chunk = vld1q_u8(ptr.add(offset));

            let ws_mask = vorrq_u8(
                vorrq_u8(
                    vceqq_u8(chunk, vdupq_n_u8(b' ')),
                    vceqq_u8(chunk, vdupq_n_u8(b'\t')),
                ),
                vorrq_u8(
                    vceqq_u8(chunk, vdupq_n_u8(b'\n')),
                    vceqq_u8(chunk, vdupq_n_u8(b'\r')),
                ),
            );

            let mask = neon_movemask(ws_mask);
            if mask != 0xFFFF {
                // Not all whitespace — find first non-WS.
                let non_ws = !mask;
                return offset + non_ws.trailing_zeros() as usize;
            }
            offset += 16;
        }
    }

    // Scalar tail.
    offset + crate::scalar::skip_whitespace_safe(&input[offset..])
}

/// Produce a bitmask where bit `i` is set if `block[i] == byte`.
///
/// Processes 16 bytes at a time using NEON `vceqq_u8`, then extracts
/// a bitmask via [`neon_movemask`]. Handles blocks up to 64 bytes.
///
/// # Safety
///
/// Caller must ensure the CPU supports NEON (always true on aarch64).
#[target_feature(enable = "neon")]
#[cfg(target_arch = "aarch64")]
pub unsafe fn compute_byte_mask(block: &[u8], byte: u8) -> u64 {
    let len = block.len();
    let ptr = block.as_ptr();
    let mut result: u64 = 0;
    let mut offset = 0;

    // SAFETY: all intrinsics below require NEON, guaranteed by #[target_feature].
    unsafe {
        let target = vdupq_n_u8(byte);

        while offset + 16 <= len {
            let chunk = vld1q_u8(ptr.add(offset));
            let cmp = vceqq_u8(chunk, target);
            let mask = neon_movemask(cmp);
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

/// Emulate x86 `_mm_movemask_epi8` on NEON.
///
/// Takes a 128-bit vector where each byte is either 0x00 or 0xFF.
/// Returns a `u16` where bit `i` corresponds to byte `i`'s high bit.
///
/// # Safety
///
/// Requires NEON support.
#[target_feature(enable = "neon")]
#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn neon_movemask(v: uint8x16_t) -> u16 {
    // SAFETY: all intrinsics below require NEON, guaranteed by #[target_feature].
    unsafe {
        // Bit-select table: each byte contributes its corresponding bit.
        // AND with power-of-2 per lane, then pairwise-add reduce to form bitmask.
        let bit_mask: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
        let bitmask = vld1q_u8(bit_mask.as_ptr());

        // AND: each lane is either 0 or its bit value.
        let masked = vandq_u8(v, bitmask);

        // Split into low and high 64-bit halves.
        let lo = vget_low_u8(masked);
        let hi = vget_high_u8(masked);

        // Horizontal add: 8 bytes -> 4 u16 -> 2 u32 -> 1 u64 per half.
        let lo_pairs = vpaddl_u8(lo);
        let lo_quads = vpaddl_u16(lo_pairs);
        let lo_single = vpaddl_u32(lo_quads);
        let lo_byte = vget_lane_u64(lo_single, 0) as u8;

        let hi_pairs = vpaddl_u8(hi);
        let hi_quads = vpaddl_u16(hi_pairs);
        let hi_single = vpaddl_u32(hi_quads);
        let hi_byte = vget_lane_u64(hi_single, 0) as u8;

        (lo_byte as u16) | ((hi_byte as u16) << 8)
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::*;
    use crate::class;

    #[test]
    fn find_delimiters_basic() {
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
        let input = b"hello world no delimiters here at all okay";
        let result = unsafe { find_delimiters(input) };
        assert_eq!(result, DelimiterResult::NotFound);
    }

    #[test]
    fn find_delimiters_all_types() {
        for &delim in &[b'<', b'>', b'&', b'"', b'\'', b'=', b'/'] {
            let mut input = vec![b'x'; 20];
            input[15] = delim;
            let result = unsafe { find_delimiters(&input) };
            assert_eq!(
                result,
                DelimiterResult::Found {
                    pos: 15,
                    byte: delim
                },
                "failed for delimiter 0x{delim:02X}"
            );
        }
    }

    #[test]
    fn find_delimiters_in_tail() {
        let mut input = vec![b'x'; 25];
        input[20] = b'<';
        let result = unsafe { find_delimiters(&input) };
        assert_eq!(
            result,
            DelimiterResult::Found {
                pos: 20,
                byte: b'<'
            }
        );
    }

    #[test]
    fn find_delimiters_empty() {
        let result = unsafe { find_delimiters(b"") };
        assert_eq!(result, DelimiterResult::NotFound);
    }

    #[test]
    fn classify_bytes_basic() {
        let input = b"a1 <b2\t>Zz09&\"'/=\nhello world...";
        let result = unsafe { classify_bytes(input) };
        assert_eq!(result[0], class::ALPHA); // 'a'
        assert_eq!(result[1], class::DIGIT); // '1'
        assert_eq!(result[2], class::WHITESPACE); // ' '
        assert_eq!(result[3], class::DELIMITER); // '<'
        assert_eq!(result[4], class::ALPHA); // 'b'
        assert_eq!(result[5], class::DIGIT); // '2'
        assert_eq!(result[6], class::WHITESPACE); // '\t'
        assert_eq!(result[7], class::DELIMITER); // '>'
    }

    #[test]
    fn classify_bytes_matches_scalar() {
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end";
        let neon_result = unsafe { classify_bytes(input) };
        let scalar_result = unsafe { crate::scalar::classify_bytes(input) };
        assert_eq!(neon_result, scalar_result);
    }

    #[test]
    fn classify_bytes_empty() {
        let result = unsafe { classify_bytes(b"") };
        assert!(result.is_empty());
    }

    #[test]
    fn skip_whitespace_basic() {
        let result = unsafe { skip_whitespace(b"   \t\nhello") };
        assert_eq!(result, 5);
    }

    #[test]
    fn skip_whitespace_all_ws() {
        let result = unsafe { skip_whitespace(b"                    ") };
        assert_eq!(result, 20);
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
    fn skip_whitespace_matches_scalar() {
        let inputs: &[&[u8]] = &[
            b"   hello",
            b"\t\n\r world",
            b"no_leading_ws",
            b"                                extra",
            b"",
            b"    ",
        ];
        for &input in inputs {
            let neon_result = unsafe { skip_whitespace(input) };
            let scalar_result = unsafe { crate::scalar::skip_whitespace(input) };
            assert_eq!(
                neon_result,
                scalar_result,
                "mismatch for input {:?}",
                std::str::from_utf8(input)
            );
        }
    }

    #[test]
    fn compute_byte_mask_basic() {
        let input = b"hello world <div>";
        let mask = unsafe { compute_byte_mask(input, b'<') };
        assert_eq!(mask, 1 << 12);
    }

    #[test]
    fn compute_byte_mask_multiple_hits() {
        // 20 bytes — crosses 16-byte boundary.
        let mut input = vec![b'x'; 20];
        input[3] = b'<';
        input[17] = b'<';
        let mask = unsafe { compute_byte_mask(&input, b'<') };
        assert_eq!(mask, (1 << 3) | (1 << 17));
    }

    #[test]
    fn compute_byte_mask_matches_scalar() {
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        for &byte in &[b'<', b'>', b'&', b'"', b'\'', b'=', b'/'] {
            let neon_result = unsafe { compute_byte_mask(input, byte) };
            let scalar_result = unsafe { crate::scalar::compute_byte_mask(input, byte) };
            assert_eq!(neon_result, scalar_result, "mismatch for byte 0x{byte:02X}");
        }
    }

    #[test]
    fn compute_byte_mask_64_bytes() {
        let mut input = vec![b'a'; 64];
        input[0] = b'<';
        input[15] = b'<';
        input[16] = b'<';
        input[31] = b'<';
        input[48] = b'<';
        input[63] = b'<';
        let mask = unsafe { compute_byte_mask(&input, b'<') };
        assert_eq!(
            mask,
            (1u64 << 0) | (1u64 << 15) | (1u64 << 16) | (1u64 << 31) | (1u64 << 48) | (1u64 << 63)
        );
    }

    #[test]
    fn neon_movemask_all_zero() {
        unsafe {
            let v = vdupq_n_u8(0);
            assert_eq!(neon_movemask(v), 0);
        }
    }

    #[test]
    fn neon_movemask_all_ones() {
        unsafe {
            let v = vdupq_n_u8(0xFF);
            assert_eq!(neon_movemask(v), 0xFFFF);
        }
    }

    #[test]
    fn neon_movemask_specific_bits() {
        unsafe {
            let mut bytes = [0u8; 16];
            bytes[0] = 0xFF;
            bytes[8] = 0xFF;
            let v = vld1q_u8(bytes.as_ptr());
            let mask = neon_movemask(v);
            assert_eq!(mask, (1 << 0) | (1 << 8));
        }
    }
}
