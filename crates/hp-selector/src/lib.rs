//! CSS selector engine for the SIMD-optimized HTML parser.
//!
//! Provides CSS selector parsing, matching, and a convenience API
//! for querying a parsed [`hp_tree::Document`].
//!
//! # Quick Start
//!
//! ```
//! use hp_tree::parse;
//! use hp_selector::Selectable;
//!
//! let doc = parse("<div><p class=\"intro\">Hello</p></div>").unwrap();
//! let sel = doc.select("p.intro").unwrap();
//! assert_eq!(sel.len(), 1);
//! assert_eq!(sel.text(), "Hello");
//! ```
//!
//! # Supported Selectors
//!
//! - Type: `div`, `p`, `span`
//! - Class: `.class`
//! - ID: `#id`
//! - Universal: `*`
//! - Attribute: `[attr]`, `[attr=val]`, `[attr~=val]`, `[attr^=val]`, `[attr$=val]`, `[attr*=val]`
//! - Pseudo: `:first-child`, `:last-child`, `:nth-child(an+b)`, `:not(sel)`
//! - Compound: `div.class#id[attr]`
//! - Combinator: `A B`, `A > B`, `A + B`, `A ~ B`
//! - Comma list: `div, span`

/// CSS selector AST types.
pub mod ast;
/// Bloom filter for ancestor pre-filtering.
pub mod bloom;
/// Right-to-left matching engine.
pub mod matcher;
/// CSS selector parser.
pub mod parser;

use std::collections::HashMap;

use hp_core::error::SelectorError;
use hp_core::tag::Tag;
use hp_tree::node::{NodeFlags, NodeId};
use hp_tree::{Document, NodeRef};

use matcher::{select_all_list, select_first_list};
use parser::parse_selector;

/// A collection of matched nodes from a selector query.
///
/// Provides iteration, text extraction, attribute access, and
/// sub-selection (chaining).
pub struct Selection<'a> {
    doc: &'a Document,
    nodes: Vec<NodeId>,
}

impl<'a> Selection<'a> {
    /// Create a new selection from a document and node list.
    fn new(doc: &'a Document, nodes: Vec<NodeId>) -> Self {
        Self { doc, nodes }
    }

    /// Get the first matched node.
    pub fn first(&self) -> Option<NodeRef<'a>> {
        self.nodes.first().map(|&id| self.doc.get(id))
    }

    /// Iterate over matched nodes as [`NodeRef`].
    pub fn iter(&self) -> impl Iterator<Item = NodeRef<'a>> + '_ {
        self.nodes.iter().map(|&id| self.doc.get(id))
    }

    /// Iterate over matched node ids.
    pub fn node_ids(&self) -> &[NodeId] {
        &self.nodes
    }

    /// Collect text content from all matched nodes.
    pub fn text(&self) -> String {
        self.iter()
            .map(|n| n.text_content())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get an attribute value from the first matched node.
    pub fn attr(&self, name: &str) -> Option<&'a str> {
        self.first()?.attr(name)
    }

    /// Get inner HTML from the first matched node.
    pub fn inner_html(&self) -> String {
        self.first().map(|n| n.inner_html()).unwrap_or_default()
    }

    /// Number of matched nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the selection is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Sub-select: run a CSS selector within the matched nodes.
    ///
    /// Each matched node is used as a subtree root, and results are
    /// deduplicated in document order.
    pub fn select(&self, css: &str) -> Result<Selection<'a>, SelectorError> {
        let list = parse_selector(css)?;
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &node_id in &self.nodes {
            for id in select_all_list(self.doc.arena(), node_id, &list) {
                if seen.insert(id) {
                    results.push(id);
                }
            }
        }
        Ok(Selection::new(self.doc, results))
    }
}

impl<'a> IntoIterator for &'a Selection<'a> {
    type Item = NodeRef<'a>;
    type IntoIter = SelectionIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        SelectionIter {
            doc: self.doc,
            inner: self.nodes.iter(),
        }
    }
}

/// Iterator over [`Selection`] results.
pub struct SelectionIter<'a> {
    doc: &'a Document,
    inner: std::slice::Iter<'a, NodeId>,
}

