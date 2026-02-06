//! 256-bit bloom filter for ancestor pre-filtering.
//!
//! During selector matching, descendant combinators (`A B`) require walking
//! up the ancestor chain. A bloom filter per node, recording which
//! tags/classes/ids exist among its ancestors, allows fast rejection without
//! the walk.
//!
//! The filter is stack-allocated (32 bytes) and built in a single DFS pass.

use hp_core::tag::Tag;
use hp_tree::arena::Arena;
use hp_tree::node::{NodeFlags, NodeId};

/// 256-bit bloom filter, stack-allocated.
///
/// Uses 4 `u64` words (256 bits total). False-positive rate is low
/// enough for ancestor pre-filtering with typical HTML documents.
#[derive(Clone, Copy, Debug)]
pub struct AncestorBloom {
    bits: [u64; 4],
}

impl AncestorBloom {
    /// Create an empty bloom filter.
    pub const fn new() -> Self {
        Self { bits: [0; 4] }
    }

    /// Insert a hash into the bloom filter.
    #[inline(always)]
    pub fn insert(&mut self, hash: u32) {
        let bit = (hash as usize) % 256;
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        self.bits[word] |= mask;
    }

    /// Check if a hash might be in the bloom filter.
    ///
    /// Returns `true` if the hash may be present (possible false positive).
    /// Returns `false` if the hash is definitely not present (guaranteed).
    #[inline(always)]
    pub fn may_contain(&self, hash: u32) -> bool {
        let bit = (hash as usize) % 256;
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        self.bits[word] & mask != 0
    }
}

impl Default for AncestorBloom {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a bloom-filter hash for a [`Tag`].
///
/// Uses Knuth's multiplicative hash on the tag discriminant.
#[inline]
pub fn hash_tag(tag: Tag) -> u32 {
    (tag as u32).wrapping_mul(2_654_435_761)
}

/// Compute a bloom-filter hash for a string (class name, id, etc.).
#[inline]
pub fn hash_str(s: &str) -> u32 {
    let mut h: u32 = 0;
    for &b in s.as_bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h.wrapping_mul(2_654_435_761)
}

/// Build ancestor bloom filters for every node in the arena.
///
/// Each node's bloom contains hashes for all tags, class names, and ids
/// found in its ancestor chain (not including itself). This is computed
/// in a single depth-first walk.
///
/// The returned `Vec` is indexed by `NodeId::index()`.
pub fn build_ancestor_blooms(arena: &Arena, root: NodeId) -> Vec<AncestorBloom> {
    let mut blooms = vec![AncestorBloom::new(); arena.len()];
    build_recursive(arena, root, &AncestorBloom::new(), &mut blooms);
    blooms
}

/// Recursive DFS to build ancestor blooms.
fn build_recursive(
    arena: &Arena,
    node: NodeId,
    ancestor_bloom: &AncestorBloom,
    blooms: &mut [AncestorBloom],
) {
    if node.is_null() {
        return;
    }

    // This node's ancestor bloom is the parent's bloom
    // (what's above this node, not including itself).
    blooms[node.index()] = *ancestor_bloom;

    let n = arena.get(node);

    // Build the bloom that children will inherit: ancestor_bloom + this node.
    let mut child_bloom = *ancestor_bloom;

    // Add this node's tag.
    if !n.flags.has(NodeFlags::IS_TEXT)
        && !n.flags.has(NodeFlags::IS_COMMENT)
        && !n.flags.has(NodeFlags::IS_DOCTYPE)
    {
        child_bloom.insert(hash_tag(n.tag));

        // Add class names and id.
        let attrs = arena.attrs(node);
        for attr in attrs {
            if attr.name == "class" {
                if let Some(ref val) = attr.value {
                    for class in val.split_whitespace() {
                        child_bloom.insert(hash_str(class));
                    }
                }
            }
            if attr.name == "id" {
                if let Some(ref val) = attr.value {
                    child_bloom.insert(hash_str(val));
                }
            }
        }
    }

    // Recurse into children.
    let mut child = n.first_child;
    while !child.is_null() {
        build_recursive(arena, child, &child_bloom, blooms);
        child = arena.get(child).next_sibling;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_insert_and_check() {
        let mut bloom = AncestorBloom::new();
        let h = hash_tag(Tag::Div);
        assert!(!bloom.may_contain(h));
        bloom.insert(h);
        assert!(bloom.may_contain(h));
    }

    #[test]
    fn bloom_empty_contains_nothing() {
        let bloom = AncestorBloom::new();
        assert!(!bloom.may_contain(0));
        assert!(!bloom.may_contain(42));
        assert!(!bloom.may_contain(u32::MAX));
    }

    #[test]
    fn bloom_multiple_inserts() {
        let mut bloom = AncestorBloom::new();
        let h1 = hash_tag(Tag::Div);
        let h2 = hash_tag(Tag::Span);
        let h3 = hash_str("active");

        bloom.insert(h1);
        bloom.insert(h2);
        bloom.insert(h3);

        assert!(bloom.may_contain(h1));
        assert!(bloom.may_contain(h2));
        assert!(bloom.may_contain(h3));
    }

    #[test]
    fn hash_tag_varies() {
        assert_ne!(hash_tag(Tag::Div), hash_tag(Tag::Span));
        assert_ne!(hash_tag(Tag::A), hash_tag(Tag::P));
    }

    #[test]
    fn hash_str_varies() {
        assert_ne!(hash_str("foo"), hash_str("bar"));
        assert_ne!(hash_str("active"), hash_str("hidden"));
    }

    #[test]
    fn build_blooms_small_tree() {
        let mut arena = Arena::new();
        let root = arena.new_element(Tag::Unknown, 0);
        let div = arena.new_element(Tag::Div, 1);
        arena.append_child(root, div);
        let span = arena.new_element(Tag::Span, 2);
        arena.append_child(div, span);

        let blooms = build_ancestor_blooms(&arena, root);

        // Root has no ancestors.
        assert!(!blooms[root.index()].may_contain(hash_tag(Tag::Div)));

        // Div's ancestors: root (Unknown).
        assert!(!blooms[div.index()].may_contain(hash_tag(Tag::Div)));
        assert!(blooms[div.index()].may_contain(hash_tag(Tag::Unknown)));

        // Span's ancestors: root, div.
        assert!(blooms[span.index()].may_contain(hash_tag(Tag::Div)));
        assert!(blooms[span.index()].may_contain(hash_tag(Tag::Unknown)));
    }
}
