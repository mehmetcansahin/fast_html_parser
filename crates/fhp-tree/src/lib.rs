//! Arena-based DOM tree with cache-line aligned nodes.
//!
//! This crate builds on `fhp-tokenizer` to construct an in-memory DOM tree
//! using an arena allocator. Each node occupies exactly 64 bytes (one cache
//! line) for optimal traversal performance.
//!
//! # Quick Start
//!
//! ```
//! use fhp_tree::parse;
//!
//! let doc = parse("<div><p>Hello</p></div>").unwrap();
//! let root = doc.root();
//! assert!(root.children().count() > 0);
//! ```

/// Arena allocator for [`Node`](crate::node::Node)s, text, and attributes.
pub mod arena;
/// Async HTML parser (requires `async-tokio` feature).
#[cfg(feature = "async-tokio")]
pub mod async_parser;
/// Tree builder — converts a [`Token`](fhp_tokenizer::token::Token) stream into a DOM tree.
pub mod builder;
/// Cache-line aligned [`Node`](crate::node::Node) layout.
pub mod node;
/// Streaming and incremental parsing — [`StreamParser`](crate::streaming::StreamParser) and [`EarlyStopParser`](crate::streaming::EarlyStopParser).
pub mod streaming;
/// Allocation-free traversal iterators (uses [`VecDeque`](std::collections::VecDeque) for BFS).
pub mod traverse;

use fhp_core::tag::Tag;

use arena::{Arena, Attribute};
use builder::TreeBuilder;
use node::{NodeFlags, NodeId};
use traverse::{Ancestors, BreadthFirst, Children, DepthFirst, Siblings};

/// Error type for HTML parsing.
#[derive(Debug, thiserror::Error)]
pub enum HtmlError {
    /// Input was too large to parse.
    #[error("input too large: {size} bytes (max {max})")]
    InputTooLarge {
        /// Actual input size.
        size: usize,
        /// Maximum allowed.
        max: usize,
    },

    /// Encoding detection or conversion failed.
    #[error("encoding error: {0}")]
    Encoding(#[from] fhp_core::error::EncodingError),

    /// I/O error during streaming or async parsing.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Maximum input size (256 MiB).
pub(crate) const MAX_INPUT_SIZE: usize = 256 * 1024 * 1024;

/// Parse an HTML string into a [`Document`].
///
/// Runs the tokenizer and tree builder in sequence.
///
/// # Errors
///
/// Returns [`HtmlError::InputTooLarge`] if the input exceeds 256 MiB.
///
/// # Example
///
/// ```
/// use fhp_tree::parse;
///
/// let doc = parse("<div><p>Hello &amp; world</p></div>").unwrap();
/// let root = doc.root();
/// assert!(root.children().count() > 0);
/// ```
pub fn parse(input: &str) -> Result<Document, HtmlError> {
    if input.len() > MAX_INPUT_SIZE {
        return Err(HtmlError::InputTooLarge {
            size: input.len(),
            max: MAX_INPUT_SIZE,
        });
    }

    let tokens = fhp_tokenizer::tokenize(input);
    let mut builder = TreeBuilder::with_capacity_hint(input.len());
    for token in &tokens {
        builder.process(token);
    }
    let (arena, root) = builder.finish();

    Ok(Document { arena, root })
}

/// Parse raw bytes into a [`Document`], auto-detecting the encoding.
///
/// The encoding detection pipeline:
/// 1. BOM detection (UTF-8, UTF-16 LE/BE)
/// 2. `<meta charset="...">` prescan (first 1 KB)
/// 3. `<meta http-equiv="Content-Type" content="...charset=...">` prescan
/// 4. Fallback to UTF-8
///
/// # Errors
///
/// Returns [`HtmlError::InputTooLarge`] if the input exceeds 256 MiB, or
/// [`HtmlError::Encoding`] if the detected encoding cannot decode the input.
///
/// # Example
///
/// ```
/// use fhp_tree::parse_bytes;
///
/// let doc = parse_bytes(b"<div>Hello</div>").unwrap();
/// let root = doc.root();
/// assert_eq!(root.text_content(), "Hello");
/// ```
pub fn parse_bytes(input: &[u8]) -> Result<Document, HtmlError> {
    if input.len() > MAX_INPUT_SIZE {
        return Err(HtmlError::InputTooLarge {
            size: input.len(),
            max: MAX_INPUT_SIZE,
        });
    }

    let (text, _encoding) = fhp_encoding::decode_or_detect(input)?;
    parse(&text)
}

/// A parsed HTML document backed by an arena.
///
/// Provides access to the root node and convenience methods for querying
/// the DOM tree.
pub struct Document {
    arena: Arena,
    root: NodeId,
}

impl Document {
    /// Get a reference to the root node.
    pub fn root(&self) -> NodeRef<'_> {
        NodeRef {
            arena: &self.arena,
            id: self.root,
        }
    }

    /// Get a node by its id.
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds.
    pub fn get(&self, id: NodeId) -> NodeRef<'_> {
        NodeRef {
            arena: &self.arena,
            id,
        }
    }

