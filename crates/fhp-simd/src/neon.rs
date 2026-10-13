//! ARM NEON accelerated operations (128-bit, aarch64).
//!
//! NEON is always available on aarch64 — no runtime detection needed.
//! Each function processes 16 bytes at a time, matching SSE4.2 throughput.

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// Compute four delimiter bitmasks in a single pass over the block.
///
/// Loads each 16-byte chunk once and produces all 4 masks simultaneously.
/// Only `<`, `>`, `"`, `'` are needed by the fused tokenizer pipeline.
///
/// # Safety
///
/// Caller must ensure the CPU supports NEON (always true on aarch64).
#[target_feature(enable = "neon")]
#[cfg(target_arch = "aarch64")]
pub unsafe fn compute_all_masks(block: &[u8]) -> crate::AllMasks {
    // Masks are 64-bit; only the first 64 bytes can be represented.
    let len = block.len().min(64);
    let ptr = block.as_ptr();
    let mut masks = crate::AllMasks::default();
    let mut offset = 0;

    // SAFETY: all intrinsics below require NEON, guaranteed by #[target_feature].
    unsafe {
        // Broadcast each delimiter into a 128-bit register.
        let v_lt = vdupq_n_u8(b'<');
        let v_gt = vdupq_n_u8(b'>');
        let v_quot = vdupq_n_u8(b'"');
        let v_apos = vdupq_n_u8(b'\'');

        while offset + 16 <= len {
            // Single load per 16-byte chunk.
            let chunk = vld1q_u8(ptr.add(offset));

            // 4 comparisons on the same loaded chunk.
            let m_lt = neon_movemask(vceqq_u8(chunk, v_lt)) as u64;
            let m_gt = neon_movemask(vceqq_u8(chunk, v_gt)) as u64;
            let m_quot = neon_movemask(vceqq_u8(chunk, v_quot)) as u64;
            let m_apos = neon_movemask(vceqq_u8(chunk, v_apos)) as u64;

            masks.lt |= m_lt << offset;
            masks.gt |= m_gt << offset;
            masks.quot |= m_quot << offset;
            masks.apos |= m_apos << offset;

            offset += 16;
        }
    }

    // Scalar tail for remaining bytes.
    while offset < len {
        let b = block[offset];
        let bit = 1u64 << offset;
        match b {
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
        // Static avoids stack allocation + load-use latency on every call.
        static BIT_MASK: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
        let bitmask = vld1q_u8(BIT_MASK.as_ptr());

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

    #[test]
    fn compute_all_masks_matches_scalar() {
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        let neon_masks = unsafe { compute_all_masks(input) };
        let scalar_masks = crate::scalar::compute_all_masks_safe(input);
        assert_eq!(neon_masks.lt, scalar_masks.lt, "lt mismatch");
        assert_eq!(neon_masks.gt, scalar_masks.gt, "gt mismatch");
        assert_eq!(neon_masks.quot, scalar_masks.quot, "quot mismatch");
        assert_eq!(neon_masks.apos, scalar_masks.apos, "apos mismatch");
    }

    #[test]
    fn compute_all_masks_matches_scalar_for_every_single_byte_block() {
        // SAFETY: NEON is mandatory on aarch64.
        unsafe { crate::differential::assert_single_byte_blocks_match_scalar(compute_all_masks) };
    }

    #[test]
    fn compute_all_masks_matches_scalar_for_delimiter_runs() {
        // SAFETY: NEON is mandatory on aarch64.
        unsafe { crate::differential::assert_delimiter_runs_match_scalar(compute_all_masks) };
    }

    #[test]
    fn compute_all_masks_64_bytes() {
        let mut input = vec![b'x'; 64];
        input[0] = b'<';
        input[15] = b'>';
        input[31] = b'"';
        input[48] = b'\'';
        let masks = unsafe { compute_all_masks(&input) };
        assert_eq!(masks.lt, 1u64 << 0);
        assert_eq!(masks.gt, 1u64 << 15);
        assert_eq!(masks.quot, 1u64 << 31);
        assert_eq!(masks.apos, 1u64 << 48);
    }
}
