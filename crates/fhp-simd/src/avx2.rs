//! AVX2 accelerated operations (256-bit, x86_64).
//!
//! Processes 32 bytes at a time — double the throughput of SSE4.2.
//! Falls back to scalar for the tail bytes.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

/// Compute four delimiter bitmasks in a single pass over the block.
///
/// Loads each 32-byte chunk once and produces all 4 masks simultaneously.
/// Only `<`, `>`, `"`, `'` are needed by the fused tokenizer pipeline.
///
/// # Safety
///
/// Caller must ensure the CPU supports AVX2.
#[target_feature(enable = "avx2")]
#[cfg(target_arch = "x86_64")]
pub unsafe fn compute_all_masks(block: &[u8]) -> crate::AllMasks {
    // Masks are 64-bit; only the first 64 bytes can be represented.
    let len = block.len().min(64);
    let ptr = block.as_ptr();
    let mut masks = crate::AllMasks::default();
    let mut offset = 0;

    // SAFETY: all intrinsics below require AVX2, guaranteed by #[target_feature].
    unsafe {
        let lt = _mm256_set1_epi8(b'<' as i8);
        let gt = _mm256_set1_epi8(b'>' as i8);
        let quot = _mm256_set1_epi8(b'"' as i8);
        let apos = _mm256_set1_epi8(b'\'' as i8);

        while offset + 32 <= len {
            let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);

            // `_mm256_movemask_epi8` returns an `i32`; byte 31 lands in the
            // sign bit, so widen through `u32` to avoid sign extension.
            let m_lt = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, lt)) as u32 as u64;
            let m_gt = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, gt)) as u32 as u64;
            let m_quot = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, quot)) as u32 as u64;
            let m_apos = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, apos)) as u32 as u64;

            masks.lt |= m_lt << offset;
            masks.gt |= m_gt << offset;
            masks.quot |= m_quot << offset;
            masks.apos |= m_apos << offset;

            offset += 32;
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

    /// Whether the AVX2 tests should run. Rosetta 2 executes AVX2 but hides it
    /// from CPUID, so `FHP_SIMD_FORCE_AVX2` lets the x86_64 build exercise
    /// this backend on Apple silicon.
    fn avx2_enabled() -> bool {
        is_x86_feature_detected!("avx2") || std::env::var_os("FHP_SIMD_FORCE_AVX2").is_some()
    }

    #[test]
    fn compute_all_masks_matches_scalar() {
        if !avx2_enabled() {
            return;
        }
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        let avx2_masks = unsafe { compute_all_masks(input) };
        let scalar_masks = crate::scalar::compute_all_masks_safe(input);
        assert_eq!(avx2_masks.lt, scalar_masks.lt, "lt mismatch");
        assert_eq!(avx2_masks.gt, scalar_masks.gt, "gt mismatch");
        assert_eq!(avx2_masks.quot, scalar_masks.quot, "quot mismatch");
        assert_eq!(avx2_masks.apos, scalar_masks.apos, "apos mismatch");
    }

    #[test]
    fn compute_all_masks_matches_scalar_for_every_single_byte_block() {
        if !avx2_enabled() {
            return;
        }
        // SAFETY: AVX2 is available (checked above).
        unsafe { crate::differential::assert_single_byte_blocks_match_scalar(compute_all_masks) };
    }

    #[test]
    fn compute_all_masks_matches_scalar_for_delimiter_runs() {
        if !avx2_enabled() {
            return;
        }
        // SAFETY: AVX2 is available (checked above).
        unsafe { crate::differential::assert_delimiter_runs_match_scalar(compute_all_masks) };
    }

    #[test]
    fn compute_all_masks_64_byte_boundaries() {
        if !avx2_enabled() {
            return;
        }
        let mut input = vec![b'x'; 64];
        input[0] = b'<';
        input[31] = b'>';
        input[32] = b'"';
        input[63] = b'\'';
        let masks = unsafe { compute_all_masks(&input) };
        assert_eq!(masks.lt, 1u64 << 0);
        assert_eq!(masks.gt, 1u64 << 31);
        assert_eq!(masks.quot, 1u64 << 32);
        assert_eq!(masks.apos, 1u64 << 63);
    }

    #[test]
    fn compute_all_masks_ignores_bytes_after_64() {
        if !avx2_enabled() {
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
}
