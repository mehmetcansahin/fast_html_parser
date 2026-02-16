//! CSS selector and XPath engine for the SIMD-optimized HTML parser.
//!
//! Provides CSS selector parsing, XPath evaluation, and a convenience API
//! for querying a parsed [`hp_tree::Document`].
//!
//! # Quick Start — CSS
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
//! # Quick Start — XPath
//!
//! ```
//! use hp_tree::parse;
//! use hp_selector::Selectable;
//! use hp_selector::xpath::ast::XPathResult;
//!
//! let doc = parse("<div><p>Hello</p></div>").unwrap();
//! let result = doc.xpath("//p/text()").unwrap();
//! match result {
//!     XPathResult::Strings(texts) => assert_eq!(texts[0], "Hello"),
//!     _ => panic!("expected strings"),
//! }
//! ```
//!
//! # Supported CSS Selectors
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
//!
//! # Supported XPath
//!
//! - `//tag` — descendant search
//! - `//tag[@attr='value']` — attribute predicate
//! - `/path/to/tag` — absolute path
//! - `//tag[contains(@attr, 'substr')]` — contains predicate
//! - `//tag[position()=N]` — position predicate
//! - `//tag/text()` — text extraction
//! - `..` — parent axis

/// CSS selector AST types.
pub mod ast;
/// Bloom filter for ancestor pre-filtering.
pub mod bloom;
/// Right-to-left matching engine.
pub mod matcher;
/// CSS selector parser.
pub mod parser;
/// XPath expression support.
pub mod xpath;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use hp_core::error::{SelectorError, XPathError};
use hp_core::tag::Tag;
use hp_tree::node::{NodeFlags, NodeId};
use hp_tree::{Document, NodeRef};

use matcher::{select_all_list, select_first_list};
use parser::parse_selector;
use xpath::ast::XPathResult;

/// Maximum number of parsed selector ASTs cached per thread.
const SELECTOR_CACHE_CAPACITY: usize = 256;
/// Skip caching unusually long selectors to avoid pinning large keys.
const MAX_CACHED_SELECTOR_LEN: usize = 512;

struct SelectorCache {
    map: HashMap<String, Arc<ast::SelectorList>>,
    order: VecDeque<String>,
}

impl SelectorCache {
    fn new() -> Self {
        Self {
            map: HashMap::with_capacity(SELECTOR_CACHE_CAPACITY),
            order: VecDeque::with_capacity(SELECTOR_CACHE_CAPACITY),
        }
    }

    fn get(&self, css: &str) -> Option<Arc<ast::SelectorList>> {
        self.map.get(css).map(Arc::clone)
    }

    fn insert(&mut self, css: &str, list: Arc<ast::SelectorList>) {
        if self.map.contains_key(css) {
            self.map.insert(css.to_owned(), list);
            return;
        }

        if self.map.len() >= SELECTOR_CACHE_CAPACITY {
            if let Some(old_key) = self.order.pop_front() {
                self.map.remove(&old_key);
            }
        }

        let key = css.to_owned();
        self.order.push_back(key.clone());
        self.map.insert(key, list);
    }
}

thread_local! {
    static SELECTOR_CACHE: RefCell<SelectorCache> = RefCell::new(SelectorCache::new());
}

#[inline]
fn parse_selector_cached(css: &str) -> Result<Arc<ast::SelectorList>, SelectorError> {
    if css.len() > MAX_CACHED_SELECTOR_LEN || is_single_simple_selector(css) {
        return Ok(Arc::new(parse_selector(css)?));
    }

    SELECTOR_CACHE.with(|cache| {
        if let Some(list) = cache.borrow().get(css) {
            return Ok(list);
        }

        let parsed = Arc::new(parse_selector(css)?);
        cache.borrow_mut().insert(css, Arc::clone(&parsed));
        Ok(parsed)
    })
}

#[inline]
fn is_single_simple_selector(css: &str) -> bool {
    if is_simple_ident(css) {
        return true;
    }

    let bytes = css.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'.' || bytes[0] == b'#') {
        return is_simple_ident(&css[1..]);
    }

    false
}