impl<'a> Iterator for SelectionIter<'a> {
    type Item = NodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|&id| self.doc.get(id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a> ExactSizeIterator for SelectionIter<'a> {}

/// Extension trait that adds CSS selector methods to [`Document`].
///
/// Import this trait to use `.select()` and convenience methods on a document.
pub trait Selectable {
    /// Select all nodes matching a CSS selector.
    ///
    /// # Errors
    ///
    /// Returns [`SelectorError::Invalid`] if the selector syntax is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use hp_tree::parse;
    /// use hp_selector::Selectable;
    ///
    /// let doc = parse("<div><p>Hello</p></div>").unwrap();
    /// let sel = doc.select("p").unwrap();
    /// assert_eq!(sel.len(), 1);
    /// ```
    fn select(&self, css: &str) -> Result<Selection<'_>, SelectorError>;

    /// Select the first node matching a CSS selector.
    fn select_first(&self, css: &str) -> Result<Option<NodeRef<'_>>, SelectorError>;

    /// Find all elements with the given tag.
    fn find_by_tag(&self, tag: Tag) -> Selection<'_>;

    /// Find an element by its `id` attribute.
    ///
    /// Scans all nodes linearly. For repeated lookups, build a
    /// [`DocumentIndex`] instead.
    fn find_by_id(&self, id: &str) -> Option<NodeRef<'_>>;

    /// Find all elements with the given CSS class.
    fn find_by_class(&self, class: &str) -> Selection<'_>;

    /// Find all elements with an attribute matching a value.
    fn find_by_attr(&self, name: &str, value: &str) -> Selection<'_>;
}

impl Selectable for Document {
    fn select(&self, css: &str) -> Result<Selection<'_>, SelectorError> {
        let list = parse_selector(css)?;
        let nodes = select_all_list(self.arena(), self.root_id(), &list);
        Ok(Selection::new(self, nodes))
    }

    fn select_first(&self, css: &str) -> Result<Option<NodeRef<'_>>, SelectorError> {
        let list = parse_selector(css)?;
        let node = select_first_list(self.arena(), self.root_id(), &list);
        Ok(node.map(|id| self.get(id)))
    }

    fn find_by_tag(&self, tag: Tag) -> Selection<'_> {
        let arena = self.arena();
        let mut nodes = Vec::new();
        for i in 0..arena.len() {
            let id = NodeId(i as u32);
            let n = arena.get(id);
            if n.tag == tag
                && !n.flags.has(NodeFlags::IS_TEXT)
                && !n.flags.has(NodeFlags::IS_COMMENT)
                && !n.flags.has(NodeFlags::IS_DOCTYPE)
            {
                nodes.push(id);
            }
        }
        Selection::new(self, nodes)
    }

    fn find_by_id(&self, id: &str) -> Option<NodeRef<'_>> {
        let arena = self.arena();
        for i in 0..arena.len() {
            let node_id = NodeId(i as u32);
            let attrs = arena.attrs(node_id);
            for attr in attrs {
                if attr.name == "id" && attr.value.as_deref() == Some(id) {
                    return Some(self.get(node_id));
                }
            }
        }
        None
    }

    fn find_by_class(&self, class: &str) -> Selection<'_> {
        let arena = self.arena();
        let mut nodes = Vec::new();
        for i in 0..arena.len() {
            let id = NodeId(i as u32);
            let attrs = arena.attrs(id);
            for attr in attrs {
                if attr.name == "class" {
                    if let Some(ref val) = attr.value {
                        if val.split_whitespace().any(|c| c == class) {
                            nodes.push(id);
                            break;
                        }
                    }
                }
            }
        }
        Selection::new(self, nodes)
    }

    fn find_by_attr(&self, name: &str, value: &str) -> Selection<'_> {
        let arena = self.arena();
        let mut nodes = Vec::new();
        for i in 0..arena.len() {
            let id = NodeId(i as u32);
            let attrs = arena.attrs(id);
            for attr in attrs {
                if attr.name == name && attr.value.as_deref() == Some(value) {
                    nodes.push(id);
                    break;
                }
            }
        }
        Selection::new(self, nodes)
    }
}

/// Pre-built index for O(1) id lookups.
///
/// Build once, reuse for many lookups.
pub struct DocumentIndex {
    id_map: HashMap<String, NodeId>,
}

