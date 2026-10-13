/// Runtime SIMD feature detection and dispatch.
pub mod dispatch;
/// Portable scalar fallback — works on every platform.
pub mod scalar;

#[cfg(all(feature = "simd", target_arch = "x86_64"))]
/// SSE4.2 accelerated operations (128-bit, x86_64).
pub mod sse42;

#[cfg(all(feature = "simd", target_arch = "x86_64"))]
/// AVX2 accelerated operations (256-bit, x86_64).
pub mod avx2;

#[cfg(all(feature = "simd", target_arch = "aarch64"))]
/// ARM NEON accelerated operations (128-bit, aarch64).
pub mod neon;

/// Delimiter bitmasks for a block of up to 64 bytes.
///
/// Produced by [`compute_all_masks`](dispatch::SimdOps), which loads each
/// chunk only once and computes all masks in a single pass. Only the four
/// delimiters the tokenizer dispatches on are tracked.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllMasks {
    /// `<` positions.
    pub lt: u64,
    /// `>` positions.
    pub gt: u64,
    /// `"` positions.
    pub quot: u64,
    /// `'` positions.
    pub apos: u64,
}

/// Differential checks shared by every backend's unit tests. The scalar
/// implementation is the oracle.
#[cfg(test)]
pub(crate) mod differential {
    use std::fmt;

    use crate::AllMasks;
    use crate::scalar::compute_all_masks_safe;

    /// Signature shared by every backend's `compute_all_masks`.
    pub(crate) type ComputeAllMasks = unsafe fn(&[u8]) -> AllMasks;

    /// Longest block exercised: one full 64-byte block plus trailing bytes the
    /// masks must ignore.
    const MAX_LEN: usize = 72;

    /// Assert that `compute` agrees with the scalar oracle on every block of
    /// length `0..=72` that is all `a` except for one byte, for every position
    /// and every byte value.
    ///
    /// # Safety
    ///
    /// The CPU must support the target features `compute` was compiled for.
    pub(crate) unsafe fn assert_single_byte_blocks_match_scalar(compute: ComputeAllMasks) {
        let mut block = [b'a'; MAX_LEN];
        for len in 0..=MAX_LEN {
            for pos in 0..len {
                for byte in 0..=u8::MAX {
                    block[pos] = byte;
                    // SAFETY: forwarded from this function's contract.
                    unsafe {
                        assert_matches_scalar(
                            compute,
                            &block[..len],
                            format_args!("len={len} pos={pos} byte={byte:#04x}"),
                        );
                    }
                }
                block[pos] = b'a';
            }
        }
    }

    /// Assert that `compute` agrees with the scalar oracle on blocks of length
    /// `0..=72` made entirely of one delimiter.
    ///
    /// # Safety
    ///
    /// The CPU must support the target features `compute` was compiled for.
    pub(crate) unsafe fn assert_delimiter_runs_match_scalar(compute: ComputeAllMasks) {
        for delimiter in [b'<', b'>', b'"', b'\''] {
            let block = [delimiter; MAX_LEN];
            for len in 0..=MAX_LEN {
                // SAFETY: forwarded from this function's contract.
                unsafe {
                    assert_matches_scalar(
                        compute,
                        &block[..len],
                        format_args!("len={len} all {:?}", delimiter as char),
                    );
                }
            }
        }
    }

    /// # Safety
    ///
    /// The CPU must support the target features `compute` was compiled for.
    unsafe fn assert_matches_scalar(
        compute: ComputeAllMasks,
        block: &[u8],
        case: fmt::Arguments<'_>,
    ) {
        // SAFETY: forwarded from this function's contract.
        let actual = unsafe { compute(block) };
        let expected = compute_all_masks_safe(block);
        for (field, got, want) in [
            ("lt", actual.lt, expected.lt),
            ("gt", actual.gt, expected.gt),
            ("quot", actual.quot, expected.quot),
            ("apos", actual.apos, expected.apos),
        ] {
            assert_eq!(
                got, want,
                "{case}: `{field}` mask {got:#018x} differs from scalar {want:#018x}"
            );
        }
    }
}
