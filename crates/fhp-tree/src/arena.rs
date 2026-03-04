//! Arena allocator for DOM nodes, text, and attributes.
//!
//! All nodes live in a single contiguous `Vec<Node>`, giving cache-friendly
//! traversal. Text content and attributes are stored in separate slabs,
//! referenced by offset+length from each [`Node`](crate::node::Node).

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
    name_len: u16,
    value_offset: u32,
    value_len: u16,
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
        }
    }

    /// Create a new arena with pre-allocated capacity.
    pub fn with_capacity(node_cap: usize, text_cap: usize, attr_cap: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(node_cap),
            text_slab: Vec::with_capacity(text_cap),
            attr_slab: Vec::with_capacity(attr_cap),
            attr_str_slab: Vec::with_capacity(attr_cap * 8),
            source: Vec::new(),
        }
    }

    /// Allocate a new element node and return its id.
    pub fn new_element(&mut self, tag: Tag, depth: u16) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new_element(tag, depth));
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

    /// Set attributes for a node from tokenizer attributes.
    pub fn set_attrs(&mut self, node: NodeId, attrs: &[fhp_tokenizer::token::Attribute<'_>]) {
        if attrs.is_empty() {
            return;
        }
        let offset = self.attr_slab.len() as u32;
        let count = attrs.len().min(255) as u8;

        for attr in &attrs[..count as usize] {
            let name_offset = self.attr_str_slab.len() as u32;
            self.attr_str_slab.extend_from_slice(attr.name.as_bytes());
            let name_len = attr.name.len() as u16;

            let (value_offset, value_len) = if let Some(ref v) = attr.value {
                let vo = self.attr_str_slab.len() as u32;
                self.attr_str_slab.extend_from_slice(v.as_bytes());
                (vo, v.len() as u16)
            } else {
                (0, 0)
            };

            self.attr_slab.push(Attribute {
                name_offset,
                name_len,
                value_offset,
                value_len,
            });
        }

        let n = &mut self.nodes[node.index()];
        n.attr_offset = offset;
        n.attr_count = count;
        n.flags.set(NodeFlags::HAS_ATTRS);
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

        let slab_offset = self.attr_slab.len() as u32;
        let mut count: u8 = 0;
        let mut pos = 0;

        loop {
            // Skip whitespace.
            while pos < end && is_attr_whitespace(bytes[pos]) {
                pos += 1;
            }
            if pos >= end || count == 255 {
                break;
            }

            // Attribute name.
            let name_start = pos;
            while pos < end && !is_attr_name_end(bytes[pos]) {
                pos += 1;
            }
            if name_start == pos {
                // Not a valid name char — skip it.
                pos += 1;
                continue;
            }

            let name_slab_offset = self.attr_str_slab.len() as u32;
            self.attr_str_slab
                .extend_from_slice(&bytes[name_start..pos]);
            let name_len = (pos - name_start) as u16;

            // Skip whitespace.
            while pos < end && is_attr_whitespace(bytes[pos]) {
                pos += 1;
            }

            // Check for `=`.
            if pos < end && bytes[pos] == b'=' {
                pos += 1;

                // Skip whitespace.
                while pos < end && is_attr_whitespace(bytes[pos]) {
                    pos += 1;
                }

                // Parse value.
                if pos < end && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                    // Quoted value.
                    let quote = bytes[pos];
                    pos += 1;
                    let val_start = pos;
                    while pos < end && bytes[pos] != quote {
                        pos += 1;
                    }
                    let val_end = pos;
                    if pos < end {
                        pos += 1; // skip closing quote
                    }
                    let raw_value = &attr_raw[val_start..val_end];
                    let (value_offset, value_len) = self.push_attr_value(raw_value);
                    self.attr_slab.push(Attribute {
                        name_offset: name_slab_offset,
                        name_len,
                        value_offset,
                        value_len,
                    });
                } else {
                    // Unquoted value.
                    let val_start = pos;
                    while pos < end && !is_attr_whitespace(bytes[pos]) && bytes[pos] != b'>' {
                        pos += 1;
                    }
                    let raw_value = &attr_raw[val_start..pos];
                    let (value_offset, value_len) = self.push_attr_value(raw_value);
                    self.attr_slab.push(Attribute {
                        name_offset: name_slab_offset,
                        name_len,
                        value_offset,
                        value_len,
                    });
                }
            } else {
                // Boolean attribute (no value).
                self.attr_slab.push(Attribute {
                    name_offset: name_slab_offset,
                    name_len,
                    value_offset: 0,
                    value_len: 0,
                });
            }

            count += 1;
        }

        if count > 0 {
            let n = &mut self.nodes[node.index()];
            n.attr_offset = slab_offset;
            n.attr_count = count;
            n.flags.set(NodeFlags::HAS_ATTRS);
        }
    }

    /// Write an attribute value to the string slab, with optional entity decoding.
    #[cfg(feature = "entity-decode")]
    fn push_attr_value(&mut self, raw_value: &str) -> (u32, u16) {
        let offset = self.attr_str_slab.len() as u32;
        let decoded = fhp_tokenizer::entity::decode_entities(raw_value);
        self.attr_str_slab.extend_from_slice(decoded.as_bytes());
        (offset, decoded.len() as u16)
    }

    /// Write an attribute value to the string slab (no entity decoding).
    #[cfg(not(feature = "entity-decode"))]
    fn push_attr_value(&mut self, raw_value: &str) -> (u32, u16) {
        let offset = self.attr_str_slab.len() as u32;
        self.attr_str_slab.extend_from_slice(raw_value.as_bytes());
        (offset, raw_value.len() as u16)
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
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let parent_last = self.nodes[parent.index()].last_child;

        // Set child's parent.
        self.nodes[child.index()].parent = parent;

        if parent_last.is_null() {
            // First child.
            self.nodes[parent.index()].first_child = child;
        } else {
            // Link after current last child.
            self.nodes[parent_last.index()].next_sibling = child;
            self.nodes[child.index()].prev_sibling = parent_last;
        }

        self.nodes[parent.index()].last_child = child;
        self.nodes[parent.index()]
            .flags
            .set(NodeFlags::HAS_CHILDREN);
    }

    /// Get the name of an attribute.
    #[inline]
    pub fn attr_name(&self, attr: &Attribute) -> &str {
        let start = attr.name_offset as usize;
        let end = start + attr.name_len as usize;
        // SAFETY: attr names are sourced from tokenizer `&str` slices (valid UTF-8).
        unsafe { std::str::from_utf8_unchecked(&self.attr_str_slab[start..end]) }
    }

    /// Get the value of an attribute, or `None` for boolean attributes.
    #[inline]
    pub fn attr_value(&self, attr: &Attribute) -> Option<&str> {
        if attr.value_len == 0 {
            return None;
        }
        let start = attr.value_offset as usize;
        let end = start + attr.value_len as usize;
        // SAFETY: attr values are sourced from tokenizer `&str` slices (valid UTF-8).
        Some(unsafe { std::str::from_utf8_unchecked(&self.attr_str_slab[start..end]) })
    }

    /// Get the attributes for a node.
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
        let start = n.text_offset as usize;
        let end = start + n.text_len as usize;
        if n.flags.has(NodeFlags::IS_TEXT) && n.flags.has(NodeFlags::IS_TEXT_FROM_SOURCE) {
            // SAFETY: source is a copy of the input &str (valid UTF-8).
            unsafe { std::str::from_utf8_unchecked(&self.source[start..end]) }
        } else {
            // SAFETY: text slab is always valid UTF-8 (we only write str bytes).
            unsafe { std::str::from_utf8_unchecked(&self.text_slab[start..end]) }
        }
    }

    /// Get the preserved name for an unknown/custom element.
    #[inline]
    pub fn unknown_tag_name(&self, node: NodeId) -> Option<&str> {
        let n = &self.nodes[node.index()];
        if n.tag != Tag::Unknown || n.text_len == 0 {
            return None;
        }
        let start = n.text_offset as usize;
        let end = start + n.text_len as usize;
        // SAFETY: the tag name is sourced from tokenizer `&str` slices.
        Some(unsafe { std::str::from_utf8_unchecked(&self.text_slab[start..end]) })
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
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Check if a byte terminates an attribute name.
#[inline(always)]
fn is_attr_name_end(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'=' | b'/' | b'>')
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
