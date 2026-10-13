//! Portable scalar fallback for all SIMD operations.
//!
//! Every function here processes bytes one at a time. It is the baseline
//! that SIMD backends must match semantically (and beat in throughput).

/// Compute all four delimiter bitmasks in a single pass over the block.
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn compute_all_masks_sets_one_bit_per_delimiter() {
        let masks = compute_all_masks_safe(b"a<b>c\"d'e&f=g/");
        assert_eq!(masks.lt, 1 << 1);
        assert_eq!(masks.gt, 1 << 3);
        assert_eq!(masks.quot, 1 << 5);
        assert_eq!(masks.apos, 1 << 7);
    }
}
