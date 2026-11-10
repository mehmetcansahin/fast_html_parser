//! XPath expression evaluator.
//!
//! Walks the DOM tree and collects nodes or text matching an [`XPathExpr`].
//!
//! [`XPathExpr`]: crate::xpath::ast::XPathExpr

use std::collections::{HashMap, HashSet};

use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::node::{NodeFlags, NodeId};

use super::ast::{NameTest, PathStep, Predicate, XPathExpr, XPathResult};
use crate::matcher::value_is;

/// Evaluate an XPath expression against an arena starting from `root`.
pub fn evaluate(expr: &XPathExpr, arena: &Arena, root: NodeId) -> XPathResult {
    match expr {
        XPathExpr::DescendantByTag(_)
        | XPathExpr::DescendantByAttr { .. }
        | XPathExpr::DescendantByAttrExists { .. }
        | XPathExpr::ContainsPredicate { .. }
        | XPathExpr::PositionPredicate { .. }
        | XPathExpr::DescendantWildcard
        | XPathExpr::DescendantWildcardByAttr { .. }
        | XPathExpr::DescendantWildcardByAttrExists { .. } => XPathResult::Nodes(
            collect_descendants(expr, arena, root, Collect::Elements)
                .expect("descendant-axis expression"),
        ),

        XPathExpr::AbsolutePath(steps) => {
            let nodes = evaluate_absolute_path(arena, root, steps);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::TextExtract(_) => {
            let text_nodes = evaluate_text_nodes(expr, arena, root).unwrap_or_default();
            XPathResult::Strings(
                text_nodes
                    .into_iter()
                    .map(|id| arena.text(id).to_owned())
                    .collect(),
            )
        }

        XPathExpr::Parent => {
            // Parent is relative — from root's parent.
            let n = arena.get(root);
            if n.parent.is_null() {
                XPathResult::Nodes(vec![])
            } else {
                XPathResult::Nodes(vec![n.parent])
            }
        }
    }
}

/// Evaluate a `.../text()` expression to the underlying text node ids.
///
/// Keeping node identity available lets callers that combine overlapping
/// context roots deduplicate the same text node without collapsing distinct
/// text nodes that happen to contain equal strings.
pub(crate) fn evaluate_text_nodes(
    expr: &XPathExpr,
    arena: &Arena,
    root: NodeId,
) -> Option<Vec<NodeId>> {
    let XPathExpr::TextExtract(inner) = expr else {
        return None;
    };
    if let Some(text_nodes) = collect_descendants(inner, arena, root, Collect::TextChildren) {
        return Some(text_nodes);
    }

    // An absolute path yields nodes of one depth and `..` one node, so their
    // direct text children cannot interleave.
    let XPathResult::Nodes(nodes) = evaluate(inner, arena, root) else {
        return Some(Vec::new());
    };

    let mut text_nodes = Vec::new();
    for node in nodes {
        collect_direct_text_children(arena, node, &mut text_nodes);
    }
    Some(text_nodes)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns `true` for element nodes (not text, comment, doctype).
#[inline]
fn is_element(n: &fhp_tree::node::Node) -> bool {
    !n.flags.has(NodeFlags::IS_TEXT)
        && !n.flags.has(NodeFlags::IS_COMMENT)
        && !n.flags.has(NodeFlags::IS_DOCTYPE)
}

#[inline]
fn matches_name_test(
    arena: &Arena,
    node: NodeId,
    element: &fhp_tree::node::Node,
    name: &NameTest,
) -> bool {
    match name {
        NameTest::Interned(tag) => element.tag == *tag,
        NameTest::Literal(literal) => {
            element.tag == Tag::Unknown
                && arena
                    .unknown_tag_name(node)
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(literal))
        }
    }
}

/// What a descendant traversal collects for the elements it matches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Collect {
    /// The matching elements themselves.
    Elements,
    /// The direct text children of the matching elements, as selected by a
    /// trailing `/text()`.
    TextChildren,
}

