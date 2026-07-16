//! Arena allocator for DOM nodes, text, and attributes.
//!
//! All nodes live in a single contiguous `Vec<Node>`, giving cache-friendly
//! traversal. Text content and attributes are stored in separate slabs,
//! referenced by offset+length from each [`Node`](crate::node::Node).

use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};
use std::mem::MaybeUninit;

use fhp_core::hash::{class_bloom_bit, selector_hash};
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
const INLINE_SEEN_ATTRIBUTES: usize = 8;
const ASCII_CI_FINGERPRINT_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

#[derive(Clone, Copy)]
struct SeenAttribute<'a> {
    fingerprint: u64,
    name: &'a [u8],
}

/// A no-op hasher for the already-hashed attribute fingerprints.
#[derive(Default)]
struct FingerprintHasher(u64);

impl Hasher for FingerprintHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

type SeenAttributeSet = HashSet<u64, BuildHasherDefault<FingerprintHasher>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsertResult {
    New,
    Duplicate,
    FingerprintCollision,
}

struct SeenAttributes<'a> {
    bloom: u64,
    // `MaybeUninit` avoids zeroing 192 bytes for the overwhelmingly common
    // one-to-three-attribute case. The initialized prefix is tracked by
    // `inline_len` and `SeenAttribute` is `Copy`, so no drop work is needed.
    inline: [MaybeUninit<SeenAttribute<'a>>; INLINE_SEEN_ATTRIBUTES],
    inline_len: usize,
    heap: Option<SeenAttributeSet>,
}

impl<'a> SeenAttributes<'a> {
    #[inline]
    fn with_first(name: &'a [u8]) -> Self {
        let fingerprint = ascii_ci_fingerprint(name);
        let mut inline = [MaybeUninit::uninit(); INLINE_SEEN_ATTRIBUTES];
        inline[0].write(SeenAttribute { fingerprint, name });
        Self {
            bloom: fingerprint_bloom(fingerprint),
            inline,
            inline_len: 1,
            heap: None,
        }
    }

    #[inline]
    fn insert(&mut self, name: &'a [u8]) -> InsertResult {
        let fingerprint = ascii_ci_fingerprint(name);
        self.insert_hashed(name, fingerprint)
    }

    /// Variant for callers that already visited every name byte while parsing.
    #[inline]
    fn insert_hashed(&mut self, name: &'a [u8], fingerprint: u64) -> InsertResult {
        let candidate = SeenAttribute { fingerprint, name };

        if let Some(heap) = &mut self.heap {
            return if heap.insert(fingerprint) {
                InsertResult::New
            } else {
                InsertResult::FingerprintCollision
            };
        }

        let bloom = fingerprint_bloom(fingerprint);
        if self.bloom & bloom == bloom
            && self
                .inline_entries()
                .iter()
                .any(|seen| seen.fingerprint == fingerprint && seen.name.eq_ignore_ascii_case(name))
        {
            return InsertResult::Duplicate;
        }

        self.bloom |= bloom;
        if self.inline_len < INLINE_SEEN_ATTRIBUTES {
            self.inline[self.inline_len].write(candidate);
            self.inline_len += 1;
        } else {
            let mut heap = SeenAttributeSet::with_capacity_and_hasher(
                INLINE_SEEN_ATTRIBUTES * 2,
                BuildHasherDefault::default(),
            );
            for &seen in self.inline_entries() {
                heap.insert(seen.fingerprint);
            }
            let fingerprint_was_new = heap.insert(fingerprint);
            self.heap = Some(heap);
            if !fingerprint_was_new {
                return InsertResult::FingerprintCollision;
            }
        }
        InsertResult::New
    }

    #[inline]
    fn inline_entries(&self) -> &[SeenAttribute<'a>] {
        // SAFETY: only the prefix `[..inline_len]` is exposed, and every entry
        // in that prefix is initialized immediately before `inline_len` grows.
        unsafe {
            std::slice::from_raw_parts(
                self.inline.as_ptr().cast::<SeenAttribute<'a>>(),
                self.inline_len,
            )
        }
    }
}

