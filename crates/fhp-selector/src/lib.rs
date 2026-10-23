#![cfg_attr(docsrs, feature(doc_cfg))]
//! CSS selector and XPath engine for the SIMD-optimized HTML parser.
//!
//! Provides CSS selector parsing, XPath evaluation, and a convenience API
//! for querying a parsed [`fhp_tree::Document`].
//!
//! # Quick Start — CSS
//!
//! ```
//! use fhp_tree::parse;
//! use fhp_selector::Selectable;
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
//! # #[cfg(feature = "xpath")] {
//! use fhp_tree::parse;
//! use fhp_selector::Selectable;
//! use fhp_selector::xpath::ast::XPathResult;
//!
//! let doc = parse("<div><p>Hello</p></div>").unwrap();
//! let result = doc.xpath("//p/text()").unwrap();
//! match result {
//!     XPathResult::Strings(texts) => assert_eq!(texts[0], "Hello"),
//!     _ => panic!("expected strings"),
//! }
//! # }
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
/// Ancestor pre-filtering for descendant combinators. Its per-node
/// `AncestorBloom` table is deprecated.
pub mod bloom;
/// Right-to-left matching engine.
pub mod matcher;
/// CSS selector parser.
pub mod parser;
mod space;
/// XPath expression support.
#[cfg(feature = "xpath")]
#[cfg_attr(docsrs, doc(cfg(feature = "xpath")))]
pub mod xpath;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use fhp_core::error::SelectorError;
#[cfg(feature = "xpath")]
use fhp_core::error::XPathError;
use fhp_core::tag::Tag;
use fhp_tree::node::{NodeFlags, NodeId};
use fhp_tree::traverse::DepthFirst;
use fhp_tree::{Document, NodeRef};

use matcher::{select_all_list, select_all_list_roots, select_first_list};
use parser::parse_selector;
use space::{contains_html_space_token, split_html_space};
#[cfg(feature = "xpath")]
use xpath::ast::{XPathExpr, XPathResult};

#[inline]
fn is_document_element(n: &fhp_tree::node::Node) -> bool {
    n.depth > 0
        && !n.flags.has(NodeFlags::IS_TEXT)
        && !n.flags.has(NodeFlags::IS_COMMENT)
        && !n.flags.has(NodeFlags::IS_DOCTYPE)
}

/// A pre-compiled CSS selector for reuse across documents and threads.
///
/// Parsing a CSS selector string has non-trivial cost. When the same selector
/// is used to query many documents (e.g., in a scraping loop), compile it once
/// and reuse it to eliminate repeated parse overhead.
///
/// # Example
///
/// ```
/// use fhp_tree::parse;
/// use fhp_selector::{CompiledSelector, Selectable};
///
/// let sel = CompiledSelector::new("div.content").unwrap();
/// let doc = parse("<div class=\"content\">Hello</div>").unwrap();
/// let results = doc.select_compiled(&sel).unwrap();
/// assert_eq!(results.len(), 1);
/// ```
#[derive(Clone)]
pub struct CompiledSelector {
    list: Arc<ast::SelectorList>,
}

impl CompiledSelector {
    /// Compile a CSS selector string.
    ///
    /// # Errors
    ///
    /// Returns [`SelectorError::Invalid`] if the selector syntax is invalid.
    pub fn new(css: &str) -> Result<Self, SelectorError> {
        Ok(Self {
            list: Arc::new(parse_selector(css)?),
        })
    }

    /// Access the underlying parsed selector list.
    pub fn as_list(&self) -> &ast::SelectorList {
        &self.list
    }
}

