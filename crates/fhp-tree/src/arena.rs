//! Arena allocator for DOM nodes, text, and attributes.
//!
//! All nodes live in a single contiguous `Vec<Node>`, giving cache-friendly
//! traversal. Text content and attributes are stored in separate slabs,
//! referenced by offset+length from each [`Node`].
//!
//! [`Node`]: crate::node::Node

use fhp_core::attr::{SeenAttributes, ascii_ci_fingerprint, iter_raw_attrs};
use fhp_core::hash::{class_bloom_bit, selector_hash};
use fhp_core::is_html_whitespace;
use fhp_core::tag::Tag;

use crate::node::{Node, NodeFlags, NodeId};

/// A compact attribute stored in the attribute slab.
///
/// Names and values are stored as offsets into `Arena::attr_str_slab` rather
/// than separate heap allocations. Use [`Arena::attr_name`] and
/// [`Arena::attr_value`] to access the strings.
#[derive(Clone, Debug)]
pub struct Attribute {
    name_offset: u32,
    name_len: u32,
    value_offset: u32,
    value_len: u32,
    has_value: bool,
}

/// Number of tag index buckets (Tag is `repr(u8)`, 256 possible values).
const TAG_INDEX_SIZE: usize = 256;

struct PendingSourceCopy {
    source_offset: usize,
    text_offset: usize,
    len: usize,
}

#[inline]
fn update_selector_hashes(
    name: &[u8],
    value: Option<&[u8]>,
    class_hash: &mut u64,
    id_hash: &mut u32,
) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };

    if name.eq_ignore_ascii_case(b"class") {
        for class in value.split(|byte| is_html_whitespace(*byte)) {
            if !class.is_empty() {
                *class_hash |= class_bloom_bit(class);
            }
        }
    } else if name.eq_ignore_ascii_case(b"id") {
        *id_hash = selector_hash(value);
    }
}

/// Arena-based storage for all DOM nodes, text content, and attributes.
///
/// Nodes are stored in a contiguous `Vec<Node>` for cache-line-friendly access.
/// Text and attributes are stored in separate slabs and referenced by
/// offset+length from each node.
pub struct Arena {
    /// All nodes in insertion order.
    pub(crate) nodes: Vec<Node>,
    /// All text content concatenated (for entity-decoded or owned text).
    pub(crate) text_slab: String,
    /// All attributes in insertion order.
    pub(crate) attr_slab: Vec<Attribute>,
    /// All attribute names and values concatenated.
    pub(crate) attr_str_slab: String,
    /// Owned copy of the original input source.
    ///
    /// Text nodes that reference entity-free (borrowed) regions of the input
    /// store offsets into this buffer via [`NodeFlags::IS_TEXT_FROM_SOURCE`].
    /// Empty for streaming parsers.
    pub(crate) source: String,
    /// Source ranges promoted into the text slab before an owned source is
    /// transferred to the arena.
    pending_source_copies: Vec<PendingSourceCopy>,
    /// Pre-built tag → NodeId index, populated during tree construction.
    ///
    /// Indexed by `Tag as u8`. Each bucket contains NodeIds of elements with
    /// that tag in allocation order. HTML foster parenting can make allocation
    /// order differ from document order, so document-ordered consumers must
    /// still traverse the tree links.
    pub(crate) tag_index: Option<Box<[Vec<NodeId>; TAG_INDEX_SIZE]>>,
}