/// Generic DFS: collect the descendant-or-self nodes that satisfy `predicate`,
/// or their direct text children, in document order.
///
/// Text children are collected by the same traversal that finds their parent,
/// so the text of a match nested inside another match lands between the
/// outer match's own text nodes.
fn dfs_collect<const TEXT: bool>(
    arena: &Arena,
    node: NodeId,
    predicate: &impl Fn(&Arena, NodeId, &fhp_tree::node::Node) -> bool,
    results: &mut Vec<NodeId>,
) {
    if node.is_null() {
        return;
    }
    let n = arena.get(node);
    let matched = predicate(arena, node, n);
    if matched && !TEXT {
        results.push(node);
    }
    let collect_text = matched && TEXT;
    let mut child = n.first_child;
    while !child.is_null() {
        let child_node = arena.get(child);
        if collect_text && child_node.flags.has(NodeFlags::IS_TEXT) {
            results.push(child);
        } else {
            dfs_collect::<TEXT>(arena, child, predicate, results);
        }
        child = child_node.next_sibling;
    }
}

/// Evaluate a descendant-axis expression (descendant-or-self of `root`) in a
/// single traversal, collecting the matching elements or their direct text
/// children in document order.
///
/// Returns `None` for the expressions that are not descendant axes: absolute
/// paths, `..`, and `text()` itself.
fn collect_descendants(
    expr: &XPathExpr,
    arena: &Arena,
    root: NodeId,
    collect: Collect,
) -> Option<Vec<NodeId>> {
    match collect {
        Collect::Elements => collect_descendants_mode::<false>(expr, arena, root),
        Collect::TextChildren => collect_descendants_mode::<true>(expr, arena, root),
    }
}

/// Select the collection mode once, outside the recursive hot loops.
fn collect_descendants_mode<const TEXT: bool>(
    expr: &XPathExpr,
    arena: &Arena,
    root: NodeId,
) -> Option<Vec<NodeId>> {
    let mut results = Vec::new();
    match expr {
        XPathExpr::DescendantByTag(tag) => dfs_collect::<TEXT>(
            arena,
            root,
            &|a, id, n| is_element(n) && matches_name_test(a, id, n, tag),
            &mut results,
        ),
        XPathExpr::DescendantByAttr { tag, attr, value } => dfs_collect::<TEXT>(
            arena,
            root,
            &|a, id, n| {
                is_element(n)
                    && matches_name_test(a, id, n, tag)
                    && a.attrs(id)
                        .iter()
                        .any(|at| a.attr_name_is(at, attr) && value_is(a, at, value))
            },
            &mut results,
        ),
        XPathExpr::DescendantByAttrExists { tag, attr } => dfs_collect::<TEXT>(
            arena,
            root,
            &|a, id, n| {
                is_element(n)
                    && matches_name_test(a, id, n, tag)
                    && a.attrs(id).iter().any(|at| a.attr_name_is(at, attr))
            },
            &mut results,
        ),
        XPathExpr::ContainsPredicate { tag, attr, substr } => dfs_collect::<TEXT>(
            arena,
            root,
            &|a, id, n| {
                is_element(n)
                    && matches_name_test(a, id, n, tag)
                    && a.attrs(id).iter().any(|at| {
                        a.attr_name_is(at, attr)
                            && a.attr_value_len(at) >= substr.len()
                            && a.attr_value(at).unwrap_or("").contains(substr.as_str())
                    })
            },
            &mut results,
        ),
        XPathExpr::DescendantWildcard => dfs_collect::<TEXT>(
            arena,
            root,
            &|_, _, n| is_element(n) && n.depth > 0,
            &mut results,
        ),
        XPathExpr::DescendantWildcardByAttr { attr, value } => dfs_collect::<TEXT>(
            arena,
            root,
            &|a, id, n| {
                is_element(n)
                    && n.depth > 0
                    && a.attrs(id)
                        .iter()
                        .any(|at| a.attr_name_is(at, attr) && value_is(a, at, value))
            },
            &mut results,
        ),
        XPathExpr::DescendantWildcardByAttrExists { attr } => dfs_collect::<TEXT>(
            arena,
            root,
            &|a, id, n| {
                is_element(n)
                    && n.depth > 0
                    && a.attrs(id).iter().any(|at| a.attr_name_is(at, attr))
            },
            &mut results,
        ),
        XPathExpr::PositionPredicate { tag, pos } => {
            return Some(find_descendants_by_tag_at_position::<TEXT>(
                arena, root, tag, *pos,
            ));
        }
        XPathExpr::AbsolutePath(_) | XPathExpr::TextExtract(_) | XPathExpr::Parent => {
            return None;
        }
    }
    Some(results)
}

