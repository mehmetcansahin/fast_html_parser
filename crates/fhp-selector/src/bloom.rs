//! Ancestor pre-filtering for descendant combinators.
//!
//! During selector matching, descendant combinators (`A B`) require walking
//! up the ancestor chain. A summary of the tags/classes/ids among a node's
//! ancestors allows fast rejection without the walk.
//!
//! Selection carries an [`AncestorFilter`] down its depth-first walk, built
//! from hashes the arena already stores. The public [`AncestorBloom`] table
//! builds a 256-bit bloom filter per node instead.

use fhp_core::hash::selector_hash;
use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::node::{Node, NodeFlags, NodeId};

use crate::ast::{Combinator, Selector, SimpleSelector};
use crate::space::split_html_space;

/// The tags, classes, and ids of an element's ancestors, carried down a
/// depth-first selection walk.
///
/// Built from fields the arena stores for every element: tags form an exact
/// set (`Tag` is a `u8`), and classes and ids are 64-bit bloom filters fed by
/// `Node::class_hash` and `Node::id_hash`. A filter never rejects a node that
/// has a matching ancestor; the matcher still verifies every candidate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AncestorFilter {
    tags: [u64; 4],
    classes: u64,
    ids: u64,
}

impl AncestorFilter {
    /// The filter of `node`'s ancestors, built by walking its parent chain.
    pub(crate) fn of_ancestors(arena: &Arena, node: NodeId) -> Self {
        let mut filter = Self::default();
        let mut ancestor = arena.get(node).parent;
        while !ancestor.is_null() {
            let n = arena.get(ancestor);
            filter = filter.with(n);
            ancestor = n.parent;
        }
        filter
    }

    /// This filter extended with `node`, as seen by `node`'s children. Text,
    /// comment, and doctype nodes contribute nothing.
    #[inline]
    pub(crate) fn with(mut self, node: &Node) -> Self {
        if node
            .flags
            .has(NodeFlags::IS_TEXT | NodeFlags::IS_COMMENT | NodeFlags::IS_DOCTYPE)
        {
            return self;
        }
        self.insert_tag(node.tag);
        self.classes |= node.class_hash;
        // `id_hash == 0` means "no id".
        if node.id_hash != 0 {
            self.ids |= id_bit(node.id_hash);
        }
        self
    }

    /// What some ancestor of every match of `selector` must contain: the
    /// known tags, classes, and ids of each compound reached through a
    /// descendant or child combinator.
    ///
    /// Such a compound is always an ancestor of the subject, because sibling
    /// combinators keep the ancestor set and the other two only shrink it.
    pub(crate) fn required_by(selector: &Selector) -> Self {
        let mut required = Self::default();
        for (combinator, compound) in &selector.chain {
            if !matches!(combinator, Combinator::Descendant | Combinator::Child) {
                continue;
            }
            for part in &compound.parts {
                match part {
                    // Unknown and custom elements share `Tag::Unknown`, so a
                    // name cannot be required.
                    SimpleSelector::Tag(tag) if *tag != Tag::Unknown => required.insert_tag(*tag),
                    SimpleSelector::Class(_, bloom_bit) => required.classes |= bloom_bit,
                    // A zero hash is the arena's "no id" sentinel.
                    SimpleSelector::Id(_, hash) if *hash != 0 => required.ids |= id_bit(*hash),
                    _ => {}
                }
            }
        }
        required
    }

    /// Whether this filter is empty, i.e. requires nothing.
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether these ancestors may satisfy every entry of `required`.
    #[inline]
    pub(crate) fn may_satisfy(&self, required: &Self) -> bool {
        self.tags
            .iter()
            .zip(&required.tags)
            .all(|(have, need)| have & need == *need)
            && self.classes & required.classes == required.classes
            && self.ids & required.ids == required.ids
    }

    #[inline]
    fn insert_tag(&mut self, tag: Tag) {
        let tag = tag as usize;
        self.tags[tag / 64] |= 1 << (tag % 64);
    }
}

/// The id bloom bit for a non-zero id hash.
#[inline]
fn id_bit(hash: u32) -> u64 {
    1 << (hash % 64)
}

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
fn fill_ancestor_blooms(arena: &Arena, root: NodeId, blooms: &mut [AncestorBloom]) {
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

    fn required(css: &str) -> AncestorFilter {
        let list = crate::parser::parse_selector(css).unwrap();
        AncestorFilter::required_by(&list.selectors[0])
    }

    #[test]
    fn required_filter_takes_ancestor_compounds_only() {
        assert!(required("p").is_empty());
        assert!(required("a + p").is_empty());
        assert!(required("a ~ p").is_empty());
        assert!(required("my-widget p").is_empty());
        assert!(required("* p").is_empty());
        assert_eq!(required("div p"), required("div > p"));
        assert_eq!(required("div p"), required("div a ~ p"));
        assert_ne!(required("div p"), required("span p"));
        assert_ne!(required(".a p"), AncestorFilter::default());
        assert_ne!(required("#main p"), AncestorFilter::default());
    }

    #[test]
    fn ancestor_filter_accumulates_tags_classes_and_ids() {
        let doc = fhp_tree::parse(
            "<section id='main'><div class='card wide'><p>x</p></div></section><p>y</p>",
        )
        .unwrap();
        let arena = doc.arena();
        let mut paragraphs = Vec::new();
        for index in 0..arena.len() {
            let id = NodeId(index as u32);
            if arena.get(id).tag == Tag::P {
                paragraphs.push(AncestorFilter::of_ancestors(arena, id));
            }
        }
        let [inner, outer] = paragraphs.as_slice() else {
            panic!("expected two paragraphs");
        };
        for css in [
            "section p",
            "div p",
            ".card p",
            ".wide p",
            "#main p",
            "#main .card > p",
        ] {
            assert!(inner.may_satisfy(&required(css)), "{css}");
        }
        assert!(!inner.may_satisfy(&required("span p")));
        assert!(!outer.may_satisfy(&required("div p")));
        assert!(!outer.may_satisfy(&required("section p")));
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