impl Arena {
    /// Create a new empty arena.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            text_slab: String::new(),
            attr_slab: Vec::new(),
            attr_str_slab: String::new(),
            source: String::new(),
            pending_source_copies: Vec::new(),
            tag_index: None,
        }
    }

    /// Create a new arena with pre-allocated capacity.
    pub fn with_capacity(node_cap: usize, text_cap: usize, attr_cap: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(node_cap),
            text_slab: String::with_capacity(text_cap),
            attr_slab: Vec::with_capacity(attr_cap),
            attr_str_slab: String::with_capacity(attr_cap * 32),
            source: String::new(),
            pending_source_copies: Vec::new(),
            tag_index: None,
        }
    }

    /// Enable the inline tag index.
    ///
    /// Once enabled, every [`Arena::new_element`] call appends the node id to
    /// the corresponding tag bucket. Consumers can retrieve the index via
    /// [`Arena::tag_index`].
    pub fn enable_tag_index(&mut self) {
        if self.tag_index.is_none() {
            // Use a boxed array to avoid 256 * 24 = 6 KB on the stack.
            self.tag_index = Some(Box::new(std::array::from_fn(|_| Vec::new())));
        }
    }

    /// Get the pre-built allocation-order tag index, if enabled.
    ///
    /// Buckets are not guaranteed to be in document order because tree
    /// construction can foster-parent an element before an earlier allocation.
    pub fn tag_index(&self) -> Option<&[Vec<NodeId>; TAG_INDEX_SIZE]> {
        self.tag_index.as_deref()
    }

    /// Allocate a new element node and return its id.
    pub fn new_element(&mut self, tag: Tag, depth: u16) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_element(tag, depth));
        // Populate inline tag index if enabled.
        if let Some(ref mut idx) = self.tag_index {
            idx[tag as u8 as usize].push(id);
        }
        id
    }

    /// Store the original tag name for an unknown/custom element.
    pub fn set_unknown_tag_name(&mut self, node: NodeId, tag_name: &str) {
        if tag_name.is_empty() || self.nodes[node.index()].tag != Tag::Unknown {
            return;
        }
        let offset = self.text_slab.len() as u32;
        let len = tag_name.len() as u32;
        self.text_slab.push_str(tag_name);
        let n = &mut self.nodes[node.index()];
        n.text_offset = offset;
        n.text_len = len;
    }

    /// Allocate a new text node, storing content in the text slab.
    pub fn new_text(&mut self, depth: u16, text: &str) -> NodeId {
        let offset = self.text_slab.len() as u32;
        let len = text.len() as u32;
        self.text_slab.push_str(text);
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_text(depth, offset, len));
        id
    }

    /// Allocate a text node that references a region of the original source.
    ///
    /// Instead of copying content to the text slab, this stores a
    /// `(source_offset, len)` pair and sets [`NodeFlags::IS_TEXT_FROM_SOURCE`].
    /// The source must have been set via [`Arena::set_source`] before calling
    /// this method.
    pub fn new_text_ref(&mut self, depth: u16, source_offset: u32, len: u32) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        let mut node = Node::new_text(depth, source_offset, len);
        node.flags.set(NodeFlags::IS_TEXT_FROM_SOURCE);
        self.nodes.push(node);
        id
    }

    /// Store an owned copy of the input source for source-backed text nodes.
    pub fn set_source(&mut self, input: &str) {
        self.source = input.to_owned();
        self.resolve_pending_source_copies();
    }

    /// Transfer an already-owned `String` as the source buffer (zero copy).
    ///
    /// When the caller owns the input `String` (e.g., from an HTTP response),
    /// this avoids the memcpy that [`set_source`](Arena::set_source) performs.
    pub fn set_source_owned(&mut self, source: String) {
        self.source = source;
        self.resolve_pending_source_copies();
    }

    /// Promote source-backed text into the owned text slab and append a new
    /// adjacent fragment. If the zero-copy owned source has not been
    /// transferred yet, reserve its range and fill it in at transfer time.
    pub(crate) fn promote_source_text(
        &mut self,
        source_offset: usize,
        source_len: usize,
        content: &str,
    ) -> Option<u32> {
        let source_end = source_offset.checked_add(source_len)?;
        let new_offset = self.text_slab.len();
        // Reject a promoted range whose end would overflow.
        new_offset.checked_add(source_len)?;
        let new_offset_u32 = u32::try_from(new_offset).ok()?;
        self.text_slab
            .reserve(source_len.checked_add(content.len())?);

        if self.source.is_empty() {
            self.text_slab.extend(std::iter::repeat_n('\0', source_len));
            self.pending_source_copies.push(PendingSourceCopy {
                source_offset,
                text_offset: new_offset,
                len: source_len,
            });
        } else {
            let source_text = self.source.get(source_offset..source_end)?;
            self.text_slab.push_str(source_text);
        }
        self.text_slab.push_str(content);
        Some(new_offset_u32)
    }

    fn resolve_pending_source_copies(&mut self) {
        for pending in std::mem::take(&mut self.pending_source_copies) {
            let source_end = pending
                .source_offset
                .checked_add(pending.len)
                .expect("pending source range overflow");
            let text_end = pending
                .text_offset
                .checked_add(pending.len)
                .expect("pending text range overflow");
            let source = self
                .source
                .get(pending.source_offset..source_end)
                .expect("owned source does not contain a pending text range");
            assert!(
                text_end <= self.text_slab.len(),
                "text slab does not contain a pending source range"
            );
            // The placeholder is NUL bytes, so the range is on character
            // boundaries and the replacement keeps the slab length.
            self.text_slab
                .replace_range(pending.text_offset..text_end, source);
        }
    }

    /// Allocate a new comment node, storing content in the text slab.
    pub fn new_comment(&mut self, depth: u16, text: &str) -> NodeId {
        let offset = self.text_slab.len() as u32;
        let len = text.len() as u32;
        self.text_slab.push_str(text);
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_comment(depth, offset, len));
        id
    }

    /// Allocate a new doctype node, storing content in the text slab.
    pub fn new_doctype(&mut self, depth: u16, text: &str) -> NodeId {
        let offset = self.text_slab.len() as u32;
        let len = text.len() as u32;
        self.text_slab.push_str(text);
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_doctype(depth, offset, len));
        id
    }

    /// Set attributes for a node from tokenizer attributes.
    ///
    /// Names are compared ASCII case-insensitively; the first occurrence of a
    /// name wins and later duplicates are dropped.
    pub fn set_attrs(&mut self, node: NodeId, attrs: &[fhp_tokenizer::token::Attribute<'_>]) {
        if attrs.is_empty() {
            return;
        }
        let offset = self.attr_slab.len() as u32;
        let mut count = 0u16;
        let mut seen = SeenAttributes::new();
        let mut class_hash = 0u64;
        let mut id_hash = 0u32;

        for attr in attrs {
            if count == u16::MAX {
                break;
            }
            let name = attr.name.as_bytes();
            if seen.duplicate_or_insert(name, ascii_ci_fingerprint(name)) {
                continue;
            }
            let name_offset = self.attr_str_slab.len() as u32;
            self.attr_str_slab.push_str(&attr.name);
            let name_len = attr.name.len() as u32;

            let (value_offset, value_len, has_value) = if let Some(v) = &attr.value {
                update_selector_hashes(name, Some(v.as_bytes()), &mut class_hash, &mut id_hash);
                let vo = self.attr_str_slab.len() as u32;
                self.attr_str_slab.push_str(v);
                (vo, v.len() as u32, true)
            } else {
                (0, 0, false)
            };

            self.attr_slab.push(Attribute {
                name_offset,
                name_len,
                value_offset,
                value_len,
                has_value,
            });
            count += 1;
        }

        if count == 0 {
            return;
        }

        let n = &mut self.nodes[node.index()];
        n.attr_offset = offset;
        n.attr_count = count;
        n.flags.set(NodeFlags::HAS_ATTRS);
        n.class_hash = class_hash;
        n.id_hash = id_hash;
    }

    /// Parse attributes directly from a raw attribute region into the slab.
    ///
    /// Skips all intermediate `Vec<Attribute>` allocation — names and values
    /// are written directly to `attr_str_slab` and compact `Attribute` structs
    /// are pushed to `attr_slab` in a single pass. Duplicate names (ASCII
    /// case-insensitive) keep the first occurrence.
    pub fn set_attrs_from_raw(&mut self, node: NodeId, attr_raw: &str) {
        let end = attr_raw.len();
        if end == 0 {
            return;
        }

        // Most raw attribute regions average roughly ten bytes per attribute.
        // Bound the initial reservation so duplicate-heavy or single-value
        // inputs do not reserve in proportion to their full source length.
        let estimated_attrs = (end / 10).clamp(1, 16);
        if estimated_attrs > 1 {
            self.attr_slab.reserve(estimated_attrs);
            self.attr_str_slab.reserve(estimated_attrs * 16);
        }

        let slab_offset = self.attr_slab.len() as u32;
        let mut count: u16 = 0;
        let mut seen = SeenAttributes::new();
        let mut class_hash = 0u64;
        let mut id_hash = 0u32;

        for attr in iter_raw_attrs(attr_raw) {
            if count == u16::MAX {
                break;
            }
            let name = attr.name.as_bytes();
            if seen.duplicate_or_insert(name, attr.fingerprint) {
                continue;
            }
            let name_offset = self.attr_str_slab.len() as u32;
            self.attr_str_slab.push_str(attr.name);

            let (value_offset, value_len, has_value) = match attr.value {
                Some(raw_value) => {
                    let (value_offset, value_len) = self.push_attr_value(raw_value);
                    let value_start = value_offset as usize;
                    update_selector_hashes(
                        name,
                        Some(
                            &self.attr_str_slab.as_bytes()
                                [value_start..value_start + value_len as usize],
                        ),
                        &mut class_hash,
                        &mut id_hash,
                    );
                    (value_offset, value_len, true)
                }
                None => (0, 0, false),
            };

            self.attr_slab.push(Attribute {
                name_offset,
                name_len: name.len() as u32,
                value_offset,
                value_len,
                has_value,
            });
            count += 1;
        }

        if count > 0 {
            let n = &mut self.nodes[node.index()];
            n.attr_offset = slab_offset;
            n.attr_count = count;
            n.flags.set(NodeFlags::HAS_ATTRS);
            n.class_hash = class_hash;
            n.id_hash = id_hash;
        }
    }

    /// Write an attribute value to the string slab, with optional entity decoding.
    #[cfg(feature = "entity-decode")]
    fn push_attr_value(&mut self, raw_value: &str) -> (u32, u32) {
        let offset = self.attr_str_slab.len() as u32;
        let decoded = fhp_tokenizer::entity::decode_attribute_entities(raw_value);
        self.attr_str_slab.push_str(&decoded);
        (offset, decoded.len() as u32)
    }

    /// Write an attribute value to the string slab (no entity decoding).
    #[cfg(not(feature = "entity-decode"))]
    fn push_attr_value(&mut self, raw_value: &str) -> (u32, u32) {
        let offset = self.attr_str_slab.len() as u32;
        self.attr_str_slab.push_str(raw_value);
        (offset, raw_value.len() as u32)
    }

    /// The full 1-based element sibling index of a node.
    #[inline]
    pub(crate) fn full_element_index(&self, node: NodeId) -> u32 {
        self.nodes[node.index()].attr_raw_offset
    }

    /// Whether a node is an element (not text, comment, or doctype).
    #[inline]
    pub(crate) fn is_element(&self, node: NodeId) -> bool {
        let flags = self.nodes[node.index()].flags;
        !flags.has(NodeFlags::IS_TEXT)
            && !flags.has(NodeFlags::IS_COMMENT)
            && !flags.has(NodeFlags::IS_DOCTYPE)
    }

    /// Set the full sibling index while maintaining the compact public field.
    #[inline]
    pub(crate) fn set_full_element_index(&mut self, node: NodeId, index: u32) {
        let target = &mut self.nodes[node.index()];
        target.attr_raw_offset = index;
        target.element_index = index.min(u32::from(u16::MAX)) as u16;
    }

    /// Set the self-closing flag on a node.
    pub fn set_self_closing(&mut self, node: NodeId) {
        self.nodes[node.index()]
            .flags
            .set(NodeFlags::IS_SELF_CLOSING);
    }

    /// Append `child` as the last child of `parent`.
    ///
    /// Updates all tree links: parent, first_child, last_child, prev_sibling,
    /// next_sibling. An element child also gets its 1-based index among its
    /// element siblings ([`Node::element_index`]), which `:nth-child`
    /// selectors read.
    ///
    /// `child` must be a freshly allocated, not-yet-linked node, distinct from
    /// `parent` (and therefore from `parent`'s current last child).
    ///
    /// # Panics
    ///
    /// Panics if either id is out of bounds, both ids refer to the same node,
    /// `child` is already linked, or the parent's existing child links are
    /// inconsistent.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let parent_index = parent.index();
        let child_index = child.index();
        let node_count = self.nodes.len();

        assert!(
            parent_index < node_count,
            "append_child: parent NodeId is out of bounds"
        );
        assert!(
            child_index < node_count,
            "append_child: child NodeId is out of bounds"
        );
        assert!(
            parent != child,
            "append_child: parent and child must be distinct nodes"
        );

        let child_node = &self.nodes[child_index];
        assert!(
            child_node.parent.is_null()
                && child_node.prev_sibling.is_null()
                && child_node.next_sibling.is_null(),
            "append_child: child is already linked"
        );
        assert!(
            child_node.first_child.is_null() && child_node.last_child.is_null(),
            "append_child: child already owns a subtree"
        );

        let first = self.nodes[parent_index].first_child;
        let last = self.nodes[parent_index].last_child;
        assert_eq!(
            first.is_null(),
            last.is_null(),
            "append_child: parent child links are inconsistent"
        );

        let last_index = if last.is_null() {
            None
        } else {
            let index = last.index();
            assert!(
                index < node_count,
                "append_child: parent last_child NodeId is out of bounds"
            );
            assert!(
                last != parent && last != child,
                "append_child: parent last_child aliases parent or child"
            );
            let last_node = &self.nodes[index];
            assert!(
                last_node.parent == parent && last_node.next_sibling.is_null(),
                "append_child: parent last_child link is inconsistent"
            );
            Some(index)
        };

        self.nodes[child_index].parent = parent;
        if let Some(index) = last_index {
            self.nodes[index].next_sibling = child;
            self.nodes[child_index].prev_sibling = last;
        } else {
            self.nodes[parent_index].first_child = child;
        }
        self.nodes[parent_index].last_child = child;
        self.nodes[parent_index].flags.set(NodeFlags::HAS_CHILDREN);

        // The tree builder maintains this index itself; trees built through
        // this API need it too.
        if self.is_element(child) {
            let mut previous = last;
            while !previous.is_null() && !self.is_element(previous) {
                previous = self.nodes[previous.index()].prev_sibling;
            }
            let index = if previous.is_null() {
                1
            } else {
                self.full_element_index(previous).saturating_add(1)
            };
            self.set_full_element_index(child, index);
        }
    }

    /// Append a node that was freshly allocated by this arena.
    ///
    /// Tree construction owns both ids and maintains the parent links, so its
    /// hot path can avoid repeating the public mutation API's defensive
    /// validation for every token. Debug builds retain the critical invariant
    /// checks without charging release parsers for them.
    #[inline]
    pub(crate) fn append_child_trusted(&mut self, parent: NodeId, child: NodeId) {
        debug_assert!(self.nodes[child.index()].first_child.is_null());
        debug_assert!(self.nodes[child.index()].last_child.is_null());
        self.append_subtree(parent, child);
    }

    /// Append a detached node, together with any subtree it owns, as the last
    /// child of `parent`. Depths and element sibling indices are left to the
    /// caller.
    #[inline]
    pub(crate) fn append_subtree(&mut self, parent: NodeId, child: NodeId) {
        let parent_index = parent.index();
        let child_index = child.index();
        debug_assert!(parent_index < self.nodes.len());
        debug_assert!(child_index < self.nodes.len());
        debug_assert_ne!(parent, child);

        let child_node = &self.nodes[child_index];
        debug_assert!(child_node.parent.is_null());
        debug_assert!(child_node.prev_sibling.is_null());
        debug_assert!(child_node.next_sibling.is_null());

        let last = self.nodes[parent_index].last_child;
        debug_assert_eq!(
            self.nodes[parent_index].first_child.is_null(),
            last.is_null()
        );
        self.nodes[child_index].parent = parent;
        if last.is_null() {
            self.nodes[parent_index].first_child = child;
        } else {
            let last_index = last.index();
            debug_assert!(last_index < self.nodes.len());
            debug_assert_eq!(self.nodes[last_index].parent, parent);
            debug_assert!(self.nodes[last_index].next_sibling.is_null());
            self.nodes[last_index].next_sibling = child;
            self.nodes[child_index].prev_sibling = last;
        }
        self.nodes[parent_index].last_child = child;
        self.nodes[parent_index].flags.set(NodeFlags::HAS_CHILDREN);
    }

    /// Insert a freshly allocated node immediately before an existing child.
    ///
    /// This is used by the tree builder's table foster-parenting recovery.
    pub(crate) fn insert_before(&mut self, parent: NodeId, reference: NodeId, child: NodeId) {
        let child_node = &self.nodes[child.index()];
        assert!(
            child_node.first_child.is_null() && child_node.last_child.is_null(),
            "insert_before: child is already linked"
        );
        self.insert_subtree_before(parent, reference, child);
    }

    /// Insert a detached node, together with any subtree it owns, immediately
    /// before an existing child. Depths and element sibling indices are left
    /// to the caller.
    pub(crate) fn insert_subtree_before(
        &mut self,
        parent: NodeId,
        reference: NodeId,
        child: NodeId,
    ) {
        let parent_index = parent.index();
        let reference_index = reference.index();
        let child_index = child.index();
        assert!(parent_index < self.nodes.len());
        assert!(reference_index < self.nodes.len());
        assert!(child_index < self.nodes.len());
        assert_eq!(self.nodes[reference_index].parent, parent);

        let child_node = &self.nodes[child_index];
        assert!(
            child_node.parent.is_null()
                && child_node.prev_sibling.is_null()
                && child_node.next_sibling.is_null(),
            "insert_before: child is already linked"
        );

        let previous = self.nodes[reference_index].prev_sibling;
        self.nodes[child_index].parent = parent;
        self.nodes[child_index].prev_sibling = previous;
        self.nodes[child_index].next_sibling = reference;
        self.nodes[reference_index].prev_sibling = child;

        if previous.is_null() {
            self.nodes[parent_index].first_child = child;
        } else {
            self.nodes[previous.index()].next_sibling = child;
        }
        self.nodes[parent_index].flags.set(NodeFlags::HAS_CHILDREN);
    }

    /// Recompute the element sibling indices of every child of `parent`.
    /// Returns the total number of element children.
    pub(crate) fn recompute_element_indices(&mut self, parent: NodeId) -> u32 {
        let mut count = 0u32;
        let mut child = self.nodes[parent.index()].first_child;
        while !child.is_null() {
            let next = self.nodes[child.index()].next_sibling;
            if self.is_element(child) {
                count = count.saturating_add(1);
                self.set_full_element_index(child, count);
            }
            child = next;
        }
        count
    }

    /// Unlink `node`, together with its subtree, from its parent and
    /// siblings. Later element siblings move up one element index.
    pub(crate) fn detach(&mut self, node: NodeId) {
        let target = &self.nodes[node.index()];
        let (parent, previous, next) = (target.parent, target.prev_sibling, target.next_sibling);
        if parent.is_null() {
            return;
        }

        if previous.is_null() {
            self.nodes[parent.index()].first_child = next;
        } else {
            self.nodes[previous.index()].next_sibling = next;
        }
        if next.is_null() {
            self.nodes[parent.index()].last_child = previous;
        } else {
            self.nodes[next.index()].prev_sibling = previous;
        }
        if self.nodes[parent.index()].first_child.is_null() {
            self.nodes[parent.index()]
                .flags
                .clear(NodeFlags::HAS_CHILDREN);
        }
        let target = &mut self.nodes[node.index()];
        target.parent = NodeId::NULL;
        target.prev_sibling = NodeId::NULL;
        target.next_sibling = NodeId::NULL;

        if self.is_element(node) {
            let mut sibling = next;
            while !sibling.is_null() {
                if self.is_element(sibling) {
                    let index = self.full_element_index(sibling).saturating_sub(1);
                    self.set_full_element_index(sibling, index);
                }
                sibling = self.nodes[sibling.index()].next_sibling;
            }
        }
    }

    /// Move every child of `from`, in order, into the childless node `to`.
    /// Element sibling indices stay valid; depths are left to the caller.
    pub(crate) fn move_children(&mut self, from: NodeId, to: NodeId) {
        debug_assert!(self.nodes[to.index()].first_child.is_null());
        let first = self.nodes[from.index()].first_child;
        let last = self.nodes[from.index()].last_child;
        let mut child = first;
        while !child.is_null() {
            let node = &mut self.nodes[child.index()];
            node.parent = to;
            child = node.next_sibling;
        }

        let source = &mut self.nodes[from.index()];
        source.first_child = NodeId::NULL;
        source.last_child = NodeId::NULL;
        source.flags.clear(NodeFlags::HAS_CHILDREN);
        if !first.is_null() {
            let target = &mut self.nodes[to.index()];
            target.first_child = first;
            target.last_child = last;
            target.flags.set(NodeFlags::HAS_CHILDREN);
        }
    }

    /// Give `node` the depth `depth` and shift its descendants to match.
    /// Allocation-free, and a no-op when the depth is already right.
    pub(crate) fn set_subtree_depth(&mut self, node: NodeId, depth: u16) {
        if self.nodes[node.index()].depth == depth {
            return;
        }
        self.nodes[node.index()].depth = depth;

        // Pre-order walk of the descendants; each parent is updated first.
        let mut current = self.nodes[node.index()].first_child;
        while !current.is_null() {
            let parent = self.nodes[current.index()].parent;
            self.nodes[current.index()].depth = self.nodes[parent.index()].depth.saturating_add(1);

            let first_child = self.nodes[current.index()].first_child;
            if !first_child.is_null() {
                current = first_child;
                continue;
            }
            let mut climb = current;
            current = NodeId::NULL;
            while climb != node {
                let entry = &self.nodes[climb.index()];
                if !entry.next_sibling.is_null() {
                    current = entry.next_sibling;
                    break;
                }
                climb = entry.parent;
            }
        }
    }

    /// Clone an element without children for adoption-agency repairs.
    ///
    /// Tag names and attributes are never modified once written, so the clone
    /// shares the original's slab ranges instead of copying them.
    pub(crate) fn clone_element_shallow(&mut self, source: NodeId, depth: u16) -> NodeId {
        let original = &self.nodes[source.index()];
        let tag = original.tag;
        let (text_offset, text_len) = (original.text_offset, original.text_len);
        let (attr_offset, attr_count) = (original.attr_offset, original.attr_count);
        let (class_hash, id_hash) = (original.class_hash, original.id_hash);

        let node = self.new_element(tag, depth);
        let target = &mut self.nodes[node.index()];
        if tag == Tag::Unknown {
            target.text_offset = text_offset;
            target.text_len = text_len;
        }
        if attr_count > 0 {
            target.attr_offset = attr_offset;
            target.attr_count = attr_count;
            target.flags.set(NodeFlags::HAS_ATTRS);
            target.class_hash = class_hash;
            target.id_hash = id_hash;
        }
        node
    }

    /// Get the name of an attribute.
    #[inline]
    pub fn attr_name(&self, attr: &Attribute) -> &str {
        checked_str(
            &self.attr_str_slab,
            attr.name_offset,
            attr.name_len,
            "attribute",
        )
    }

    /// Get the value of an attribute, or `None` for boolean attributes.
    #[inline]
    pub fn attr_value(&self, attr: &Attribute) -> Option<&str> {
        if !attr.has_value {
            return None;
        }
        Some(checked_str(
            &self.attr_str_slab,
            attr.value_offset,
            attr.value_len,
            "attribute",
        ))
    }

    /// Get the parsed attributes for a node, or an empty slice when it has no
    /// attributes.
    #[inline]
    pub fn attrs(&self, node: NodeId) -> &[Attribute] {
        let n = &self.nodes[node.index()];
        if n.attr_count == 0 {
            return &[];
        }
        let start = n.attr_offset as usize;
        let end = start + n.attr_count as usize;
        &self.attr_slab[start..end]
    }

    /// Get the direct content of a text, comment, or doctype node. Element
    /// nodes return an empty string.
    #[inline]
    pub fn text(&self, node: NodeId) -> &str {
        let n = &self.nodes[node.index()];
        // An unknown element keeps its tag name in the text fields.
        if n.text_len == 0
            || !n
                .flags
                .has(NodeFlags::IS_TEXT | NodeFlags::IS_COMMENT | NodeFlags::IS_DOCTYPE)
        {
            return "";
        }
        if n.flags.has(NodeFlags::IS_TEXT) && n.flags.has(NodeFlags::IS_TEXT_FROM_SOURCE) {
            checked_str(&self.source, n.text_offset, n.text_len, "text")
        } else {
            let text_start = n.text_offset as usize;
            let text_end = text_start
                .checked_add(n.text_len as usize)
                .expect("text range overflow");
            assert!(
                !self.pending_source_copies.iter().any(|pending| {
                    let pending_end = pending.text_offset.saturating_add(pending.len);
                    pending.text_offset < text_end && pending_end > text_start
                }),
                "source-backed text promotion is not finalized; call Arena::set_source_owned first"
            );
            checked_str(&self.text_slab, n.text_offset, n.text_len, "text")
        }
    }

    /// Get the preserved name for an unknown/custom element.
    #[inline]
    pub fn unknown_tag_name(&self, node: NodeId) -> Option<&str> {
        let n = &self.nodes[node.index()];
        if n.tag != Tag::Unknown || n.text_len == 0 {
            return None;
        }
        Some(checked_str(
            &self.text_slab,
            n.text_offset,
            n.text_len,
            "text",
        ))
    }

    /// Get a reference to a node by id.
    #[inline]
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Get a mutable reference to a node by id.
    #[inline]
    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// Total number of nodes in the arena.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the arena contains no nodes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve string metadata without trusting mutable `Node` fields or assuming