/// Reorder `ids`, nodes of the tree under `root`, into document order.
///
/// Node ids are not allocated in document order (foster parenting and
/// adoption-agency clones reorder them). Already ordered results need only
/// adjacent tree-link comparisons; otherwise one DFS from `root` orders them.
/// Duplicate ids collapse into one.
pub(crate) fn sort_in_document_order(arena: &Arena, root: NodeId, ids: &mut Vec<NodeId>) {
    if ids.len() < 2 {
        return;
    }
    if ids
        .windows(2)
        .all(|pair| precedes_in_document(arena, pair[0], pair[1]))
    {
        ids.dedup();
        return;
    }
    let wanted: HashSet<NodeId> = ids.iter().copied().collect();
    let mut ordered = Vec::with_capacity(wanted.len());
    dfs_collect::<false>(arena, root, &|_, id, _| wanted.contains(&id), &mut ordered);
    *ids = ordered;
}

/// Compare reachable nodes of one finalized tree without allocating paths.
/// Ancestors precede descendants; otherwise the first divergent siblings
/// decide. Ordered adjacent result pairs walk disjoint sibling gaps.
fn precedes_in_document(arena: &Arena, left: NodeId, right: NodeId) -> bool {
    if left == right {
        return true;
    }
    let mut left_id = left;
    let mut right_id = right;
    let mut left_node = arena.get(left_id);
    let mut right_node = arena.get(right_id);
    while left_node.depth > right_node.depth {
        left_id = left_node.parent;
        left_node = arena.get(left_id);
    }
    while right_node.depth > left_node.depth {
        right_id = right_node.parent;
        right_node = arena.get(right_id);
    }
    if left_id == right_id {
        return left_id == left;
    }
    while left_node.parent != right_node.parent {
        left_id = left_node.parent;
        right_id = right_node.parent;
        left_node = arena.get(left_id);
        right_node = arena.get(right_id);
    }
    if right_node.next_sibling == left_id {
        return false;
    }
    let mut sibling = left_node.next_sibling;
    while !sibling.is_null() {
        if sibling == right_id {
            return true;
        }
        sibling = arena.get(sibling).next_sibling;
    }
    false
}

/// Find the `pos`th matching element child under every parent in one DFS,
/// collecting those elements or their direct text children.
///
/// The context root is included for descendant-or-self semantics. Its position
/// may depend on siblings outside the traversed subtree, so that one node is
/// checked separately; every descendant position is then computed while its
/// parent's children are visited once.
fn find_descendants_by_tag_at_position<const TEXT: bool>(
    arena: &Arena,
    root: NodeId,
    tag: &NameTest,
    pos: usize,
) -> Vec<NodeId> {
    if root.is_null() || pos == 0 {
        return Vec::new();
    }

    let mut results = Vec::new();
    let root_node = arena.get(root);
    let root_matched = is_element(root_node)
        && matches_name_test(arena, root, root_node, tag)
        && has_position_within_parent(arena, root, tag, pos);
    if root_matched && !TEXT {
        results.push(root);
    }

    collect_positioned_descendants::<TEXT>(arena, root, tag, pos, root_matched, &mut results);
    results
}

/// Find positioned descendants, or their direct text children, across
/// document-ordered context roots.
///
/// A selection can contain many siblings, so checking every context root by
/// rescanning its preceding siblings would be quadratic. Cache the positioned
/// child for each parent and traverse overlapping context subtrees only once.
pub(crate) fn find_descendants_by_tag_at_position_for_roots(
    arena: &Arena,
    roots: &[NodeId],
    tag: &NameTest,
    pos: usize,
    collect: Collect,
) -> Vec<NodeId> {
    let mut position_cache = RootPositionCache::with_capacity(roots.len());
    match collect {
        Collect::Elements => find_descendants_by_tag_at_position_for_roots_cached::<false>(
            arena,
            roots,
            tag,
            pos,
            &mut position_cache,
        ),
        Collect::TextChildren => find_descendants_by_tag_at_position_for_roots_cached::<true>(
            arena,
            roots,
            tag,
            pos,
            &mut position_cache,
        ),
    }
}

