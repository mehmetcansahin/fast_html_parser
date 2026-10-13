//! SSE4.2 accelerated operations (128-bit, x86_64).
//!
//! Each function is marked `#[target_feature(enable = "sse4.2")]` so the
//! compiler generates SSE4.2 instructions without requiring the entire
//! crate to be compiled with `-C target-feature=+sse4.2`.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

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

            // Widen the `i32` movemask through `u32`, as in the AVX2 backend.
            let m_lt = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, lt)) as u32 as u64;
            let m_gt = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, gt)) as u32 as u64;
            let m_quot = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, quot)) as u32 as u64;
            let m_apos = _mm_movemask_epi8(_mm_cmpeq_epi8(chunk, apos)) as u32 as u64;

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

    /// Whether the SSE4.2 tests should run. `FHP_SIMD_FORCE_SSE42` mirrors
    /// `FHP_SIMD_FORCE_AVX2` for emulators whose CPUID under-reports the
    /// instruction sets they execute (Rosetta 2 executes AVX2 but hides it).
    fn sse42_enabled() -> bool {
        is_x86_feature_detected!("sse4.2") || std::env::var_os("FHP_SIMD_FORCE_SSE42").is_some()
    }

    #[test]
    fn compute_all_masks_matches_scalar() {
        if !sse42_enabled() {
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
    fn compute_all_masks_matches_scalar_for_every_single_byte_block() {
        if !sse42_enabled() {
            return;
        }
        // SAFETY: SSE4.2 is available (checked above).
        unsafe { crate::differential::assert_single_byte_blocks_match_scalar(compute_all_masks) };
    }

    #[test]
    fn compute_all_masks_matches_scalar_for_delimiter_runs() {
        if !sse42_enabled() {
            return;
        }
        // SAFETY: SSE4.2 is available (checked above).
        unsafe { crate::differential::assert_delimiter_runs_match_scalar(compute_all_masks) };
    }

    #[test]
    fn compute_all_masks_64_byte_boundaries() {
        if !sse42_enabled() {
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
        if !sse42_enabled() {
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