/// an `Attribute` belongs to this arena: `Attribute` values can be cloned and
/// passed to another arena through the safe public API. Both the byte range
/// and UTF-8 boundaries are checked; `what` names the slab in panic messages.
///
/// Every slab is a `String`, so an in-bounds range is valid UTF-8 exactly when
/// both ends fall on character boundaries, which is checked in O(1).
#[inline]
fn checked_str<'a>(text: &'a str, offset: u32, len: u32, what: &str) -> &'a str {
    let start = offset as usize;
    match start
        .checked_add(len as usize)
        .and_then(|end| text.get(start..end))
    {
        Some(slice) => slice,
        None => invalid_str_range(text, start, len, what),
    }
}

/// Panic for a range [`checked_str`] rejected. Kept out of line so the
/// inlined accessors stay small on the parser's hot paths.
#[cold]
#[inline(never)]
fn invalid_str_range(text: &str, start: usize, len: u32, what: &str) -> ! {
    if start
        .checked_add(len as usize)
        .is_none_or(|end| end > text.len())
    {
        panic!("arena {what} range is out of bounds");
    }
    panic!("arena {what} range is not valid UTF-8");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn new_element_and_get() {
        let mut arena = Arena::new();
        let id = arena.new_element(Tag::Div, 0);
        assert_eq!(id, NodeId(0));
        assert_eq!(arena.get(id).tag, Tag::Div);
        assert_eq!(arena.get(id).depth, 0);
    }

    #[test]
    fn new_text_and_read() {
        let mut arena = Arena::new();
        let id = arena.new_text(1, "hello world");
        assert!(arena.get(id).flags.has(NodeFlags::IS_TEXT));
        assert_eq!(arena.text(id), "hello world");
    }

    #[test]
    fn append_child_single() {
        let mut arena = Arena::new();
        let parent = arena.new_element(Tag::Div, 0);
        let child = arena.new_element(Tag::Span, 1);
        arena.append_child(parent, child);

        assert_eq!(arena.get(parent).first_child, child);
        assert_eq!(arena.get(parent).last_child, child);
        assert_eq!(arena.get(child).parent, parent);
        assert!(arena.get(child).next_sibling.is_null());
        assert!(arena.get(child).prev_sibling.is_null());
    }

    #[test]
    fn append_child_multiple() {
        let mut arena = Arena::new();
        let parent = arena.new_element(Tag::Div, 0);
        let c1 = arena.new_element(Tag::Span, 1);
        let c2 = arena.new_element(Tag::P, 1);
        let c3 = arena.new_element(Tag::A, 1);

        arena.append_child(parent, c1);
        arena.append_child(parent, c2);
        arena.append_child(parent, c3);

        assert_eq!(arena.get(parent).first_child, c1);
        assert_eq!(arena.get(parent).last_child, c3);

        assert_eq!(arena.get(c1).next_sibling, c2);
        assert!(arena.get(c1).prev_sibling.is_null());

        assert_eq!(arena.get(c2).prev_sibling, c1);
        assert_eq!(arena.get(c2).next_sibling, c3);

        assert_eq!(arena.get(c3).prev_sibling, c2);
        assert!(arena.get(c3).next_sibling.is_null());
    }

    #[test]
    fn append_child_indexes_element_siblings() {
        let mut arena = Arena::new();
        let parent = arena.new_element(Tag::Div, 0);
        let leading_text = arena.new_text(1, "a");
        let first = arena.new_element(Tag::Span, 1);
        let comment = arena.new_comment(1, "c");
        let text = arena.new_text(1, "b");
        let second = arena.new_element(Tag::P, 1);
        for child in [leading_text, first, comment, text, second] {
            arena.append_child(parent, child);
        }

        assert_eq!(arena.get(first).element_index, 1);
        assert_eq!(arena.full_element_index(first), 1);
        assert_eq!(arena.get(second).element_index, 2);
        assert_eq!(arena.full_element_index(second), 2);
        for non_element in [leading_text, comment, text] {
            assert_eq!(arena.get(non_element).element_index, 0);
        }
    }

    #[test]
    #[should_panic(expected = "append_child: child is already linked")]
    fn append_child_rejects_relinked_child() {
        let mut arena = Arena::new();
        let first_parent = arena.new_element(Tag::Div, 0);
        let second_parent = arena.new_element(Tag::Section, 0);
        let child = arena.new_element(Tag::Span, 1);

        arena.append_child(first_parent, child);
        arena.append_child(second_parent, child);
    }

    #[test]
    #[should_panic(expected = "append_child: child already owns a subtree")]
    fn append_child_rejects_cycle_through_child_subtree() {
        let mut arena = Arena::new();
        let ancestor = arena.new_element(Tag::Div, 0);
        let descendant = arena.new_element(Tag::Span, 1);

        arena.append_child(ancestor, descendant);
        arena.append_child(descendant, ancestor);
    }

    #[test]
    #[should_panic(expected = "append_child: parent and child must be distinct nodes")]
    fn append_child_rejects_aliasing_ids() {
        let mut arena = Arena::new();
        let node = arena.new_element(Tag::Div, 0);

        arena.append_child(node, node);
    }

    #[test]
    #[should_panic(expected = "append_child: parent NodeId is out of bounds")]
    fn append_child_rejects_invalid_parent_id() {
        let mut arena = Arena::new();
        let child = arena.new_element(Tag::Span, 0);

        arena.append_child(NodeId(10), child);
    }

    #[test]
    #[should_panic(expected = "append_child: child NodeId is out of bounds")]
    fn append_child_rejects_invalid_child_id() {
        let mut arena = Arena::new();
        let parent = arena.new_element(Tag::Div, 0);

        arena.append_child(parent, NodeId(10));
    }

    #[test]
    #[should_panic(expected = "arena text range is out of bounds")]
    fn text_rejects_corrupted_range() {
        let mut arena = Arena::new();
        let text = arena.new_text(0, "hello");
        let node = arena.get_mut(text);
        node.text_offset = u32::MAX;
        node.text_len = 1;

        let _ = arena.text(text);
    }

    #[test]
    #[should_panic(expected = "arena text range is not valid UTF-8")]
    fn text_rejects_corrupted_utf8_boundary() {
        let mut arena = Arena::new();
        let text = arena.new_text(0, "é");
        let node = arena.get_mut(text);
        node.text_offset = 1;
        node.text_len = 1;

        let _ = arena.text(text);
    }

    #[test]
    fn attrs_roundtrip() {
        use fhp_tokenizer::token::Attribute as TokAttr;

        let mut arena = Arena::new();
        let id = arena.new_element(Tag::A, 0);

        let tok_attrs = vec![
            TokAttr {
                name: Cow::Borrowed("href"),
                value: Some(Cow::Borrowed("https://example.com")),
            },
            TokAttr {
                name: Cow::Borrowed("class"),
                value: Some(Cow::Borrowed("link")),
            },
        ];
        arena.set_attrs(id, &tok_attrs);

        let attrs = arena.attrs(id);
        assert_eq!(attrs.len(), 2);
        assert_eq!(arena.attr_name(&attrs[0]), "href");
        assert_eq!(arena.attr_value(&attrs[0]), Some("https://example.com"));
        assert_eq!(arena.attr_name(&attrs[1]), "class");
        assert_eq!(arena.attr_value(&attrs[1]), Some("link"));
    }

    #[test]
    fn raw_attributes_cross_inline_heap_boundary_and_keep_first_value() {
        let mut empty_arena = Arena::new();
        let empty_node = empty_arena.new_element(Tag::Div, 0);
        empty_arena.set_attrs_from_raw(empty_node, "");
        assert!(empty_arena.attrs(empty_node).is_empty());

        for count in [1usize, 8, 9, 64, 1_000] {
            let mut raw = String::new();
            for index in 0..count {
                raw.push_str(&format!(" data-{index}={index}"));
            }
            raw.push_str(" DATA-0=duplicate");

            let mut arena = Arena::new();
            let node = arena.new_element(Tag::Div, 0);
            arena.set_attrs_from_raw(node, &raw);
            let attrs = arena.attrs(node);
            assert_eq!(attrs.len(), count);
            assert_eq!(arena.attr_value(&attrs[0]), Some("0"));
        }
    }

    #[test]
    fn selector_hashes_are_computed_during_attribute_write() {
        let mut arena = Arena::new();
        let node = arena.new_element(Tag::Div, 0);
        arena.set_attrs_from_raw(node, " class='one two' id=main CLASS=ignored ID=ignored");

        assert_eq!(
            arena.get(node).class_hash,
            class_bloom_bit(b"one") | class_bloom_bit(b"two")
        );
        assert_eq!(arena.get(node).id_hash, selector_hash(b"main"));
    }

    #[test]
    fn class_hash_uses_only_html_space_characters() {
        let mut arena = Arena::new();
        let node = arena.new_element(Tag::Div, 1);
        arena.set_attrs_from_raw(node, "class='alpha\x0Bbeta'");

        assert_eq!(
            arena.get(node).class_hash,
            class_bloom_bit(b"alpha\x0Bbeta")
        );
    }

    #[test]
    fn duplicate_raw_value_is_not_written_to_the_attribute_slab() {
        let mut arena = Arena::new();
        let node = arena.new_element(Tag::Div, 0);
        arena.set_attrs_from_raw(node, " title=first TITLE='&amp;&amp;&amp;&amp;' ");

        let attrs = arena.attrs(node);
        assert_eq!(attrs.len(), 1);
        assert_eq!(arena.attr_name(&attrs[0]), "title");
        assert_eq!(arena.attr_value(&attrs[0]), Some("first"));
        assert_eq!(arena.attr_str_slab.len(), "titlefirst".len());
    }

    #[test]
    #[should_panic(expected = "arena attribute range is not valid UTF-8")]
    fn attr_name_rejects_attribute_from_another_arena_at_invalid_boundary() {
        use fhp_tokenizer::token::Attribute as TokAttr;

        let mut source = Arena::new();
        let source_node = source.new_element(Tag::Div, 0);
        source.set_attrs(
            source_node,
            &[TokAttr {
                name: Cow::Borrowed("é"),
                value: None,
            }],
        );
        let foreign_attr = source.attrs(source_node)[0].clone();

        let mut target = Arena::new();
        let target_node = target.new_element(Tag::Div, 0);
        target.set_attrs(
            target_node,
            &[TokAttr {
                name: Cow::Borrowed("aé"),
                value: None,
            }],
        );

        let _ = target.attr_name(&foreign_attr);
    }

    #[test]
    #[should_panic(expected = "arena attribute range is not valid UTF-8")]
    fn attr_value_rejects_attribute_from_another_arena_at_invalid_boundary() {
        use fhp_tokenizer::token::Attribute as TokAttr;

        let mut source = Arena::new();
        let source_node = source.new_element(Tag::Div, 0);
        source.set_attrs(
            source_node,
            &[TokAttr {
                name: Cow::Borrowed("x"),
                value: Some(Cow::Borrowed("é")),
            }],
        );
        let foreign_attr = source.attrs(source_node)[0].clone();

        let mut target = Arena::new();
        let target_node = target.new_element(Tag::Div, 0);
        target.set_attrs(
            target_node,
            &[TokAttr {
                name: Cow::Borrowed("x"),
                value: Some(Cow::Borrowed("aé")),
            }],
        );

        let _ = target.attr_value(&foreign_attr);
    }

    #[test]
    fn empty_attrs() {
        let mut arena = Arena::new();
        let id = arena.new_element(Tag::Div, 0);
        assert!(arena.attrs(id).is_empty());
    }

    #[test]
    fn arena_len() {
        let mut arena = Arena::new();
        assert!(arena.is_empty());
        arena.new_element(Tag::Div, 0);
        arena.new_text(1, "hi");
        assert_eq!(arena.len(), 2);
    }
}