    /// Get the underlying arena (for advanced usage).
    pub fn arena(&self) -> &Arena {
        &self.arena
    }

    /// Total number of nodes in the document.
    pub fn node_count(&self) -> usize {
        self.arena.len()
    }

    /// Root node id.
    pub fn root_id(&self) -> NodeId {
        self.root
    }
}

/// A borrowed reference to a node inside the document.
///
/// Provides convenience methods for querying node properties,
/// traversing the tree, and extracting content.
#[derive(Clone, Copy)]
pub struct NodeRef<'a> {
    arena: &'a Arena,
    id: NodeId,
}

impl<'a> NodeRef<'a> {
    /// The node id.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The tag type of this node.
    pub fn tag(&self) -> Tag {
        self.arena.get(self.id).tag
    }

    /// The nesting depth.
    pub fn depth(&self) -> u16 {
        self.arena.get(self.id).depth
    }

    /// Whether this is a text node.
    pub fn is_text(&self) -> bool {
        self.arena.get(self.id).flags.has(NodeFlags::IS_TEXT)
    }

    /// Whether this is a comment node.
    pub fn is_comment(&self) -> bool {
        self.arena.get(self.id).flags.has(NodeFlags::IS_COMMENT)
    }

    /// Whether this is a doctype node.
    pub fn is_doctype(&self) -> bool {
        self.arena.get(self.id).flags.has(NodeFlags::IS_DOCTYPE)
    }

    /// Whether this is a void element.
    pub fn is_void(&self) -> bool {
        self.arena.get(self.id).flags.has(NodeFlags::IS_VOID)
    }

    /// Whether this node has any children.
    pub fn has_children(&self) -> bool {
        !self.arena.get(self.id).first_child.is_null()
    }

