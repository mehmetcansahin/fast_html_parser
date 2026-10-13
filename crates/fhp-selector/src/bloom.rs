//! 256-bit bloom filter for ancestor pre-filtering.
//!
//! During selector matching, descendant combinators (`A B`) require walking
//! up the ancestor chain. A bloom filter per node, recording which
//! tags/classes/ids exist among its ancestors, allows fast rejection without
//! the walk.
//!
//! The filter is stack-allocated (32 bytes) and built in a single DFS pass.

use fhp_core::hash::selector_hash;
use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::node::{NodeFlags, NodeId};

use crate::space::split_html_space;

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
/// Uses FNV-1a on the tag discriminant byte, consistent with
/// [`selector_hash`] used for per-node class/id hashes.
#[inline]
pub fn hash_tag(tag: Tag) -> u32 {
    selector_hash(&[tag as u8])
}

/// Compute a bloom-filter hash for a string (class name, id, etc.).
///
/// Delegates to [`selector_hash`] (FNV-1a) so that ancestor bloom
/// hashes are consistent with per-node class/id hashes.
#[inline]
pub fn hash_str(s: &str) -> u32 {
    selector_hash(s.as_bytes())
}

/// Build ancestor bloom filters for `root` and every node below it.
///
/// Each node's bloom contains hashes for all tags, class names, and ids
/// found in its ancestor chain (not including itself), including the
/// ancestors above `root`: selector matching walks real ancestors, so a
/// subtree root must not start from an empty filter. This is computed in a
/// single depth-first walk seeded by the ancestors of `root`.
///
/// The returned `Vec` is indexed by `NodeId::index()`; entries outside the
/// subtree of `root` stay empty.
pub fn build_ancestor_blooms(arena: &Arena, root: NodeId) -> Vec<AncestorBloom> {
    let mut blooms = vec![AncestorBloom::new(); arena.len()];
    fill_ancestor_blooms(arena, root, &mut blooms);
    blooms
}

/// Fill the ancestor blooms of `root` and its subtree into a caller-owned
/// table indexed by `NodeId::index()` (see [`build_ancestor_blooms`]).
///
/// Entries outside the subtree are left untouched, so one table can be
/// filled for several roots.
pub(crate) fn fill_ancestor_blooms(arena: &Arena, root: NodeId, blooms: &mut [AncestorBloom]) {
    if root.is_null() {
        return;
    }

    let mut seed = AncestorBloom::new();
    let mut ancestor = arena.get(root).parent;
    while !ancestor.is_null() {
        insert_node_hashes(arena, ancestor, &mut seed);
        ancestor = arena.get(ancestor).parent;
    }
    build_recursive(arena, root, &seed, blooms);
}

/// Add the hashes a node contributes to its descendants' blooms: its tag,
/// class names, and id. Text, comment, and doctype nodes contribute nothing.
fn insert_node_hashes(arena: &Arena, node: NodeId, bloom: &mut AncestorBloom) {
    let n = arena.get(node);
    if n.flags.has(NodeFlags::IS_TEXT)
        || n.flags.has(NodeFlags::IS_COMMENT)
        || n.flags.has(NodeFlags::IS_DOCTYPE)
    {
        return;
    }

    bloom.insert(hash_tag(n.tag));

    for attr in arena.attrs(node) {
        let name = arena.attr_name(attr);
        // Attribute names are not lowercased during parsing, and the matcher
        // compares them case-insensitively, so the bloom must too (the values
        // stay case-sensitive, matching CSS semantics).
        if name.eq_ignore_ascii_case("class") {
            if let Some(val) = arena.attr_value(attr) {
                for class in split_html_space(val) {
                    bloom.insert(hash_str(class));
                }
            }
        }
        if name.eq_ignore_ascii_case("id") {
            if let Some(val) = arena.attr_value(attr) {
                bloom.insert(hash_str(val));
            }
        }
    }
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

    // Build the bloom that children will inherit: ancestor_bloom + this node.
    let mut child_bloom = *ancestor_bloom;
    insert_node_hashes(arena, node, &mut child_bloom);

    // Recurse into children.
    let mut child = arena.get(node).first_child;
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

    #[test]
    fn subtree_blooms_include_ancestors_above_the_root() {
        let mut arena = Arena::new();
        let root = arena.new_element(Tag::Unknown, 0);
        let div = arena.new_element(Tag::Div, 1);
        arena.append_child(root, div);
        let section = arena.new_element(Tag::Section, 2);
        arena.append_child(div, section);
        let p = arena.new_element(Tag::P, 3);
        arena.append_child(section, p);

        let blooms = build_ancestor_blooms(&arena, section);

        assert!(blooms[section.index()].may_contain(hash_tag(Tag::Div)));
        assert!(!blooms[section.index()].may_contain(hash_tag(Tag::Section)));
        assert!(blooms[p.index()].may_contain(hash_tag(Tag::Div)));
        assert!(blooms[p.index()].may_contain(hash_tag(Tag::Section)));
        // Nodes outside the subtree are not filled.
        assert!(!blooms[div.index()].may_contain(hash_tag(Tag::Unknown)));
    }
}
