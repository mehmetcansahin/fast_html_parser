//! Runtime SIMD feature detection and function-pointer dispatch.
//!
//! On first access the best available SIMD level is detected via CPUID
//! (x86_64) or compile-time knowledge (aarch64, where NEON is mandatory).
//! The result is cached in a `OnceLock` so subsequent calls are a single
//! pointer load.

use std::sync::OnceLock;

use crate::{AllMasks, DelimiterResult};

/// Supported SIMD instruction set level, ordered from least to most capable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimdLevel {
    /// Portable scalar fallback — always available.
    Scalar,
    /// x86_64 SSE4.2 — 128-bit registers.
    Sse42,
    /// x86_64 AVX2 — 256-bit registers.
    Avx2,
    /// ARM aarch64 NEON — 128-bit registers (always available on aarch64).
    Neon,
}

/// Detect the best SIMD level supported by the current CPU at runtime.
///
/// On x86_64 this queries CPUID; on aarch64 NEON is always present.
/// All other architectures fall back to `Scalar`.
pub fn detect() -> SimdLevel {
    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2") {
            return SimdLevel::Avx2;
        }
        if is_x86_feature_detected!("sse4.2") {
            return SimdLevel::Sse42;
        }
    }

    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    {
        return SimdLevel::Neon;
    }

    #[allow(unreachable_code)]
    SimdLevel::Scalar
}

/// Function-pointer table for the three core SIMD operations.
///
/// Each slot holds an `unsafe fn` because the SIMD variants require
/// `#[target_feature]` and therefore cannot be called without `unsafe`.
/// The scalar implementations are inherently safe but share the same
/// signature for uniformity.
pub struct SimdOps {
    /// Scan for the first HTML delimiter (`<`, `>`, `&`, `"`, `'`, `=`, `/`).
    pub find_delimiters: unsafe fn(&[u8]) -> DelimiterResult,

    /// Classify every byte into a category bitmask (see [`crate::class`]).
    pub classify_bytes: unsafe fn(&[u8]) -> Vec<u8>,

    /// Return the offset of the first non-whitespace byte.
    pub skip_whitespace: unsafe fn(&[u8]) -> usize,

    /// Produce a u64 bitmask where bit `i` is set if `block[i] == byte`.
    /// The block must be at most 64 bytes.
    pub compute_byte_mask: unsafe fn(&[u8], u8) -> u64,

    /// Compute all seven delimiter bitmasks in a single pass.
    /// Loads each 16-byte chunk once, producing all masks simultaneously.
    pub compute_all_masks: unsafe fn(&[u8]) -> AllMasks,

    /// The SIMD level backing these function pointers.
    pub level: SimdLevel,
}

/// Global singleton — initialised once on first access.
static SIMD_OPS: OnceLock<SimdOps> = OnceLock::new();