impl core::fmt::Debug for CompiledSelector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompiledSelector")
            .field("selectors", &self.list.selectors.len())
            .finish()
    }
}

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
    if css.len() > MAX_CACHED_SELECTOR_LEN {
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

    /// Sub-select with a pre-compiled selector within the matched nodes.
    ///
    /// Each matched node is used as a subtree root, and results are
    /// deduplicated in document order.
    pub fn select_compiled(&self, sel: &CompiledSelector) -> Result<Selection<'a>, SelectorError> {
        Ok(Selection::new(self.doc, self.select_within(&sel.list)))
    }

    /// Sub-select: run a CSS selector within the matched nodes.
    ///
    /// Each matched node is used as a subtree root, and results are
    /// deduplicated in document order.
    pub fn select(&self, css: &str) -> Result<Selection<'a>, SelectorError> {
        let list = parse_selector_cached(css)?;
        Ok(Selection::new(self.doc, self.select_within(&list)))
    }

    /// Matches of `list` within the subtrees of the matched nodes.
    fn select_within(&self, list: &ast::SelectorList) -> Vec<NodeId> {
        let arena = self.doc.arena();
        match self.nodes.as_slice() {
            // Single root: the DFS is already in document order without duplicates.
            [root] => select_all_list(arena, *root, list),
            roots => select_all_list_roots(arena, roots, list),
        }
    }

    /// Evaluate an XPath expression within the matched nodes.
    ///
    /// Each matched node is used as a context root, and results are
    /// deduplicated in document order.
    ///
    /// Note: the leading `//` is evaluated **relative to each matched node's
    /// subtree** (descendant-or-self), not against the whole document. So
    /// `doc.select("div").xpath("//span")` finds the spans inside each matched
    /// `div`, and an axis like `//div` also yields the context `div` itself.
    /// This scoping is intentional so XPath composes with a prior selection;
    /// for a document-wide query, evaluate from the document root instead.
    #[cfg(feature = "xpath")]
    #[cfg_attr(docsrs, doc(cfg(feature = "xpath")))]
    pub fn xpath(&self, expr: &str) -> Result<XPathResult, XPathError> {
        let parsed = xpath::parser::parse_xpath(expr)?;

        if let XPathExpr::TextExtract(inner) = &parsed {
            let arena = self.doc.arena();
            let mut candidates = Vec::new();
            match inner.as_ref() {
                // Resolve positions once per parent across all roots instead of
                // rescanning each root's siblings.
                XPathExpr::PositionPredicate { tag, pos } if self.nodes.len() > 1 => {
                    for node in xpath::eval::find_descendants_by_tag_at_position_for_roots(
                        arena,
                        &self.nodes,
                        tag,
                        *pos,
                    ) {
                        xpath::eval::collect_direct_text_children(arena, node, &mut candidates);
                    }
                }
                _ => {
                    for &node_id in &self.nodes {
                        candidates.extend(
                            xpath::eval::evaluate_text_nodes(&parsed, arena, node_id)
                                .unwrap_or_default(),
                        );
                    }
                }
            }

            let mut seen = std::collections::HashSet::new();
            return Ok(XPathResult::Strings(
                candidates
                    .into_iter()
                    .filter(|&id| seen.insert(id))
                    .map(|id| arena.text(id).to_owned())
                    .collect(),
            ));
        }

        if self.nodes.len() > 1 {
            if let XPathExpr::PositionPredicate { tag, pos } = &parsed {
                return Ok(XPathResult::Nodes(
                    xpath::eval::find_descendants_by_tag_at_position_for_roots(
                        self.doc.arena(),
                        &self.nodes,
                        tag,
                        *pos,
                    ),
                ));
            }
        }

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
    /// use fhp_tree::parse;
    /// use fhp_selector::Selectable;
    ///
    /// let doc = parse("<div><p>Hello</p></div>").unwrap();
    /// let sel = doc.select("p").unwrap();
    /// assert_eq!(sel.len(), 1);
    /// ```
    fn select(&self, css: &str) -> Result<Selection<'_>, SelectorError>;

    /// Select all nodes matching a pre-compiled CSS selector.
    ///
    /// This avoids re-parsing the selector string on every call, which is
    /// beneficial when the same selector is reused across many documents.
    fn select_compiled(&self, sel: &CompiledSelector) -> Result<Selection<'_>, SelectorError>;

    /// Select the first node matching a pre-compiled CSS selector.
    fn select_first_compiled(
        &self,
        sel: &CompiledSelector,
    ) -> Result<Option<NodeRef<'_>>, SelectorError>;

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
    ///
    /// As in `[name='']`, a valueless attribute such as `disabled` has the
    /// empty string as its value.
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
    /// # #[cfg(feature = "xpath")] {
    /// use fhp_tree::parse;
    /// use fhp_selector::Selectable;
    /// use fhp_selector::xpath::ast::XPathResult;
    ///
    /// let doc = parse("<div><p>Hello</p></div>").unwrap();
    /// let result = doc.xpath("//p").unwrap();
    /// match result {
    ///     XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
    ///     _ => panic!("expected nodes"),
    /// }
    /// # }
    /// ```
    #[cfg(feature = "xpath")]
    #[cfg_attr(docsrs, doc(cfg(feature = "xpath")))]
    fn xpath(&self, expr: &str) -> Result<XPathResult, XPathError>;
}

