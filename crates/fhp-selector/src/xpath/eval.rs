//! XPath expression evaluator.
//!
//! Walks the DOM tree and collects nodes or text matching an [`XPathExpr`](crate::xpath::ast::XPathExpr).

use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::node::{NodeFlags, NodeId};

use super::ast::{NameTest, PathStep, Predicate, XPathExpr, XPathResult};

/// Evaluate an XPath expression against an arena starting from `root`.
pub fn evaluate(expr: &XPathExpr, arena: &Arena, root: NodeId) -> XPathResult {
    match expr {
        XPathExpr::DescendantByTag(tag) => {
            let nodes = find_descendants_by_tag(arena, root, tag);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::DescendantByAttr { tag, attr, value } => {
            let nodes = find_descendants_by_tag_and_attr(arena, root, tag, attr, Some(value));
            XPathResult::Nodes(nodes)
        }

        XPathExpr::DescendantByAttrExists { tag, attr } => {
            let nodes = find_descendants_by_tag_and_attr_exists(arena, root, tag, attr);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::ContainsPredicate { tag, attr, substr } => {
            let nodes = find_descendants_by_tag_and_contains(arena, root, tag, attr, substr);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::PositionPredicate { tag, pos } => {
            let predicate = Predicate::Position(*pos);
            let nodes = find_descendants_by_tag(arena, root, tag)
                .into_iter()
                .filter(|&node| matches_predicate(arena, node, &predicate))
                .collect();
            XPathResult::Nodes(nodes)
        }

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

        XPathExpr::DescendantWildcard => {
            let nodes = find_all_elements(arena, root);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::DescendantWildcardByAttr { attr, value } => {
            let nodes = find_all_elements_by_attr(arena, root, attr, Some(value));
            XPathResult::Nodes(nodes)
        }

        XPathExpr::DescendantWildcardByAttrExists { attr } => {
            let nodes = find_all_elements_by_attr(arena, root, attr, None);
            XPathResult::Nodes(nodes)
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

#[inline]
fn same_element_name(arena: &Arena, left: NodeId, right: NodeId) -> bool {
    let left_node = arena.get(left);
    let right_node = arena.get(right);
    if left_node.tag != right_node.tag {
        return false;
    }
    if left_node.tag != Tag::Unknown {
        return true;
    }
    match (arena.unknown_tag_name(left), arena.unknown_tag_name(right)) {
        (Some(left_name), Some(right_name)) => left_name.eq_ignore_ascii_case(right_name),
        _ => false,
    }
}

/// Generic DFS: collect descendant nodes that satisfy `predicate`.
fn dfs_collect(
    arena: &Arena,
    node: NodeId,
    predicate: &dyn Fn(&Arena, NodeId, &fhp_tree::node::Node) -> bool,
    results: &mut Vec<NodeId>,
) {
    if node.is_null() {
        return;
    }
    let n = arena.get(node);
    if predicate(arena, node, n) {
        results.push(node);
    }
    let mut child = n.first_child;
    while !child.is_null() {
        dfs_collect(arena, child, predicate, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS: find all descendant elements with a specific tag.
fn find_descendants_by_tag(arena: &Arena, root: NodeId, tag: &NameTest) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| is_element(n) && matches_name_test(a, id, n, tag),
        &mut results,
    );
    results
}

/// DFS: find descendants by tag with exact attribute match (single pass).
fn find_descendants_by_tag_and_attr(
    arena: &Arena,
    root: NodeId,
    tag: &NameTest,
    attr: &str,
    value: Option<&str>,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| {
            is_element(n)
                && matches_name_test(a, id, n, tag)
                && a.attrs(id).iter().any(|at| {
                    a.attr_name(at).eq_ignore_ascii_case(attr)
                        && value.is_some_and(|expected| a.attr_value(at).unwrap_or("") == expected)
                })
        },
        &mut results,
    );
    results
}

/// DFS: find descendants by tag with attribute existence (single pass).
fn find_descendants_by_tag_and_attr_exists(
    arena: &Arena,
    root: NodeId,
    tag: &NameTest,
    attr: &str,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| {
            is_element(n)
                && matches_name_test(a, id, n, tag)
                && a.attrs(id)
                    .iter()
                    .any(|at| a.attr_name(at).eq_ignore_ascii_case(attr))
        },
        &mut results,
    );
    results
}

/// DFS: find descendants by tag with contains predicate (single pass).
fn find_descendants_by_tag_and_contains(
    arena: &Arena,
    root: NodeId,
    tag: &NameTest,
    attr: &str,
    substr: &str,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| {
            is_element(n)
                && matches_name_test(a, id, n, tag)
                && a.attrs(id).iter().any(|at| {
                    a.attr_name(at).eq_ignore_ascii_case(attr)
                        && a.attr_value(at).unwrap_or("").contains(substr)
                })
        },
        &mut results,
    );
    results
}

/// DFS: find all descendant elements with optional attribute filter (single pass).
fn find_all_elements_by_attr(
    arena: &Arena,
    root: NodeId,
    attr: &str,
    value: Option<&str>,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| {
            if !is_element(n) || n.depth == 0 {
                return false;
            }
            match value {
                Some(val) => a.attrs(id).iter().any(|at| {
                    a.attr_name(at).eq_ignore_ascii_case(attr)
                        && a.attr_value(at).unwrap_or("") == val
                }),
                None => a
                    .attrs(id)
                    .iter()
                    .any(|at| a.attr_name(at).eq_ignore_ascii_case(attr)),
            }
        },
        &mut results,
    );
    results
}

/// DFS: find all descendant elements.
fn find_all_elements(arena: &Arena, root: NodeId) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|_, _, n| is_element(n) && n.depth > 0,
        &mut results,
    );
    results
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
        for &node_id in &current {
            let n = arena.get(node_id);
            if !is_element(n) || !matches_name_test(arena, node_id, n, &step.tag) {
                continue;
            }
            if let Some(ref pred) = step.predicate {
                if !matches_predicate(arena, node_id, pred) {
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
fn matches_predicate(arena: &Arena, node: NodeId, pred: &Predicate) -> bool {
    match pred {
        Predicate::AttrEquals { attr, value } => arena.attrs(node).iter().any(|a| {
            arena.attr_name(a).eq_ignore_ascii_case(attr)
                && arena.attr_value(a).unwrap_or("") == value
        }),

        Predicate::Contains { attr, substr } => arena.attrs(node).iter().any(|a| {
            arena.attr_name(a).eq_ignore_ascii_case(attr)
                && arena.attr_value(a).unwrap_or("").contains(substr.as_str())
        }),

        Predicate::Position(pos) => {
            // 1-based position among siblings of same type.
            let n = arena.get(node);
            if n.parent.is_null() {
                return *pos == 1;
            }
            let parent = arena.get(n.parent);
            let mut child = parent.first_child;
            let mut idx = 0usize;
            while !child.is_null() {
                let c = arena.get(child);
                if is_element(c) && same_element_name(arena, child, node) {
                    idx += 1;
                    if child == node {
                        return idx == *pos;
                    }
                }
                child = c.next_sibling;
            }
            false
        }

        Predicate::AttrExists { attr } => arena
            .attrs(node)
            .iter()
            .any(|a| arena.attr_name(a).eq_ignore_ascii_case(attr)),
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