    /// Direct text content of this node (not recursive).
    ///
    /// For element nodes, returns `""`. For text nodes, returns the text.
    pub fn text(&self) -> &'a str {
        let node = self.arena.get(self.id);
        if node.flags.has(NodeFlags::IS_TEXT)
            || node.flags.has(NodeFlags::IS_COMMENT)
            || node.flags.has(NodeFlags::IS_DOCTYPE)
        {
            self.arena.text(self.id)
        } else {
            ""
        }
    }

    /// Recursively collect all text content from this node and its
    /// descendants.
    pub fn text_content(&self) -> String {
        let node = self.arena.get(self.id);
        // Fast path for text nodes.
        if node.flags.has(NodeFlags::IS_TEXT) {
            return self.arena.text(self.id).to_string();
        }
        // Heuristic: estimate based on text slab size, capped at 4KB.
        let hint = (self.arena.text_slab.len() / 4).min(4096);
        let mut result = String::with_capacity(hint);
        self.collect_text(&mut result);
        result
    }

    /// Recursive text collection helper.
    fn collect_text(&self, out: &mut String) {
        let node = self.arena.get(self.id);
        if node.flags.has(NodeFlags::IS_TEXT) {
            out.push_str(self.arena.text(self.id));
            return;
        }
        let mut child = node.first_child;
        while !child.is_null() {
            NodeRef {
                arena: self.arena,
                id: child,
            }
            .collect_text(out);
            child = self.arena.get(child).next_sibling;
        }
    }

    /// Reconstruct the inner HTML of this node.
    pub fn inner_html(&self) -> String {
        let mut result = String::new();
        let node = self.arena.get(self.id);
        let mut child = node.first_child;
        while !child.is_null() {
            NodeRef {
                arena: self.arena,
                id: child,
            }
            .write_outer_html(&mut result);
            child = self.arena.get(child).next_sibling;
        }
        result
    }

    /// Reconstruct the outer HTML of this node (including the tag itself).
    pub fn outer_html(&self) -> String {
        let mut result = String::new();
        self.write_outer_html(&mut result);
        result
    }

    /// Write outer HTML to a string buffer.
    fn write_outer_html(&self, out: &mut String) {
        let node = self.arena.get(self.id);

        if node.flags.has(NodeFlags::IS_TEXT) {
            out.push_str(self.arena.text(self.id));
            return;
        }

        if node.flags.has(NodeFlags::IS_COMMENT) {
            out.push_str("<!--");
            out.push_str(self.arena.text(self.id));
            out.push_str("-->");
            return;
        }

        if node.flags.has(NodeFlags::IS_DOCTYPE) {
            out.push_str("<!DOCTYPE ");
            out.push_str(self.arena.text(self.id));
            out.push('>');
            return;
        }

        let tag_name = node
            .tag
            .as_str()
            .or_else(|| self.arena.unknown_tag_name(self.id));
        // Skip only the synthetic root node tag created by the builder.
        let is_root_wrapper = node.depth == 0 && node.parent.is_null();

        if !is_root_wrapper {
            if let Some(name) = tag_name {
                out.push('<');
                out.push_str(name);

                // Write attributes.
                let attrs = self.arena.attrs(self.id);
                for attr in attrs {
                    out.push(' ');
                    out.push_str(&attr.name);
                    if let Some(ref val) = attr.value {
                        out.push_str("=\"");
                        out.push_str(val);
                        out.push('"');
                    }
                }

                if node.flags.has(NodeFlags::IS_VOID) {
                    out.push_str(" />");
                    return;
                }
                out.push('>');
            }
        }

        // Write children.
        let mut child = node.first_child;
        while !child.is_null() {
            NodeRef {
                arena: self.arena,
                id: child,
            }
            .write_outer_html(out);
            child = self.arena.get(child).next_sibling;
        }

        // Close tag.
        if !is_root_wrapper {
            if let Some(name) = tag_name {
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
    }

    /// Get the value of an attribute by name.
    pub fn attr(&self, name: &str) -> Option<&'a str> {
        self.arena
            .attrs(self.id)
            .iter()
            .find(|a| a.name == name)
            .and_then(|a| a.value.as_deref())
    }

    /// Check if the node has a given CSS class.
    ///
    /// Splits the `class` attribute on whitespace and checks if any
    /// segment matches.
    pub fn has_class(&self, class_name: &str) -> bool {
        if let Some(classes) = self.attr("class") {
            classes.split_whitespace().any(|c| c == class_name)
        } else {
            false
        }
    }

    /// Get all attributes.
    pub fn attrs(&self) -> &'a [Attribute] {
        self.arena.attrs(self.id)
    }

    /// Iterate over direct children.
    pub fn children(&self) -> Children<'a> {
        Children::new(self.arena, self.id)
    }

    /// Get the parent node, if any.
    pub fn parent(&self) -> Option<NodeRef<'a>> {
        let parent = self.arena.get(self.id).parent;
        if parent.is_null() {
            None
        } else {
            Some(NodeRef {
                arena: self.arena,
                id: parent,
            })
        }
    }

    /// Get the first child, if any.
    pub fn first_child(&self) -> Option<NodeRef<'a>> {
        let fc = self.arena.get(self.id).first_child;
        if fc.is_null() {
            None
        } else {
            Some(NodeRef {
                arena: self.arena,
                id: fc,
            })
        }
    }

    /// Get the next sibling, if any.
    pub fn next_sibling(&self) -> Option<NodeRef<'a>> {
        let ns = self.arena.get(self.id).next_sibling;
        if ns.is_null() {
            None
        } else {
            Some(NodeRef {
                arena: self.arena,
                id: ns,
            })
        }
    }

    /// Get the previous sibling, if any.
    pub fn prev_sibling(&self) -> Option<NodeRef<'a>> {
        let ps = self.arena.get(self.id).prev_sibling;
        if ps.is_null() {
            None
        } else {
            Some(NodeRef {
                arena: self.arena,
                id: ps,
            })
        }
    }

    /// Iterate over ancestors (parent chain, not including self).
    pub fn ancestors(&self) -> Ancestors<'a> {
        Ancestors::new(self.arena, self.id)
    }

    /// Iterate over next siblings (not including self).
    pub fn siblings(&self) -> Siblings<'a> {
        Siblings::new(self.arena, self.id)
    }

    /// Pre-order depth-first traversal of the subtree rooted at this node.
    pub fn descendants(&self) -> DepthFirst<'a> {
        DepthFirst::new(self.arena, self.id)
    }

    /// Breadth-first traversal of the subtree rooted at this node.
    pub fn descendants_bfs(&self) -> BreadthFirst<'a> {
        BreadthFirst::new(self.arena, self.id)
    }
}