#[inline]
fn is_simple_ident(s: &str) -> bool {
    !s.is_empty()
        && s.as_bytes()
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

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
        let list = parse_selector_cached(css)?;
        if self.nodes.len() == 1 {
            // Single root — DFS produces document-order results, no duplicates possible.
            let results = select_all_list(self.doc.arena(), self.nodes[0], &list);
            return Ok(Selection::new(self.doc, results));
        }
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

    /// Evaluate an XPath expression within the matched nodes.
    ///
    /// Each matched node is used as a context root, and results are
    /// deduplicated in document order.
    pub fn xpath(&self, expr: &str) -> Result<XPathResult, XPathError> {
        let parsed = xpath::parser::parse_xpath(expr)?;
        let mut all_nodes = Vec::new();
        let mut all_strings = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for &node_id in &self.nodes {
            let result = xpath::eval::evaluate(&parsed, self.doc.arena(), node_id);
            match result {
                XPathResult::Nodes(nodes) => {
                    for id in nodes {
                        if seen.insert(id) {
                            all_nodes.push(id);
                        }
                    }
                }
                XPathResult::Strings(strings) => {
                    all_strings.extend(strings);
                }
                XPathResult::Boolean(b) => return Ok(XPathResult::Boolean(b)),
            }
        }

        if !all_strings.is_empty() {
            Ok(XPathResult::Strings(all_strings))
        } else {
            Ok(XPathResult::Nodes(all_nodes))
        }
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

    /// Evaluate an XPath expression against the document.
    ///
    /// # Errors
    ///
    /// Returns [`XPathError::Invalid`] if the expression syntax is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use hp_tree::parse;
    /// use hp_selector::Selectable;
    /// use hp_selector::xpath::ast::XPathResult;
    ///
    /// let doc = parse("<div><p>Hello</p></div>").unwrap();
    /// let result = doc.xpath("//p").unwrap();
    /// match result {
    ///     XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
    ///     _ => panic!("expected nodes"),
    /// }
    /// ```
    fn xpath(&self, expr: &str) -> Result<XPathResult, XPathError>;
}

impl Selectable for Document {
    fn select(&self, css: &str) -> Result<Selection<'_>, SelectorError> {
        let list = parse_selector_cached(css)?;
        let nodes = select_all_list(self.arena(), self.root_id(), &list);
        Ok(Selection::new(self, nodes))
    }

    fn select_first(&self, css: &str) -> Result<Option<NodeRef<'_>>, SelectorError> {
        let list = parse_selector_cached(css)?;
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

    fn xpath(&self, expr: &str) -> Result<XPathResult, XPathError> {
        let parsed = xpath::parser::parse_xpath(expr)?;
        Ok(xpath::eval::evaluate(&parsed, self.arena(), self.root_id()))
    }
}

/// Pre-built index for O(1) id, class, and tag lookups.
///
/// Build once with a single DFS pass, reuse for many lookups.
pub struct DocumentIndex {
    id_map: HashMap<String, NodeId>,
    class_map: HashMap<String, Vec<NodeId>>,
    tag_map: HashMap<Tag, Vec<NodeId>>,
}

impl DocumentIndex {
    /// Build an index from a document by scanning all nodes in a single pass.
    pub fn build(doc: &Document) -> Self {
        let arena = doc.arena();
        let mut id_map = HashMap::with_capacity(arena.len() / 8);
        let mut class_map: HashMap<String, Vec<NodeId>> = HashMap::with_capacity(arena.len() / 4);
        let mut tag_map: HashMap<Tag, Vec<NodeId>> = HashMap::with_capacity(64);

        for i in 0..arena.len() {
            let node_id = NodeId(i as u32);
            let n = arena.get(node_id);

            // Skip non-element nodes.
            if n.flags.has(NodeFlags::IS_TEXT)
                || n.flags.has(NodeFlags::IS_COMMENT)
                || n.flags.has(NodeFlags::IS_DOCTYPE)
            {
                continue;
            }

            // Index by tag.
            tag_map.entry(n.tag).or_default().push(node_id);

            // Index by attributes.
            let attrs = arena.attrs(node_id);
            for attr in attrs {
                if attr.name == "id" {
                    if let Some(val) = attr.value.as_deref() {
                        if let Some(existing) = id_map.get_mut(val) {
                            *existing = node_id;
                        } else {
                            id_map.insert(val.to_owned(), node_id);
                        }
                    }
                }
                if attr.name == "class" {
                    if let Some(val) = attr.value.as_deref() {
                        for class in val.split_whitespace() {
                            if let Some(ids) = class_map.get_mut(class) {
                                ids.push(node_id);
                            } else {
                                class_map.insert(class.to_owned(), vec![node_id]);
                            }
                        }
                    }
                }
            }
        }

        Self {
            id_map,
            class_map,
            tag_map,
        }
    }