fn find_descendants_by_tag_at_position_for_roots_cached<const TEXT: bool>(
    arena: &Arena,
    roots: &[NodeId],
    tag: &NameTest,
    pos: usize,
    position_cache: &mut RootPositionCache,
) -> Vec<NodeId> {
    if pos == 0 {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut visited = HashSet::with_capacity(roots.len());

    for &root in roots {
        if root.is_null() || !visited.insert(root) {
            continue;
        }

        let root_node = arena.get(root);
        let root_matched = is_element(root_node)
            && matches_name_test(arena, root, root_node, tag)
            && has_position_within_parent_cached(arena, root, tag, pos, position_cache);
        if root_matched && !TEXT {
            results.push(root);
        }

        collect_positioned_descendants_once::<TEXT>(
            arena,
            root,
            tag,
            pos,
            root_matched,
            &mut results,
            &mut visited,
        );
    }

    results
}

/// Visit the children of `parent`, a node that matched as positioned when
/// `parent_matched`, and collect positioned elements or text children.
fn collect_positioned_descendants<const TEXT: bool>(
    arena: &Arena,
    parent: NodeId,
    tag: &NameTest,
    pos: usize,
    parent_matched: bool,
    results: &mut Vec<NodeId>,
) {
    let mut matching_position = 0usize;
    let mut child = arena.get(parent).first_child;
    while !child.is_null() {
        let child_node = arena.get(child);
        let mut child_matched = false;
        if is_element(child_node) && matches_name_test(arena, child, child_node, tag) {
            matching_position += 1;
            child_matched = matching_position == pos;
        }
        let collected = if TEXT {
            parent_matched && child_node.flags.has(NodeFlags::IS_TEXT)
        } else {
            child_matched
        };
        if collected {
            results.push(child);
        }
        collect_positioned_descendants::<TEXT>(arena, child, tag, pos, child_matched, results);
        child = child_node.next_sibling;
    }
}

/// [`collect_positioned_descendants`] that skips subtrees already traversed
/// from an earlier context root while still counting their positions.
fn collect_positioned_descendants_once<const TEXT: bool>(
    arena: &Arena,
    parent: NodeId,
    tag: &NameTest,
    pos: usize,
    parent_matched: bool,
    results: &mut Vec<NodeId>,
    visited: &mut HashSet<NodeId>,
) {
    let mut matching_position = 0usize;
    let mut child = arena.get(parent).first_child;
    while !child.is_null() {
        let child_node = arena.get(child);
        let first_visit = visited.insert(child);
        let mut child_matched = false;
        if is_element(child_node) && matches_name_test(arena, child, child_node, tag) {
            matching_position += 1;
            child_matched = matching_position == pos;
        }
        if first_visit {
            let collected = if TEXT {
                parent_matched && child_node.flags.has(NodeFlags::IS_TEXT)
            } else {
                child_matched
            };
            if collected {
                results.push(child);
            }
            collect_positioned_descendants_once::<TEXT>(
                arena,
                child,
                tag,
                pos,
                child_matched,
                results,
                visited,
            );
        }
        child = child_node.next_sibling;
    }
}

fn has_position_within_parent(
    arena: &Arena,
    node: NodeId,
    tag: &NameTest,
    expected: usize,
) -> bool {
    let parent = arena.get(node).parent;
    if parent.is_null() {
        return expected == 1;
    }

    nth_matching_child(arena, parent, tag, expected) == Some(node)
}

fn has_position_within_parent_cached(
    arena: &Arena,
    node: NodeId,
    tag: &NameTest,
    expected: usize,
    position_cache: &mut RootPositionCache,
) -> bool {
    let parent = arena.get(node).parent;
    if parent.is_null() {
        return expected == 1;
    }

    position_cache.positioned_child(arena, parent, tag, expected) == Some(node)
}

struct RootPositionCache {
    positioned_children: HashMap<NodeId, Option<NodeId>>,
    #[cfg(test)]
    parent_scans: usize,
}

impl RootPositionCache {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            positioned_children: HashMap::with_capacity(capacity),
            #[cfg(test)]
            parent_scans: 0,
        }
    }

    fn positioned_child(
        &mut self,
        arena: &Arena,
        parent: NodeId,
        tag: &NameTest,
        expected: usize,
    ) -> Option<NodeId> {
        if let Some(&positioned) = self.positioned_children.get(&parent) {
            return positioned;
        }

        #[cfg(test)]
        {
            self.parent_scans += 1;
        }
        let positioned = nth_matching_child(arena, parent, tag, expected);
        self.positioned_children.insert(parent, positioned);
        positioned
    }
}