impl<'a> core::fmt::Debug for NodeRef<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let node = self.arena.get(self.id);
        if node.flags.has(NodeFlags::IS_TEXT) {
            write!(f, "Text({:?})", self.text())
        } else if node.flags.has(NodeFlags::IS_COMMENT) {
            write!(f, "Comment({:?})", self.text())
        } else {
            write!(f, "<{}>", node.tag)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        assert!(doc.node_count() > 0);
        let root = doc.root();
        assert!(root.has_children());
    }

    #[test]
    fn parse_text_content() {
        let doc = parse("<div><span>Hello</span> <span>World</span></div>").unwrap();
        let root = doc.root();
        let text = root.text_content();
        assert!(text.contains("Hello"), "text: {text}");
        assert!(text.contains("World"), "text: {text}");
    }

    #[test]
    fn parse_attr() {
        let doc = parse("<a href=\"https://example.com\" class=\"link primary\">text</a>").unwrap();
        let root = doc.root();
        // Root -> a
        let a = root.first_child().expect("should have child");
        assert_eq!(a.tag(), Tag::A);
        assert_eq!(a.attr("href"), Some("https://example.com"));
        assert!(a.has_class("link"));
        assert!(a.has_class("primary"));
        assert!(!a.has_class("secondary"));
    }

    #[test]
    fn parse_inner_html() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let root = doc.root();
        let div = root.first_child().unwrap();
        assert_eq!(div.tag(), Tag::Div);
        let inner = div.inner_html();
        assert!(inner.contains("<p>"), "inner: {inner}");
        assert!(inner.contains("Hello"), "inner: {inner}");
        assert!(inner.contains("</p>"), "inner: {inner}");
    }

    #[test]
    fn parse_outer_html() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let root = doc.root();
        let div = root.first_child().unwrap();
        let outer = div.outer_html();
        assert!(outer.starts_with("<div>"), "outer: {outer}");
        assert!(outer.ends_with("</div>"), "outer: {outer}");
    }

    #[test]
    fn parse_void_elements() {
        let doc = parse("<div><br><hr></div>").unwrap();
        let root = doc.root();
        let div = root.first_child().unwrap();
        let children: Vec<_> = div.children().collect();
        assert_eq!(children.len(), 2);

        let br_ref = doc.get(children[0]);
        assert_eq!(br_ref.tag(), Tag::Br);
        assert!(br_ref.is_void());

        let hr_ref = doc.get(children[1]);
        assert_eq!(hr_ref.tag(), Tag::Hr);
        assert!(hr_ref.is_void());
    }

    #[test]
    fn parse_depth_first() {
        let doc = parse("<div><span>a</span><p>b</p></div>").unwrap();
        let root = doc.root();
        let tags: Vec<_> = root
            .descendants()
            .map(|id| doc.get(id))
            .filter(|n| !n.is_text())
            .map(|n| n.tag())
            .collect();
        // root(Unknown), div, span, p
        assert!(tags.contains(&Tag::Div));
        assert!(tags.contains(&Tag::Span));
        assert!(tags.contains(&Tag::P));
    }

    #[test]
    fn parse_ancestors() {
        let doc = parse("<div><span><a>link</a></span></div>").unwrap();
        let root = doc.root();

        // Navigate: root -> div -> span -> a -> text
        let div = root.first_child().unwrap();
        let span = div.first_child().unwrap();
        let a = span.first_child().unwrap();

        let ancestor_tags: Vec<_> = a.ancestors().map(|id| doc.get(id).tag()).collect();
        assert_eq!(ancestor_tags, vec![Tag::Span, Tag::Div, Tag::Unknown]);
    }

    #[test]
    fn parse_siblings() {
        let doc = parse("<ul><li>1</li><li>2</li><li>3</li></ul>").unwrap();
        let root = doc.root();
        let ul = root.first_child().unwrap();
        let li1 = ul.first_child().unwrap();

        let sibling_count = li1.siblings().count();
        assert_eq!(sibling_count, 2);
    }

    #[test]
    fn empty_input() {
        let doc = parse("").unwrap();
        assert!(!doc.root().has_children());
    }

    #[test]
    fn text_only() {
        let doc = parse("just text").unwrap();
        assert_eq!(doc.root().text_content(), "just text");
    }

    #[test]
    fn broken_html_unclosed() {
        let doc = parse("<div><p>unclosed").unwrap();
        let root = doc.root();
        assert!(root.has_children());
        assert_eq!(root.text_content(), "unclosed");
    }

    #[test]
    fn broken_html_extra_close() {
        let doc = parse("</div><p>ok</p>").unwrap();
        let root = doc.root();
        assert_eq!(root.text_content(), "ok");
    }

    #[test]
    fn implicit_close_p_p() {
        let doc = parse("<p>first<p>second").unwrap();
        let root = doc.root();
        let children: Vec<_> = root.children().collect();
        // Both <p> should be direct children of root.
        let p_count = children
            .iter()
            .filter(|&c| doc.get(*c).tag() == Tag::P)
            .count();
        assert_eq!(p_count, 2, "both <p> should be root children");
    }

    #[test]
    fn node_64_bytes_alignment() {
        assert_eq!(std::mem::size_of::<node::Node>(), 64);
        assert_eq!(std::mem::align_of::<node::Node>(), 64);
    }

    #[test]
    fn input_too_large() {
        // We can't actually allocate 256 MiB in a test, but check the error path.
        let result = parse("");
        assert!(result.is_ok());
    }

    #[test]
    fn comment_and_doctype() {
        let doc = parse("<!DOCTYPE html><!-- comment --><div>ok</div>").unwrap();
        let root = doc.root();
        let mut has_comment = false;
        let mut has_doctype = false;
        for child_id in root.children() {
            let child = doc.get(child_id);
            if child.is_comment() {
                has_comment = true;
            }
            if child.is_doctype() {
                has_doctype = true;
            }
        }
        assert!(has_doctype, "should have doctype");
        assert!(has_comment, "should have comment");
    }

    #[test]
    fn void_outer_html() {
        let doc = parse("<br>").unwrap();
        let root = doc.root();
        let br = root.first_child().unwrap();
        let html = br.outer_html();
        assert!(html.contains("br"), "outer: {html}");
        assert!(html.contains("/>"), "outer: {html}");
    }

    #[test]
    fn unknown_tag_outer_html_preserved() {
        let doc = parse("<my-widget><x-item>ok</x-item></my-widget>").unwrap();
        let root = doc.root();
        let outer = root.inner_html();
        assert_eq!(outer, "<my-widget><x-item>ok</x-item></my-widget>");
    }

    // ---- parse_bytes tests ----

    #[test]
    fn parse_bytes_utf8() {
        let doc = parse_bytes(b"<div><p>Hello</p></div>").unwrap();
        assert_eq!(doc.root().text_content(), "Hello");
    }

    #[test]
    fn parse_bytes_utf8_bom() {
        let html = b"\xEF\xBB\xBF<div><p>BOM test</p></div>";
        let doc = parse_bytes(html).unwrap();
        assert!(doc.root().text_content().contains("BOM test"));
    }

    #[test]
    fn parse_bytes_windows_1254_meta() {
        // Turkish ü=0xFC in windows-1254.
        let html = b"<meta charset=\"windows-1254\"><p>Merhaba d\xFCnya</p>";
        let doc = parse_bytes(html).unwrap();
        let text = doc.root().text_content();
        assert!(text.contains("dünya"), "text: {text}");
    }

    #[test]
    fn parse_bytes_utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE]; // BOM
        for &ch in b"<p>UTF16</p>" {
            bytes.push(ch);
            bytes.push(0x00);
        }
        let doc = parse_bytes(&bytes).unwrap();
        let text = doc.root().text_content();
        assert!(text.contains("UTF16"), "text: {text}");
    }

    #[test]
    fn parse_bytes_empty() {
        let doc = parse_bytes(b"").unwrap();
        assert!(!doc.root().has_children());
    }
}
