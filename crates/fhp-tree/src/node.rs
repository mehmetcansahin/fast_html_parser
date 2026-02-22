//! Cache-line aligned node layout and supporting types.
//!
//! Each [`Node`] occupies exactly 64 bytes (one cache line), split into a hot
//! first half (tree links, tag, depth) and a cold second half (attribute
//! metadata, padding).

use fhp_core::tag::Tag;

/// Index into the arena's node vector.
///
/// Uses `u32` internally — supports up to ~4 billion nodes.
/// [`NodeId::NULL`] represents the absence of a node (like `None`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Sentinel value representing "no node".
    pub const NULL: NodeId = NodeId(u32::MAX);

    /// Returns `true` if this is the null sentinel.
    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == u32::MAX
    }

    /// Returns the index as `usize` for array indexing.
    #[inline(always)]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl core::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_null() {
            write!(f, "NodeId(NULL)")
        } else {
            write!(f, "NodeId({})", self.0)
        }
    }
}

/// Bitflags for node properties.
///
/// Packed into a single `u8` for minimal space inside the [`Node`] struct.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeFlags(u8);

impl NodeFlags {
    /// The element is a void element (no children allowed).
    pub const IS_VOID: u8 = 1 << 0;
    /// The element has at least one child.
    pub const HAS_CHILDREN: u8 = 1 << 1;
    /// The element has at least one attribute.
    pub const HAS_ATTRS: u8 = 1 << 2;
    /// The element was self-closing in source (`<br/>`).
    pub const IS_SELF_CLOSING: u8 = 1 << 3;
    /// This node is a text node (not an element).
    pub const IS_TEXT: u8 = 1 << 4;
    /// This node is a comment node.
    pub const IS_COMMENT: u8 = 1 << 5;
    /// This node is a doctype node.
    pub const IS_DOCTYPE: u8 = 1 << 6;
    /// This node is a CDATA section.
    pub const IS_CDATA: u8 = 1 << 7;

    /// Create empty flags.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Set a flag bit.
    #[inline(always)]
    pub fn set(&mut self, flag: u8) {
        self.0 |= flag;
    }

    /// Check if a flag bit is set.
    #[inline(always)]
    pub const fn has(self, flag: u8) -> bool {
        self.0 & flag != 0
    }
}

/// A single DOM node in the arena, exactly 64 bytes (one cache line).
///
/// The first 32 bytes contain frequently accessed fields (tree links, tag,
/// depth). The second 32 bytes contain less-frequently accessed data
/// (attribute metadata).
///
/// # Layout
///
/// | Offset | Bytes | Field |
/// |--------|-------|-------|
/// | 0      | 1     | `tag` |
/// | 1      | 1     | `flags` |
/// | 2      | 2     | `depth` |
/// | 4      | 4     | `parent` |
/// | 8      | 4     | `first_child` |
/// | 12     | 4     | `next_sibling` |
/// | 16     | 4     | `last_child` |
/// | 20     | 4     | `prev_sibling` |
/// | 24     | 4     | `text_offset` |
/// | 28     | 4     | `text_len` |
/// | 32     | 4     | `attr_offset` |
/// | 36     | 1     | `attr_count` |
/// | 37     | 27    | `_padding` |
#[repr(C, align(64))]
pub struct Node {
    // === Hot (first 32 bytes) ===
    /// Interned tag type.
    pub tag: Tag,
    /// Bitflags for node properties.
    pub flags: NodeFlags,
    /// Nesting depth from root (root = 0).
    pub depth: u16,
    /// Parent node, or `NodeId::NULL` for root.
    pub parent: NodeId,
    /// First child, or `NodeId::NULL` if no children.
    pub first_child: NodeId,
    /// Next sibling, or `NodeId::NULL` if last.
    pub next_sibling: NodeId,
    /// Last child, or `NodeId::NULL` if no children.
    pub last_child: NodeId,
    /// Previous sibling, or `NodeId::NULL` if first.
    pub prev_sibling: NodeId,
    /// Byte offset into the text slab.
    pub text_offset: u32,
    /// Length in bytes of the text content in the slab.
    pub text_len: u32,

    // === Cold (second 32 bytes) ===
    /// Byte offset into the attribute slab.
    pub attr_offset: u32,
    /// Number of attributes.
    pub attr_count: u8,
    /// Padding to fill the cache line.
    pub _padding: [u8; 27],
}

