//! XPath expression evaluator.
//!
//! Walks the DOM tree and collects nodes or text matching an [`XPathExpr`].

use hp_core::tag::Tag;
use hp_tree::arena::Arena;
use hp_tree::node::{NodeFlags, NodeId};

use super::ast::{PathStep, Predicate, XPathExpr, XPathResult};

/// Evaluate an XPath expression against an arena starting from `root`.
pub fn evaluate(expr: &XPathExpr, arena: &Arena, root: NodeId) -> XPathResult {
    match expr {
        XPathExpr::DescendantByTag(tag) => {
            let nodes = find_descendants_by_tag(arena, root, *tag);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::DescendantByAttr { tag, attr, value } => {
            let nodes = find_descendants_by_tag_and_attr(arena, root, *tag, attr, Some(value));
            XPathResult::Nodes(nodes)
        }

        XPathExpr::DescendantByAttrExists { tag, attr } => {
            let nodes = find_descendants_by_tag_and_attr_exists(arena, root, *tag, attr);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::ContainsPredicate { tag, attr, substr } => {
            let nodes = find_descendants_by_tag_and_contains(arena, root, *tag, attr, substr);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::PositionPredicate { tag, pos } => {
            let nodes = find_descendants_by_tag(arena, root, *tag);
            if *pos >= 1 && *pos <= nodes.len() {
                XPathResult::Nodes(vec![nodes[*pos - 1]])
            } else {
                XPathResult::Nodes(vec![])
            }
        }

        XPathExpr::AbsolutePath(steps) => {
            let nodes = evaluate_absolute_path(arena, root, steps);
            XPathResult::Nodes(nodes)
        }

        XPathExpr::TextExtract(inner) => {
            let inner_result = evaluate(inner, arena, root);
            match inner_result {
                XPathResult::Nodes(nodes) => {
                    let texts: Vec<String> =
                        nodes.iter().map(|&id| collect_text(arena, id)).collect();
                    XPathResult::Strings(texts)
                }
                other => other,
            }
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

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns `true` for element nodes (not text, comment, doctype).
#[inline]
fn is_element(n: &hp_tree::node::Node) -> bool {
    !n.flags.has(NodeFlags::IS_TEXT)
        && !n.flags.has(NodeFlags::IS_COMMENT)
        && !n.flags.has(NodeFlags::IS_DOCTYPE)
}

/// Generic DFS: collect descendant nodes that satisfy `predicate`.
fn dfs_collect(
    arena: &Arena,
    node: NodeId,
    predicate: &dyn Fn(&Arena, NodeId, &hp_tree::node::Node) -> bool,
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
fn find_descendants_by_tag(arena: &Arena, root: NodeId, tag: Tag) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|_, _, n| is_element(n) && n.tag == tag,
        &mut results,
    );
    results
}

/// DFS: find descendants by tag with exact attribute match (single pass).
fn find_descendants_by_tag_and_attr(
    arena: &Arena,
    root: NodeId,
    tag: Tag,
    attr: &str,
    value: Option<&str>,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| {
            is_element(n)
                && n.tag == tag
                && a.attrs(id)
                    .iter()
                    .any(|at| at.name == attr && at.value.as_deref() == value)
        },
        &mut results,
    );
    results
}

/// DFS: find descendants by tag with attribute existence (single pass).
fn find_descendants_by_tag_and_attr_exists(
    arena: &Arena,
    root: NodeId,
    tag: Tag,
    attr: &str,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| is_element(n) && n.tag == tag && a.attrs(id).iter().any(|at| at.name == attr),
        &mut results,
    );
    results
}

/// DFS: find descendants by tag with contains predicate (single pass).
fn find_descendants_by_tag_and_contains(
    arena: &Arena,
    root: NodeId,
    tag: Tag,
    attr: &str,
    substr: &str,
) -> Vec<NodeId> {
    let mut results = Vec::new();
    dfs_collect(
        arena,
        root,
        &|a, id, n| {
            is_element(n)
                && n.tag == tag
                && a.attrs(id).iter().any(|at| {
                    at.name == attr && at.value.as_ref().is_some_and(|v| v.contains(substr))
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
                Some(val) => a
                    .attrs(id)
                    .iter()
                    .any(|at| at.name == attr && at.value.as_deref() == Some(val)),
                None => a.attrs(id).iter().any(|at| at.name == attr),
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
fn evaluate_absolute_path(arena: &Arena, root: NodeId, steps: &[PathStep]) -> Vec<NodeId> {
    if steps.is_empty() {
        return vec![];
    }

    // Start from the root's children (the root itself is a synthetic wrapper).
    let mut current_nodes: Vec<NodeId> = direct_element_children(arena, root);

    for step in steps {
        let mut next = Vec::new();
        for &node_id in &current_nodes {
            let n = arena.get(node_id);
            if !is_element(n) || n.tag != step.tag {
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

        // For the next step, expand to the children of matched nodes.
        if steps.last() != Some(step) {
            let mut children = Vec::new();
            for &nid in &next {
                children.extend(direct_element_children(arena, nid));
            }
            current_nodes = children;
        } else {
            current_nodes = next;
        }
    }

    current_nodes
}

/// Get the direct element children of a node.
fn direct_element_children(arena: &Arena, node: NodeId) -> Vec<NodeId> {
    let n = arena.get(node);
    let mut children = Vec::new();
    let mut child = n.first_child;
    while !child.is_null() {
        let c = arena.get(child);
        if is_element(c) {
            children.push(child);
        }
        child = c.next_sibling;
    }
    children
}

/// Check if a node matches a predicate.
fn matches_predicate(arena: &Arena, node: NodeId, pred: &Predicate) -> bool {
    match pred {
        Predicate::AttrEquals { attr, value } => arena
            .attrs(node)
            .iter()
            .any(|a| a.name == *attr && a.value.as_deref() == Some(value)),

        Predicate::Contains { attr, substr } => arena.attrs(node).iter().any(|a| {
            a.name == *attr
                && a.value
                    .as_ref()
                    .is_some_and(|v| v.contains(substr.as_str()))
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
                if is_element(c) && c.tag == n.tag {
                    idx += 1;
                    if child == node {
                        return idx == *pos;
                    }
                }
                child = c.next_sibling;
            }
            false
        }

        Predicate::AttrExists { attr } => arena.attrs(node).iter().any(|a| a.name == *attr),
    }
}

/// Recursively collect text content from a node.
fn collect_text(arena: &Arena, node: NodeId) -> String {
    let mut out = String::new();
    collect_text_inner(arena, node, &mut out);
    out
}

/// DFS text collection helper.
fn collect_text_inner(arena: &Arena, node: NodeId, out: &mut String) {
    let n = arena.get(node);
    if n.flags.has(NodeFlags::IS_TEXT) {
        out.push_str(arena.text(node));
        return;
    }
    let mut child = n.first_child;
    while !child.is_null() {
        collect_text_inner(arena, child, out);
        child = arena.get(child).next_sibling;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::parser::parse_xpath;

    fn eval(html: &str, xpath: &str) -> XPathResult {
        let doc = hp_tree::parse(html).unwrap();
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