impl DocumentIndex {
    /// Build an index from a document by scanning all nodes.
    pub fn build(doc: &Document) -> Self {
        let arena = doc.arena();
        let mut id_map = HashMap::new();
        for i in 0..arena.len() {
            let id = NodeId(i as u32);
            let attrs = arena.attrs(id);
            for attr in attrs {
                if attr.name == "id" {
                    if let Some(ref val) = attr.value {
                        id_map.insert(val.clone(), id);
                    }
                }
            }
        }
        Self { id_map }
    }

    /// Look up a node by its `id` attribute in O(1).
    pub fn find_by_id<'a>(&self, doc: &'a Document, id: &str) -> Option<NodeRef<'a>> {
        self.id_map.get(id).map(|&node_id| doc.get(node_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hp_tree::parse;

    #[test]
    fn select_basic() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let sel = doc.select("p").unwrap();
        assert_eq!(sel.len(), 1);
        assert_eq!(sel.text(), "Hello");
    }

    #[test]
    fn select_first_found() {
        let doc = parse("<div><p>a</p><p>b</p></div>").unwrap();
        let first = doc.select_first("p").unwrap();
        assert!(first.is_some());
        assert_eq!(first.unwrap().text_content(), "a");
    }

    #[test]
    fn select_chaining() {
        let doc = parse("<ul><li><a>1</a></li><li><a>2</a></li></ul>").unwrap();
        let lis = doc.select("li").unwrap();
        assert_eq!(lis.len(), 2);
        let links = lis.select("a").unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links.text(), "12");
    }

    #[test]
    fn find_by_tag_works() {
        let doc = parse("<div><span>a</span><span>b</span></div>").unwrap();
        let sel = doc.find_by_tag(Tag::Span);
        assert_eq!(sel.len(), 2);
    }

    #[test]
    fn find_by_id_works() {
        let doc = parse("<div id=\"main\">x</div><div>y</div>").unwrap();
        let node = doc.find_by_id("main");
        assert!(node.is_some());
        assert_eq!(node.unwrap().text_content(), "x");
    }

    #[test]
    fn find_by_id_missing() {
        let doc = parse("<div>x</div>").unwrap();
        assert!(doc.find_by_id("nope").is_none());
    }

    #[test]
    fn find_by_class_works() {
        let doc = parse("<div class=\"a b\">x</div><div class=\"c\">y</div>").unwrap();
        let sel = doc.find_by_class("a");
        assert_eq!(sel.len(), 1);
        assert_eq!(sel.text(), "x");
    }

    #[test]
    fn find_by_attr_works() {
        let doc = parse("<a href=\"x\">a</a><a href=\"y\">b</a>").unwrap();
        let sel = doc.find_by_attr("href", "x");
        assert_eq!(sel.len(), 1);
        assert_eq!(sel.text(), "a");
    }

    #[test]
    fn selection_attr() {
        let doc = parse("<a href=\"url\">link</a>").unwrap();
        let sel = doc.select("a").unwrap();
        assert_eq!(sel.attr("href"), Some("url"));
    }

    #[test]
    fn selection_inner_html() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let sel = doc.select("div").unwrap();
        assert_eq!(sel.inner_html(), "<p>Hello</p>");
    }

    #[test]
    fn selection_empty() {
        let doc = parse("<div>x</div>").unwrap();
        let sel = doc.select("span").unwrap();
        assert!(sel.is_empty());
        assert_eq!(sel.len(), 0);
        assert!(sel.first().is_none());
    }

    #[test]
    fn document_index_o1() {
        let doc = parse("<div id=\"a\">x</div><div id=\"b\">y</div>").unwrap();
        let index = DocumentIndex::build(&doc);
        let node = index.find_by_id(&doc, "b").unwrap();
        assert_eq!(node.text_content(), "y");
    }

    #[test]
    fn selection_into_iter() {
        let doc = parse("<div><p>a</p><p>b</p></div>").unwrap();
        let sel = doc.select("p").unwrap();
        let texts: Vec<String> = (&sel).into_iter().map(|n| n.text_content()).collect();
        assert_eq!(texts, vec!["a", "b"]);
    }
}