#[inline]
fn ascii_ci_fingerprint(name: &[u8]) -> u64 {
    let mut hash = ASCII_CI_FINGERPRINT_OFFSET;
    for &byte in name {
        hash = ascii_ci_fingerprint_step(hash, byte);
    }
    hash
}

#[inline(always)]
fn ascii_ci_fingerprint_step(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte.to_ascii_lowercase())).wrapping_mul(0x0000_0100_0000_01b3)
}

#[inline]
fn fingerprint_bloom(fingerprint: u64) -> u64 {
    (1u64 << (fingerprint & 63)) | (1u64 << ((fingerprint >> 32) & 63))
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
        for class in value.split(|byte| byte.is_ascii_whitespace()) {
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
    pub(crate) text_slab: Vec<u8>,
    /// All attributes in insertion order.
    pub(crate) attr_slab: Vec<Attribute>,
    /// All attribute name and value bytes concatenated.
    pub(crate) attr_str_slab: Vec<u8>,
    /// Owned copy of the original input source.
    ///
    /// Text nodes that reference entity-free (borrowed) regions of the input
    /// store offsets into this buffer via [`NodeFlags::IS_TEXT_FROM_SOURCE`].
    /// Empty for streaming parsers.
    pub(crate) source: Vec<u8>,
    /// Pre-built tag → NodeId index, populated during tree construction.
    ///
    /// Indexed by `Tag as u8`. Each bucket contains NodeIds of elements with
    /// that tag, in document order. Built inline during `open_tag` to avoid
    /// a separate DFS pass.
    pub(crate) tag_index: Option<Box<[Vec<NodeId>; TAG_INDEX_SIZE]>>,
}

impl Arena {
    /// Create a new empty arena.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            text_slab: Vec::new(),
            attr_slab: Vec::new(),
            attr_str_slab: Vec::new(),
            source: Vec::new(),
            tag_index: None,
        }
    }

    /// Create a new arena with pre-allocated capacity.
    pub fn with_capacity(node_cap: usize, text_cap: usize, attr_cap: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(node_cap),
            text_slab: Vec::with_capacity(text_cap),
            attr_slab: Vec::with_capacity(attr_cap),
            attr_str_slab: Vec::with_capacity(attr_cap * 32),
            source: Vec::new(),
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

    /// Get the pre-built tag index, if it was enabled during construction.
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
        self.text_slab.extend_from_slice(tag_name.as_bytes());
        let n = &mut self.nodes[node.index()];
        n.text_offset = offset;
        n.text_len = len;
    }

    /// Allocate a new text node, storing content in the text slab.
    pub fn new_text(&mut self, depth: u16, text: &str) -> NodeId {
        let offset = self.text_slab.len() as u32;
        let len = text.len() as u32;
        self.text_slab.extend_from_slice(text.as_bytes());
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
        self.source = input.as_bytes().to_vec();
    }

    /// Transfer an already-owned `String` as the source buffer (zero copy).
    ///
    /// When the caller owns the input `String` (e.g., from an HTTP response),
    /// this avoids the memcpy that [`set_source`](Arena::set_source) performs.
    pub fn set_source_owned(&mut self, source: String) {
        self.source = source.into_bytes();
    }

    /// Allocate a new comment node, storing content in the text slab.
    pub fn new_comment(&mut self, depth: u16, text: &str) -> NodeId {
        let offset = self.text_slab.len() as u32;
        let len = text.len() as u32;
        self.text_slab.extend_from_slice(text.as_bytes());
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_comment(depth, offset, len));
        id
    }

    /// Allocate a new doctype node, storing content in the text slab.
    pub fn new_doctype(&mut self, depth: u16, text: &str) -> NodeId {
        let offset = self.text_slab.len() as u32;
        let len = text.len() as u32;
        self.text_slab.extend_from_slice(text.as_bytes());
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_doctype(depth, offset, len));
        id
    }

    #[inline]
    fn attr_name_exists(&self, offset: u32, count: u16, name: &[u8]) -> bool {
        let start = offset as usize;
        let end = start + usize::from(count);
        self.attr_slab[start..end].iter().any(|attr| {
            let name_start = attr.name_offset as usize;
            let name_end = name_start + attr.name_len as usize;
            self.attr_str_slab[name_start..name_end].eq_ignore_ascii_case(name)
        })
    }

    /// Set attributes for a node from tokenizer attributes.
    pub fn set_attrs(&mut self, node: NodeId, attrs: &[fhp_tokenizer::token::Attribute<'_>]) {
        if attrs.is_empty() {
            return;
        }
        let offset = self.attr_slab.len() as u32;
        let mut count = 0u16;
        let mut seen: Option<SeenAttributes<'_>> = None;
        let mut first_name: Option<&[u8]> = None;
        let mut class_hash = 0u64;
        let mut id_hash = 0u32;

        for attr in attrs {
            let name = attr.name.as_bytes();
            if count == u16::MAX {
                continue;
            }
            let insert_result = if let Some(seen) = &mut seen {
                seen.insert(name)
            } else if count == 0 {
                InsertResult::New
            } else if self.attr_name_exists(offset, count, name) {
                InsertResult::Duplicate
            } else {
                let tracker = seen.insert(SeenAttributes::with_first(
                    first_name.expect("the first accepted attribute name must be tracked"),
                ));
                tracker.insert(name)
            };
            let duplicate = match insert_result {
                InsertResult::New => false,
                InsertResult::Duplicate => true,
                InsertResult::FingerprintCollision => self.attr_name_exists(offset, count, name),
            };
            if duplicate {
                continue;
            }
            first_name.get_or_insert(name);
            let name_offset = self.attr_str_slab.len() as u32;
            self.attr_str_slab.extend_from_slice(name);
            let name_len = attr.name.len() as u32;

            let (value_offset, value_len, has_value) = if let Some(ref v) = attr.value {
                update_selector_hashes(name, Some(v.as_bytes()), &mut class_hash, &mut id_hash);
                let vo = self.attr_str_slab.len() as u32;
                self.attr_str_slab.extend_from_slice(v.as_bytes());
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
    /// are pushed to `attr_slab` in a single pass.
    pub fn set_attrs_from_raw(&mut self, node: NodeId, attr_raw: &str) {
        let bytes = attr_raw.as_bytes();
        let end = bytes.len();
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
        let mut seen: Option<SeenAttributes<'_>> = None;
        let mut first_name: Option<&[u8]> = None;
        let mut class_hash = 0u64;
        let mut id_hash = 0u32;
        let mut pos = 0;

        loop {
            // Skip whitespace using fast byte scan.
            pos += bytes[pos..end]
                .iter()
                .position(|&b| !is_attr_whitespace(b))
                .unwrap_or(end - pos);
            if pos >= end || count == u16::MAX {
                break;
            }

            // Attribute name.
            let name_start = pos;
            let hash_during_parse = seen.is_some();
            let mut fingerprint = ASCII_CI_FINGERPRINT_OFFSET;
            if hash_during_parse {
                while pos < end && !is_attr_name_end(bytes[pos]) {
                    fingerprint = ascii_ci_fingerprint_step(fingerprint, bytes[pos]);
                    pos += 1;
                }
            } else {
                while pos < end && !is_attr_name_end(bytes[pos]) {
                    pos += 1;
                }
            }
            if name_start == pos {
                // Not a valid name char — skip it.
                pos += 1;
                continue;
            }

            let name = &bytes[name_start..pos];
            let insert_result = if let Some(seen) = &mut seen {
                debug_assert!(hash_during_parse);
                seen.insert_hashed(name, fingerprint)
            } else if count == 0 {
                InsertResult::New
            } else if self.attr_name_exists(slab_offset, count, name) {
                InsertResult::Duplicate
            } else {
                let tracker = seen.insert(SeenAttributes::with_first(
                    first_name.expect("the first accepted attribute name must be tracked"),
                ));
                tracker.insert(name)
            };
            let duplicate = match insert_result {
                InsertResult::New => false,
                InsertResult::Duplicate => true,
                InsertResult::FingerprintCollision => {
                    self.attr_name_exists(slab_offset, count, name)
                }
            };
            let name_slab_offset = self.attr_str_slab.len() as u32;
            let name_len = (pos - name_start) as u32;
            if !duplicate {
                first_name.get_or_insert(name);
                self.attr_str_slab.extend_from_slice(name);
            }

            // Skip whitespace using fast byte scan.
            pos += bytes[pos..end]
                .iter()
                .position(|&b| !is_attr_whitespace(b))
                .unwrap_or(end - pos);

            // Check for `=`.
            if pos < end && bytes[pos] == b'=' {
                pos += 1;

                // Skip whitespace using fast byte scan.
                pos += bytes[pos..end]
                    .iter()
                    .position(|&b| !is_attr_whitespace(b))
                    .unwrap_or(end - pos);

                // Parse value.
                if pos < end && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                    // Quoted value — use memchr for SIMD-accelerated scan.
                    let quote = bytes[pos];
                    pos += 1;
                    let val_start = pos;
                    if let Some(found) = memchr::memchr(quote, &bytes[pos..end]) {
                        pos += found;
                    } else {
                        pos = end;
                    }
                    let val_end = pos;
                    if pos < end {
                        pos += 1; // skip closing quote
                    }
                    if !duplicate {
                        let raw_value = &attr_raw[val_start..val_end];
                        let (value_offset, value_len) = self.push_attr_value(raw_value);
                        update_selector_hashes(
                            name,
                            Some(
                                &self.attr_str_slab[value_offset as usize
                                    ..value_offset as usize + value_len as usize],
                            ),
                            &mut class_hash,
                            &mut id_hash,
                        );
                        self.attr_slab.push(Attribute {
                            name_offset: name_slab_offset,
                            name_len,
                            value_offset,
                            value_len,
                            has_value: true,
                        });
                    }
                } else {
                    // Unquoted value.
                    let val_start = pos;
                    while pos < end && !is_attr_whitespace(bytes[pos]) && bytes[pos] != b'>' {
                        pos += 1;
                    }
                    if !duplicate {
                        let raw_value = &attr_raw[val_start..pos];
                        let (value_offset, value_len) = self.push_attr_value(raw_value);
                        update_selector_hashes(
                            name,
                            Some(
                                &self.attr_str_slab[value_offset as usize
                                    ..value_offset as usize + value_len as usize],
                            ),
                            &mut class_hash,
                            &mut id_hash,
                        );
                        self.attr_slab.push(Attribute {
                            name_offset: name_slab_offset,
                            name_len,
                            value_offset,
                            value_len,
                            has_value: true,
                        });
                    }
                }
            } else if !duplicate {
                // Boolean attribute (no value).
                self.attr_slab.push(Attribute {
                    name_offset: name_slab_offset,
                    name_len,
                    value_offset: 0,
                    value_len: 0,
                    has_value: false,
                });
            }

            if !duplicate {
                count += 1;
            }
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
        self.attr_str_slab.extend_from_slice(decoded.as_bytes());
        (offset, decoded.len() as u32)
    }

    /// Write an attribute value to the string slab (no entity decoding).
    #[cfg(not(feature = "entity-decode"))]
    fn push_attr_value(&mut self, raw_value: &str) -> (u32, u32) {
        let offset = self.attr_str_slab.len() as u32;
        self.attr_str_slab.extend_from_slice(raw_value.as_bytes());
        (offset, raw_value.len() as u32)
    }

    /// Set the 1-based element sibling index for a node.
    ///
    /// Called by [`TreeBuilder`](crate::builder::TreeBuilder) after appending an element child.
    #[inline]
    pub fn set_element_index(&mut self, node: NodeId, index: u16) {
        let target = &mut self.nodes[node.index()];
        target.attr_raw_offset = u32::from(index);
        target.element_index = index;
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
    /// next_sibling.
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
    }

    /// Append a node that was freshly allocated by this arena.
    ///
    /// Tree construction owns both ids and maintains the parent links, so its
    /// hot path can avoid repeating the public mutation API's defensive
    /// validation for every token. Debug builds retain the critical invariant
    /// checks without charging release parsers for them.
    #[inline]
    pub(crate) fn append_child_trusted(&mut self, parent: NodeId, child: NodeId) {
        let parent_index = parent.index();
        let child_index = child.index();
        debug_assert!(parent_index < self.nodes.len());
        debug_assert!(child_index < self.nodes.len());
        debug_assert_ne!(parent, child);

        let child_node = &self.nodes[child_index];
        debug_assert!(child_node.parent.is_null());
        debug_assert!(child_node.prev_sibling.is_null());
        debug_assert!(child_node.next_sibling.is_null());
        debug_assert!(child_node.first_child.is_null());
        debug_assert!(child_node.last_child.is_null());

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
                && child_node.next_sibling.is_null()
                && child_node.first_child.is_null()
                && child_node.last_child.is_null(),
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

    /// Recompute element sibling indices after an insertion before an existing
    /// child. Returns the total number of element children.
    pub(crate) fn recompute_element_indices(&mut self, parent: NodeId) -> u32 {
        let mut count = 0u32;
        let mut child = self.nodes[parent.index()].first_child;
        while !child.is_null() {
            let next = self.nodes[child.index()].next_sibling;
            let flags = self.nodes[child.index()].flags;
            if !flags.has(NodeFlags::IS_TEXT)
                && !flags.has(NodeFlags::IS_COMMENT)
                && !flags.has(NodeFlags::IS_DOCTYPE)
            {
                count = count.saturating_add(1);
                self.set_full_element_index(child, count);
            }
            child = next;
        }
        count
    }

    /// Clone an element without children for formatting-element reconstruction.
    pub(crate) fn clone_element_shallow(&mut self, source: NodeId, depth: u16) -> NodeId {
        let original = &self.nodes[source.index()];
        let tag = original.tag;
        let attr_offset = original.attr_offset;
        let attr_count = original.attr_count;
        let class_hash = original.class_hash;
        let id_hash = original.id_hash;
        let unknown_name = if tag == Tag::Unknown && original.text_len > 0 {
            let start = original.text_offset as usize;
            let end = start + original.text_len as usize;
            Some(self.text_slab[start..end].to_vec())
        } else {
            None
        };

        let node = self.new_element(tag, depth);
        if let Some(name) = unknown_name {
            let offset = self.text_slab.len() as u32;
            self.text_slab.extend_from_slice(&name);
            let target = &mut self.nodes[node.index()];
            target.text_offset = offset;
            target.text_len = name.len() as u32;
        }

        if attr_count > 0 {
            let source_start = attr_offset as usize;
            let source_end = source_start + attr_count as usize;
            let new_offset = self.attr_slab.len() as u32;
            self.attr_slab.extend_from_within(source_start..source_end);
            let target = &mut self.nodes[node.index()];
            target.attr_offset = new_offset;
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
        checked_attr_slice(&self.attr_str_slab, attr.name_offset, attr.name_len)
    }

    /// Get the value of an attribute, or `None` for boolean attributes.
    #[inline]
    pub fn attr_value(&self, attr: &Attribute) -> Option<&str> {
        if !attr.has_value {
            return None;
        }
        Some(checked_attr_slice(
            &self.attr_str_slab,
            attr.value_offset,
            attr.value_len,
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

    /// Get the text content for a node (direct text, not recursive).
    #[inline]
    pub fn text(&self, node: NodeId) -> &str {
        let n = &self.nodes[node.index()];
        if n.text_len == 0 {
            return "";
        }
        if n.flags.has(NodeFlags::IS_TEXT) && n.flags.has(NodeFlags::IS_TEXT_FROM_SOURCE) {
            checked_text_slice(&self.source, n.text_offset, n.text_len)
        } else {
            checked_text_slice(&self.text_slab, n.text_offset, n.text_len)
        }
    }

    /// Get the preserved name for an unknown/custom element.
    #[inline]
    pub fn unknown_tag_name(&self, node: NodeId) -> Option<&str> {
        let n = &self.nodes[node.index()];
        if n.tag != Tag::Unknown || n.text_len == 0 {
            return None;
        }
        Some(checked_text_slice(
            &self.text_slab,
            n.text_offset,
            n.text_len,
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

/// Check if a byte is ASCII whitespace (for attribute parsing).
#[inline(always)]
fn is_attr_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0C' | b'\r')
}

/// Check if a byte terminates an attribute name.
#[inline(always)]
fn is_attr_name_end(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\x0C' | b'\r' | b'=' | b'/' | b'>'
    )
}

/// Resolve arena-owned text metadata without trusting mutable `Node` fields.
#[inline]
fn checked_text_slice(bytes: &[u8], offset: u32, len: u32) -> &str {
    let start = offset as usize;
    let end = start
        .checked_add(len as usize)
        .expect("arena text range is out of bounds");
    let slice = bytes
        .get(start..end)
        .expect("arena text range is out of bounds");
    std::str::from_utf8(slice).expect("arena text range is not valid UTF-8")
}

/// Resolve an attribute's string metadata without assuming it belongs to this
/// arena. `Attribute` values can be cloned and passed to another arena through
/// the safe public API, so both the byte range and UTF-8 boundaries must be
/// checked here.
#[inline]
fn checked_attr_slice(bytes: &[u8], offset: u32, len: u32) -> &str {
    let start = offset as usize;
    let end = start
        .checked_add(len as usize)
        .expect("arena attribute range is out of bounds");
    let slice = bytes
        .get(start..end)
        .expect("arena attribute range is out of bounds");
    std::str::from_utf8(slice).expect("arena attribute range is not valid UTF-8")
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
    fn seen_attribute_fingerprint_collision_is_not_a_false_duplicate() {
        let candidate = b"beta";
        let fingerprint = ascii_ci_fingerprint(candidate);
        let mut seen = SeenAttributes::with_first(b"alpha");
        seen.bloom = fingerprint_bloom(fingerprint);
        seen.inline[0].write(SeenAttribute {
            fingerprint,
            name: b"alpha",
        });

        assert_eq!(seen.insert(candidate), InsertResult::New);
        assert_eq!(seen.insert(b"BETA"), InsertResult::Duplicate);

        let mut heap_seen = SeenAttributes::with_first(b"alpha");
        heap_seen.bloom = fingerprint_bloom(fingerprint);
        heap_seen.inline[0].write(SeenAttribute {
            fingerprint,
            name: b"alpha",
        });
        for (index, name) in [
            b"bravo".as_slice(),
            b"charlie".as_slice(),
            b"delta".as_slice(),
            b"echo".as_slice(),
            b"foxtrot".as_slice(),
            b"golf".as_slice(),
            b"hotel".as_slice(),
            b"india".as_slice(),
        ]
        .into_iter()
        .enumerate()
        {
            let expected = if index + 1 < INLINE_SEEN_ATTRIBUTES {
                InsertResult::New
            } else {
                InsertResult::FingerprintCollision
            };
            assert_eq!(heap_seen.insert_hashed(name, fingerprint), expected);
        }
        assert!(heap_seen.heap.is_some());
        assert_eq!(
            heap_seen.insert_hashed(candidate, fingerprint),
            InsertResult::FingerprintCollision
        );
        assert_eq!(
            heap_seen.insert_hashed(b"BETA", fingerprint),
            InsertResult::FingerprintCollision
        );
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
