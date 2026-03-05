//! Fast hash functions for selector matching.
//!
//! Provides FNV-1a hashing used by the arena (to compute per-node class/id
//! hashes during tree building) and by the selector matcher (to reject
//! non-matching nodes with a single integer comparison).

/// FNV-1a hash of a byte slice, returning a `u32`.
///
/// Used for exact-match comparisons (id_hash) and as a building block
/// for the class bloom filter.
#[inline]
pub fn selector_hash(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 2_166_136_261; // FNV offset basis
    for &byte in bytes {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16_777_619); // FNV prime
    }
    hash
}

/// Compute a single bloom bit position from a class token.
///
/// Returns a `u32` with exactly one bit set at position `hash % 32`.
/// Used to build and query the per-node 32-bit class bloom filter.
#[inline]
pub fn class_bloom_bit(class_bytes: &[u8]) -> u32 {
    1u32 << (selector_hash(class_bytes) % 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_deterministic() {
        assert_eq!(selector_hash(b"hello"), selector_hash(b"hello"));
    }

    #[test]
    fn hash_different_inputs() {
        assert_ne!(selector_hash(b"hello"), selector_hash(b"world"));
    }

    #[test]
    fn hash_empty() {
        // Empty input should return the offset basis.
        assert_eq!(selector_hash(b""), 2_166_136_261);
    }

    #[test]
    fn bloom_bit_single_bit() {
        let bit = class_bloom_bit(b"content");
        assert_eq!(bit.count_ones(), 1);
    }

    #[test]
    fn bloom_bit_in_range() {
        let bit = class_bloom_bit(b"test");
        assert!(bit != 0);
        assert!(bit.is_power_of_two());
    }
}