impl Selectable for Document {
    fn select(&self, css: &str) -> Result<Selection<'_>, SelectorError> {
        let list = parse_selector_cached(css)?;
        let nodes = select_all_list(self.arena(), self.root_id(), &list);
        Ok(Selection::new(self, nodes))
    }

    fn select_compiled(&self, sel: &CompiledSelector) -> Result<Selection<'_>, SelectorError> {
        let nodes = select_all_list(self.arena(), self.root_id(), &sel.list);
        Ok(Selection::new(self, nodes))
    }

    fn select_first_compiled(
        &self,
        sel: &CompiledSelector,
    ) -> Result<Option<NodeRef<'_>>, SelectorError> {
        let node = select_first_list(self.arena(), self.root_id(), &sel.list);
        Ok(node.map(|id| self.get(id)))
    }

    fn select_first(&self, css: &str) -> Result<Option<NodeRef<'_>>, SelectorError> {
        let list = parse_selector_cached(css)?;
        let node = select_first_list(self.arena(), self.root_id(), &list);
        Ok(node.map(|id| self.get(id)))
    }

    fn find_by_tag(&self, tag: Tag) -> Selection<'_> {
        let arena = self.arena();
        let mut nodes = Vec::new();
        for id in DepthFirst::new(arena, self.root_id()) {
            let n = arena.get(id);
            if n.tag == tag && is_document_element(n) {
                nodes.push(id);
            }
        }
        Selection::new(self, nodes)
    }

    fn find_by_id(&self, id: &str) -> Option<NodeRef<'_>> {
        let arena = self.arena();
        for node_id in DepthFirst::new(arena, self.root_id()) {
            if !is_document_element(arena.get(node_id)) {
                continue;
            }
            let attrs = arena.attrs(node_id);
            for attr in attrs {
                if arena.attr_name(attr).eq_ignore_ascii_case("id")
                    && arena.attr_value(attr) == Some(id)
                {
                    return Some(self.get(node_id));
                }
            }
        }
        None
    }

    fn find_by_class(&self, class: &str) -> Selection<'_> {
        let arena = self.arena();
        let mut nodes = Vec::new();
        for id in DepthFirst::new(arena, self.root_id()) {
            if !is_document_element(arena.get(id)) {
                continue;
            }
            let attrs = arena.attrs(id);
            for attr in attrs {
                if arena.attr_name(attr).eq_ignore_ascii_case("class") {
                    if let Some(val) = arena.attr_value(attr) {
                        if contains_html_space_token(val, class) {
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
        for id in DepthFirst::new(arena, self.root_id()) {
            if !is_document_element(arena.get(id)) {
                continue;
            }
            let attrs = arena.attrs(id);
            for attr in attrs {
                if arena.attr_name(attr).eq_ignore_ascii_case(name)
                    && arena.attr_value(attr).unwrap_or("") == value
                {
                    nodes.push(id);
                    break;
                }
            }
        }
        Selection::new(self, nodes)
    }

    #[cfg(feature = "xpath")]
    fn xpath(&self, expr: &str) -> Result<XPathResult, XPathError> {
        let parsed = xpath::parser::parse_xpath(expr)?;
        Ok(xpath::eval::evaluate(&parsed, self.arena(), self.root_id()))
    }
}

/// Pre-built index for O(1) id, class, and tag lookups.
///
/// Build once with a single DFS pass, reuse for many lookups.
pub struct DocumentIndex<'a> {
    doc: &'a Document,
    id_map: HashMap<String, NodeId>,
    class_map: HashMap<String, Vec<NodeId>>,
    tag_map: HashMap<Tag, Vec<NodeId>>,
}

impl<'a> DocumentIndex<'a> {
    /// Build an index from a document in one document-order traversal.
    pub fn build(doc: &'a Document) -> Self {
        let arena = doc.arena();
        let mut id_map = HashMap::with_capacity(arena.len() / 8);
        let mut class_map: HashMap<String, Vec<NodeId>> = HashMap::with_capacity(arena.len() / 4);
        let mut tag_map: HashMap<Tag, Vec<NodeId>> = HashMap::with_capacity(64);

        // Arena allocation order is not necessarily DOM order: HTML foster
        // parenting can move a later-allocated node before a table. Traverse
        // tree links so every lookup has the same order as CSS selection.
        for node_id in DepthFirst::new(arena, doc.root_id()) {
            let n = arena.get(node_id);
            if !is_document_element(n) {
                continue;
            }

            tag_map.entry(n.tag).or_default().push(node_id);

            let attrs = arena.attrs(node_id);
            for attr in attrs {
                let attr_name = arena.attr_name(attr);
                if attr_name.eq_ignore_ascii_case("id") {
                    if let Some(val) = arena.attr_value(attr) {
                        id_map.entry(val.to_owned()).or_insert(node_id);
                    }
                }
                if attr_name.eq_ignore_ascii_case("class") {
                    if let Some(val) = arena.attr_value(attr) {
                        for class in split_html_space(val) {
                            if let Some(ids) = class_map.get_mut(class) {
                                if ids.last().copied() != Some(node_id) {
                                    ids.push(node_id);
                                }
                            } else {
                                class_map.insert(class.to_owned(), vec![node_id]);
                            }
                        }
                    }
                }
            }
        }

        Self {
            doc,
            id_map,
            class_map,
            tag_map,
        }
    }

    /// Look up a node by its `id` attribute in O(1).
    ///
    /// The `doc` argument is retained for compatibility; lookups resolve against
    /// the document used to build this index.
    pub fn find_by_id(&self, _doc: &Document, id: &str) -> Option<NodeRef<'a>> {
        self.id_map.get(id).map(|&node_id| self.doc.get(node_id))
    }

    /// Look up all nodes with a given CSS class in O(1).
    ///
    /// The `doc` argument is retained for compatibility; lookups resolve against
    /// the document used to build this index.
    pub fn find_by_class(&self, _doc: &Document, class: &str) -> Vec<NodeRef<'a>> {
        self.class_map
            .get(class)
            .map(|ids| ids.iter().map(|&id| self.doc.get(id)).collect())
            .unwrap_or_default()
    }

    /// Look up all nodes with a given tag in O(1).
    ///
    /// The `doc` argument is retained for compatibility; lookups resolve against
    /// the document used to build this index.
    pub fn find_by_tag(&self, _doc: &Document, tag: Tag) -> Vec<NodeRef<'a>> {
        self.tag_map
            .get(&tag)
            .map(|ids| ids.iter().map(|&id| self.doc.get(id)).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fhp_tree::parse;

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
    fn document_index_uses_document_it_was_built_from() {
        let source_doc = parse("<div id=\"a\"><span>source</span></div>").unwrap();
        let other_doc = parse("<p>other</p>").unwrap();
        let index = DocumentIndex::build(&source_doc);

        let node = index.find_by_id(&other_doc, "a").unwrap();
        assert_eq!(node.text_content(), "source");
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
    fn document_index_handles_uppercase_html_attributes() {
        let doc = parse("<div ID=\"main\" CLASS=\"active hero\">x</div>").unwrap();
        let index = DocumentIndex::build(&doc);

        assert_eq!(index.find_by_id(&doc, "main").unwrap().text_content(), "x");
        assert_eq!(index.find_by_class(&doc, "active").len(), 1);
        assert_eq!(index.find_by_class(&doc, "hero").len(), 1);
    }

    #[test]
    fn selection_into_iter() {
        let doc = parse("<div><p>a</p><p>b</p></div>").unwrap();
        let sel = doc.select("p").unwrap();
        let texts: Vec<String> = (&sel).into_iter().map(|n| n.text_content()).collect();
        assert_eq!(texts, vec!["a", "b"]);
    }

    #[cfg(feature = "xpath")]
    #[test]
    fn xpath_descendant() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let result = doc.xpath("//p").unwrap();
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[cfg(feature = "xpath")]
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

    #[cfg(feature = "xpath")]
    #[test]
    fn xpath_invalid() {
        let doc = parse("<div>x</div>").unwrap();
        assert!(doc.xpath("").is_err());
        assert!(doc.xpath("bad").is_err());
    }

    #[cfg(feature = "xpath")]
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
    fn compiled_selector_basic() {
        let sel = CompiledSelector::new("p").unwrap();
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        let results = doc.select_compiled(&sel).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results.text(), "Hello");
    }

    #[test]
    fn compiled_selector_first() {
        let sel = CompiledSelector::new("p").unwrap();
        let doc = parse("<div><p>a</p><p>b</p></div>").unwrap();
        let first = doc.select_first_compiled(&sel).unwrap();
        assert!(first.is_some());
        assert_eq!(first.unwrap().text_content(), "a");
    }

    #[test]
    fn compiled_selector_reuse_across_docs() {
        let sel = CompiledSelector::new("span.active").unwrap();
        let doc1 = parse("<span class=\"active\">one</span>").unwrap();
        let doc2 = parse("<div><span class=\"active\">two</span></div>").unwrap();
        assert_eq!(doc1.select_compiled(&sel).unwrap().text(), "one");
        assert_eq!(doc2.select_compiled(&sel).unwrap().text(), "two");
    }

    #[test]
    fn compiled_selector_chaining() {
        let sel = CompiledSelector::new("a").unwrap();
        let doc = parse("<ul><li><a>1</a></li><li><a>2</a></li></ul>").unwrap();
        let lis = doc.select("li").unwrap();
        let links = lis.select_compiled(&sel).unwrap();
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn compiled_selector_invalid() {
        assert!(CompiledSelector::new("").is_err());
    }

    #[test]
    fn compiled_selector_clone() {
        let sel = CompiledSelector::new("div").unwrap();
        let sel2 = sel.clone();
        let doc = parse("<div>ok</div>").unwrap();
        assert_eq!(doc.select_compiled(&sel2).unwrap().len(), 1);
    }
}