impl Node {
    /// Create a new element node with all links set to NULL.
    pub fn new_element(tag: Tag, depth: u16) -> Self {
        let mut flags = NodeFlags::empty();
        if tag.is_void() {
            flags.set(NodeFlags::IS_VOID);
        }
        Self {
            tag,
            flags,
            depth,
            parent: NodeId::NULL,
            first_child: NodeId::NULL,
            next_sibling: NodeId::NULL,
            last_child: NodeId::NULL,
            prev_sibling: NodeId::NULL,
            text_offset: 0,
            text_len: 0,
            attr_offset: 0,
            attr_count: 0,
            _padding: [0; 27],
        }
    }

    /// Create a new text node.
    pub fn new_text(depth: u16, text_offset: u32, text_len: u32) -> Self {
        let mut flags = NodeFlags::empty();
        flags.set(NodeFlags::IS_TEXT);
        Self {
            tag: Tag::Unknown,
            flags,
            depth,
            parent: NodeId::NULL,
            first_child: NodeId::NULL,
            next_sibling: NodeId::NULL,
            last_child: NodeId::NULL,
            prev_sibling: NodeId::NULL,
            text_offset,
            text_len,
            attr_offset: 0,
            attr_count: 0,
            _padding: [0; 27],
        }
    }

    /// Create a new comment node.
    pub fn new_comment(depth: u16, text_offset: u32, text_len: u32) -> Self {
        let mut flags = NodeFlags::empty();
        flags.set(NodeFlags::IS_COMMENT);
        Self {
            tag: Tag::Unknown,
            flags,
            depth,
            parent: NodeId::NULL,
            first_child: NodeId::NULL,
            next_sibling: NodeId::NULL,
            last_child: NodeId::NULL,
            prev_sibling: NodeId::NULL,
            text_offset,
            text_len,
            attr_offset: 0,
            attr_count: 0,
            _padding: [0; 27],
        }
    }

    /// Create a new doctype node.
    pub fn new_doctype(depth: u16, text_offset: u32, text_len: u32) -> Self {
        let mut flags = NodeFlags::empty();
        flags.set(NodeFlags::IS_DOCTYPE);
        Self {
            tag: Tag::Unknown,
            flags,
            depth,
            parent: NodeId::NULL,
            first_child: NodeId::NULL,
            next_sibling: NodeId::NULL,
            last_child: NodeId::NULL,
            prev_sibling: NodeId::NULL,
            text_offset,
            text_len,
            attr_offset: 0,
            attr_count: 0,
            _padding: [0; 27],
        }
    }
}

impl core::fmt::Debug for Node {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Node")
            .field("tag", &self.tag)
            .field("flags", &self.flags)
            .field("depth", &self.depth)
            .field("parent", &self.parent)
            .field("first_child", &self.first_child)
            .field("next_sibling", &self.next_sibling)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_is_64_bytes() {
        assert_eq!(
            std::mem::size_of::<Node>(),
            64,
            "Node must be exactly 64 bytes (one cache line)"
        );
    }

    #[test]
    fn node_alignment_is_64() {
        assert_eq!(
            std::mem::align_of::<Node>(),
            64,
            "Node must be 64-byte aligned"
        );
    }

    #[test]
    fn node_id_null() {
        assert!(NodeId::NULL.is_null());
        assert!(!NodeId(0).is_null());
        assert!(!NodeId(42).is_null());
    }

    #[test]
    fn node_id_debug() {
        assert_eq!(format!("{:?}", NodeId::NULL), "NodeId(NULL)");
        assert_eq!(format!("{:?}", NodeId(5)), "NodeId(5)");
    }

    #[test]
    fn node_flags() {
        let mut flags = NodeFlags::empty();
        assert!(!flags.has(NodeFlags::IS_VOID));
        assert!(!flags.has(NodeFlags::HAS_CHILDREN));

        flags.set(NodeFlags::IS_VOID);
        assert!(flags.has(NodeFlags::IS_VOID));
        assert!(!flags.has(NodeFlags::HAS_CHILDREN));

        flags.set(NodeFlags::HAS_CHILDREN);
        assert!(flags.has(NodeFlags::IS_VOID));
        assert!(flags.has(NodeFlags::HAS_CHILDREN));
    }

    #[test]
    fn new_element_sets_void_flag() {
        let br = Node::new_element(Tag::Br, 0);
        assert!(br.flags.has(NodeFlags::IS_VOID));

        let div = Node::new_element(Tag::Div, 0);
        assert!(!div.flags.has(NodeFlags::IS_VOID));
    }

    #[test]
    fn new_text_sets_text_flag() {
        let text = Node::new_text(1, 0, 5);
        assert!(text.flags.has(NodeFlags::IS_TEXT));
        assert_eq!(text.text_offset, 0);
        assert_eq!(text.text_len, 5);
    }
}