fn nth_matching_child(
    arena: &Arena,
    parent: NodeId,
    tag: &NameTest,
    expected: usize,
) -> Option<NodeId> {
    let mut position = 0usize;
    let mut sibling = arena.get(parent).first_child;
    while !sibling.is_null() {
        let sibling_node = arena.get(sibling);
        if is_element(sibling_node) && matches_name_test(arena, sibling, sibling_node, tag) {
            position += 1;
            if position == expected {
                return Some(sibling);
            }
        }
        sibling = sibling_node.next_sibling;
    }
    None
}

/// Evaluate an absolute path from the root.
///
/// Uses a single reusable buffer to avoid per-step Vec allocations.
/// Children are expanded in-place using a swap buffer.
fn evaluate_absolute_path(arena: &Arena, root: NodeId, steps: &[PathStep]) -> Vec<NodeId> {
    if steps.is_empty() {
        return vec![];
    }

    // Start from the root's children (the root itself is a synthetic wrapper).
    let mut current = Vec::new();
    collect_element_children(arena, root, &mut current);

    let mut next = Vec::new();
    let last_idx = steps.len() - 1;

    for (i, step) in steps.iter().enumerate() {
        next.clear();
        let mut position_parent = NodeId::NULL;
        let mut position = 0usize;
        for &node_id in &current {
            let n = arena.get(node_id);
            if !is_element(n) || !matches_name_test(arena, node_id, n, &step.tag) {
                continue;
            }

            // `current` contains each parent's children contiguously in DOM
            // order, so the position among nodes matching this step's name can
            // be maintained without rescanning preceding siblings.
            if n.parent == position_parent {
                position += 1;
            } else {
                position_parent = n.parent;
                position = 1;
            }
            if let Some(ref pred) = step.predicate {
                if !matches_predicate(arena, node_id, pred, position) {
                    continue;
                }
            }
            next.push(node_id);
        }

        if next.is_empty() {
            return vec![];
        }

        if i < last_idx {
            // Expand to children of matched nodes for the next step.
            current.clear();
            for &nid in &next {
                collect_element_children(arena, nid, &mut current);
            }
        } else {
            std::mem::swap(&mut current, &mut next);
        }
    }

    current
}

/// Collect direct element children of a node into `out` (no allocation).
#[inline]
fn collect_element_children(arena: &Arena, node: NodeId, out: &mut Vec<NodeId>) {
    let n = arena.get(node);
    let mut child = n.first_child;
    while !child.is_null() {
        let c = arena.get(child);
        if is_element(c) {
            out.push(child);
        }
        child = c.next_sibling;
    }
}

/// Check if a node matches a predicate.
fn matches_predicate(arena: &Arena, node: NodeId, pred: &Predicate, position: usize) -> bool {
    match pred {
        Predicate::AttrEquals { attr, value } => arena
            .attrs(node)
            .iter()
            .any(|a| arena.attr_name_is(a, attr) && value_is(arena, a, value)),

        Predicate::Contains { attr, substr } => arena.attrs(node).iter().any(|a| {
            arena.attr_name_is(a, attr)
                && arena.attr_value_len(a) >= substr.len()
                && arena.attr_value(a).unwrap_or("").contains(substr.as_str())
        }),

        Predicate::Position(pos) => position == *pos,

        Predicate::AttrExists { attr } => arena
            .attrs(node)
            .iter()
            .any(|a| arena.attr_name_is(a, attr)),
    }
}