/// Return a reference to the auto-detected [`SimdOps`] dispatch table.
///
/// The detection runs exactly once (via `OnceLock`); subsequent calls
/// are a single atomic load.
pub fn ops() -> &'static SimdOps {
    SIMD_OPS.get_or_init(|| {
        let level = detect();
        match level {
            #[cfg(all(feature = "simd", target_arch = "x86_64"))]
            SimdLevel::Avx2 => SimdOps {
                find_delimiters: crate::avx2::find_delimiters,
                classify_bytes: crate::avx2::classify_bytes,
                skip_whitespace: crate::avx2::skip_whitespace,
                compute_byte_mask: crate::avx2::compute_byte_mask,
                compute_all_masks: crate::avx2::compute_all_masks,
                level,
            },
            #[cfg(all(feature = "simd", target_arch = "x86_64"))]
            SimdLevel::Sse42 => SimdOps {
                find_delimiters: crate::sse42::find_delimiters,
                classify_bytes: crate::sse42::classify_bytes,
                skip_whitespace: crate::sse42::skip_whitespace,
                compute_byte_mask: crate::sse42::compute_byte_mask,
                compute_all_masks: crate::sse42::compute_all_masks,
                level,
            },
            #[cfg(all(feature = "simd", target_arch = "aarch64"))]
            SimdLevel::Neon => SimdOps {
                find_delimiters: crate::neon::find_delimiters,
                classify_bytes: crate::neon::classify_bytes,
                skip_whitespace: crate::neon::skip_whitespace,
                compute_byte_mask: crate::neon::compute_byte_mask,
                compute_all_masks: crate::neon::compute_all_masks,
                level,
            },
            // Scalar fallback for any architecture or if no SIMD detected.
            _ => SimdOps {
                find_delimiters: crate::scalar::find_delimiters,
                classify_bytes: crate::scalar::classify_bytes,
                skip_whitespace: crate::scalar::skip_whitespace,
                compute_byte_mask: crate::scalar::compute_byte_mask,
                compute_all_masks: crate::scalar::compute_all_masks,
                level: SimdLevel::Scalar,
            },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_valid_level() {
        let level = detect();
        // On this platform at least scalar should be available.
        assert!(level >= SimdLevel::Scalar);
    }

    #[cfg(not(feature = "simd"))]
    #[test]
    fn disabled_simd_forces_scalar_dispatch() {
        assert_eq!(detect(), SimdLevel::Scalar);
        assert_eq!(ops().level, SimdLevel::Scalar);
    }

    #[test]
    fn ops_singleton_is_consistent() {
        let a = ops();
        let b = ops();
        // Same pointer — OnceLock guarantees singleton.
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn ops_level_matches_detect() {
        let o = ops();
        let detected = detect();
        assert_eq!(o.level, detected);
    }

    #[test]
    fn dispatch_find_delimiters() {
        let o = ops();
        let input = b"hello <world>";
        let result = unsafe { (o.find_delimiters)(input) };
        assert_eq!(result, crate::DelimiterResult::Found { pos: 6, byte: b'<' });
    }

    #[test]
    fn dispatch_classify_bytes() {
        let o = ops();
        let input = b"a1 <";
        let result = unsafe { (o.classify_bytes)(input) };
        assert_eq!(result[0], crate::class::ALPHA);
        assert_eq!(result[1], crate::class::DIGIT);
        assert_eq!(result[2], crate::class::WHITESPACE);
        assert_eq!(result[3], crate::class::DELIMITER);
    }

    #[test]
    fn dispatch_skip_whitespace() {
        let o = ops();
        let input = b"   hello";
        let result = unsafe { (o.skip_whitespace)(input) };
        assert_eq!(result, 3);
    }

    #[test]
    fn dispatch_skip_whitespace_matches_scalar_at_every_short_length() {
        const HTML_WHITESPACE: &[u8] = b" \t\n\r\x0C";
        let o = ops();

        for len in 0..=128 {
            let mut input = Vec::with_capacity(len + 1);
            input.extend((0..len).map(|index| HTML_WHITESPACE[index % HTML_WHITESPACE.len()]));
            input.push(b'X');

            let dispatched = unsafe { (o.skip_whitespace)(&input) };
            let scalar = crate::scalar::skip_whitespace_safe(&input);
            assert_eq!(dispatched, scalar, "mismatch at whitespace length {len}");
            assert_eq!(dispatched, len);
        }
    }

    #[test]
    fn dispatch_compute_byte_mask() {
        let o = ops();
        let input = b"hello <world> & end";
        let mask = unsafe { (o.compute_byte_mask)(input, b'<') };
        assert_eq!(mask, 1 << 6);
    }

    #[test]
    fn dispatch_compute_byte_mask_over_64_bytes_does_not_overflow() {
        // The active backend must cap at 64 bytes rather than overflow the
        // shift when handed an over-long block.
        let o = ops();
        let input = vec![b'<'; 80];
        let mask = unsafe { (o.compute_byte_mask)(&input, b'<') };
        assert_eq!(mask, u64::MAX);
        let masks = unsafe { (o.compute_all_masks)(&input) };
        assert_eq!(masks.lt, u64::MAX);
    }

    #[test]
    fn dispatch_compute_byte_mask_matches_scalar() {
        let o = ops();
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        for &byte in b"<>&\"'=/" {
            let dispatched = unsafe { (o.compute_byte_mask)(input, byte) };
            let scalar = unsafe { crate::scalar::compute_byte_mask(input, byte) };
            assert_eq!(dispatched, scalar, "mismatch for byte 0x{byte:02X}");
        }
    }

    #[test]
    fn dispatch_matches_scalar() {
        let o = ops();
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end";
        let dispatched = unsafe { (o.classify_bytes)(input) };
        let scalar = unsafe { crate::scalar::classify_bytes(input) };
        assert_eq!(dispatched, scalar);
    }

    #[test]
    fn dispatch_compute_all_masks() {
        let o = ops();
        let input = b"Hello <World> & \"test\" = 'value' / 123\n\r\t end!!";
        let dispatched = unsafe { (o.compute_all_masks)(input) };
        let scalar = crate::scalar::compute_all_masks_safe(input);
        assert_eq!(dispatched.lt, scalar.lt, "lt mismatch");
        assert_eq!(dispatched.gt, scalar.gt, "gt mismatch");
        assert_eq!(dispatched.quot, scalar.quot, "quot mismatch");
        assert_eq!(dispatched.apos, scalar.apos, "apos mismatch");
    }
}