    /// Look up a node by its `id` attribute in O(1).
    pub fn find_by_id<'a>(&self, doc: &'a Document, id: &str) -> Option<NodeRef<'a>> {
        self.id_map.get(id).map(|&node_id| doc.get(node_id))
    }

    /// Look up all nodes with a given CSS class in O(1).
    pub fn find_by_class<'a>(&self, doc: &'a Document, class: &str) -> Vec<NodeRef<'a>> {
        self.class_map
            .get(class)
            .map(|ids| ids.iter().map(|&id| doc.get(id)).collect())
            .unwrap_or_default()
    }

    /// Look up all nodes with a given tag in O(1).
    pub fn find_by_tag<'a>(&self, doc: &'a Document, tag: Tag) -> Vec<NodeRef<'a>> {
        self.tag_map
            .get(&tag)
            .map(|ids| ids.iter().map(|&id| doc.get(id)).collect())
            .unwrap_or_default()
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
    fn document_index_find_by_class() {
        let doc = parse("<div class=\"a b\">x</div><span class=\"b c\">y</span><p>z</p>").unwrap();
        let index = DocumentIndex::build(&doc);

        let class_b = index.find_by_class(&doc, "b");
        assert_eq!(class_b.len(), 2);

        let class_a = index.find_by_class(&doc, "a");
        assert_eq!(class_a.len(), 1);
        assert_eq!(class_a[0].text_content(), "x");

        let class_missing = index.find_by_class(&doc, "nope");
        assert!(class_missing.is_empty());
    }

    #[test]
    fn document_index_find_by_tag() {
        let doc = parse("<div>a</div><div>b</div><span>c</span>").unwrap();
        let index = DocumentIndex::build(&doc);

        let divs = index.find_by_tag(&doc, Tag::Div);
        assert_eq!(divs.len(), 2);

        let spans = index.find_by_tag(&doc, Tag::Span);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text_content(), "c");

        let links = index.find_by_tag(&doc, Tag::A);
        assert!(links.is_empty());
    }

    #[test]
    fn selection_into_iter() {
        let doc = parse("<div><p>a</p><p>b</p></div>").unwrap();
        let sel = doc.select("p").unwrap();
        let texts: Vec<String> = (&sel).into_iter().map(|n| n.text_content()).collect();
        assert_eq!(texts, vec!["a", "b"]);
    }

    #[test]
    fn xpath_descendant() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let result = doc.xpath("//p").unwrap();
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn xpath_text_extract() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let result = doc.xpath("//p/text()").unwrap();
        match result {
            XPathResult::Strings(texts) => {
                assert_eq!(texts.len(), 1);
                assert_eq!(texts[0], "Hello");
            }
            _ => panic!("expected Strings"),
        }
    }

    #[test]
    fn xpath_invalid() {
        let doc = parse("<div>x</div>").unwrap();
        assert!(doc.xpath("").is_err());
        assert!(doc.xpath("bad").is_err());
    }

    #[test]
    fn selection_xpath_chaining() {
        let doc = parse("<ul><li>1</li><li>2</li></ul><ol><li>3</li></ol>").unwrap();
        let sel = doc.select("ul").unwrap();
        let result = sel.xpath("//li").unwrap();
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 2),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn single_simple_selector_detection() {
        assert!(is_single_simple_selector("p"));
        assert!(is_single_simple_selector("my-widget"));
        assert!(is_single_simple_selector(".content"));
        assert!(is_single_simple_selector("#main"));

        assert!(!is_single_simple_selector("div p"));
        assert!(!is_single_simple_selector("div.article"));
        assert!(!is_single_simple_selector("a[href]"));
    }
}