/// Collect the direct child text nodes selected by XPath's `text()` node test.
fn collect_direct_text_children(arena: &Arena, node: NodeId, out: &mut Vec<NodeId>) {
    let n = arena.get(node);
    let mut child = n.first_child;
    while !child.is_null() {
        let child_node = arena.get(child);
        if child_node.flags.has(NodeFlags::IS_TEXT) {
            out.push(child);
        }
        child = child_node.next_sibling;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::parser::parse_xpath;

    fn eval(html: &str, xpath: &str) -> XPathResult {
        let doc = fhp_tree::parse(html).unwrap();
        let expr = parse_xpath(xpath).unwrap();
        evaluate(&expr, doc.arena(), doc.root_id())
    }

    #[test]
    fn eval_descendant_tag() {
        let result = eval("<div><p>Hello</p><p>World</p></div>", "//p");
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 2),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_descendant_attr() {
        let result = eval("<a href=\"x\">a</a><a href=\"y\">b</a>", "//a[@href='x']");
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_descendant_attr_exists() {
        let result = eval("<a href=\"x\">a</a><span>b</span>", "//a[@href]");
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_contains() {
        let result = eval(
            "<div class=\"nav-main\">a</div><div class=\"footer\">b</div>",
            "//div[contains(@class, 'nav')]",
        );
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_position() {
        let result = eval(
            "<ul><li>1</li><li>2</li><li>3</li></ul>",
            "//li[position()=2]",
        );
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_position_shorthand() {
        let result = eval("<ul><li>1</li><li>2</li><li>3</li></ul>", "//li[1]");
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn positioned_selection_roots_scan_their_shared_parent_once() {
        const WIDTH: usize = 1_024;
        let mut html = String::with_capacity(WIDTH * 9 + 16);
        html.push_str("<ul>");
        for _ in 0..WIDTH {
            html.push_str("<li></li>");
        }
        html.push_str("</ul>");

        let doc = fhp_tree::parse(&html).unwrap();
        let tag = NameTest::Interned(Tag::Li);
        let roots = collect_descendants(
            &XPathExpr::DescendantByTag(tag.clone()),
            doc.arena(),
            doc.root_id(),
            Collect::Elements,
        )
        .unwrap();
        let mut position_cache = RootPositionCache::with_capacity(roots.len());
        let nodes = find_descendants_by_tag_at_position_for_roots_cached::<false>(
            doc.arena(),
            &roots,
            &tag,
            WIDTH,
            &mut position_cache,
        );

        assert_eq!(nodes, vec![roots[WIDTH - 1]]);
        assert_eq!(position_cache.parent_scans, 1);
    }

    #[test]
    fn eval_text_extract() {
        let result = eval("<div><p>Hello</p><p>World</p></div>", "//p/text()");
        match result {
            XPathResult::Strings(texts) => {
                assert_eq!(texts.len(), 2);
                assert_eq!(texts[0], "Hello");
                assert_eq!(texts[1], "World");
            }
            _ => panic!("expected Strings"),
        }
    }

    #[test]
    fn eval_absolute_path() {
        let result = eval(
            "<html><body><div>content</div></body></html>",
            "/html/body/div",
        );
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_absolute_path_text() {
        let result = eval(
            "<html><body><p>text</p></body></html>",
            "/html/body/p/text()",
        );
        match result {
            XPathResult::Strings(texts) => {
                assert_eq!(texts.len(), 1);
                assert_eq!(texts[0], "text");
            }
            _ => panic!("expected Strings"),
        }
    }

    #[test]
    fn eval_wildcard() {
        let result = eval("<div><p>a</p><span>b</span></div>", "//*");
        match result {
            XPathResult::Nodes(nodes) => assert!(nodes.len() >= 3),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_wildcard_attr() {
        let result = eval("<div id=\"main\">a</div><span>b</span>", "//*[@id='main']");
        match result {
            XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_empty_result() {
        let result = eval("<div>text</div>", "//span");
        match result {
            XPathResult::Nodes(nodes) => assert!(nodes.is_empty()),
            _ => panic!("expected Nodes"),
        }
    }

    #[test]
    fn eval_position_out_of_range() {
        let result = eval("<ul><li>1</li></ul>", "//li[position()=5]");
        match result {
            XPathResult::Nodes(nodes) => assert!(nodes.is_empty()),
            _ => panic!("expected Nodes"),
        }
    }
}
