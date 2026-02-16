//! Right-to-left CSS selector matching engine.
//!
//! Matching starts from the subject (rightmost compound) and walks
//! left through the combinator chain, checking ancestors, parents,
//! or siblings as required.
//!
//! When a selector contains descendant combinators, ancestor bloom
//! filters are used for fast rejection.

use hp_core::tag::Tag;
use hp_tree::arena::Arena;
use hp_tree::node::{NodeFlags, NodeId};

use crate::ast::{
    AttrOp, AttrSelector, Combinator, CompoundSelector, Selector, SelectorList, SimpleSelector,
};
use crate::bloom::{AncestorBloom, build_ancestor_blooms, hash_str, hash_tag};

// ---------------------------------------------------------------------------
// Simple & compound matching
// ---------------------------------------------------------------------------

/// Check if a node matches a compound selector (all parts must match).
#[inline]
fn match_compound(arena: &Arena, node: NodeId, compound: &CompoundSelector) -> bool {
    match compound.parts.as_slice() {
        [part] => match_simple(arena, node, part),
        parts => parts.iter().all(|part| match_simple(arena, node, part)),
    }
}

/// Check if a node matches a single simple selector.
#[inline]
fn match_simple(arena: &Arena, node: NodeId, selector: &SimpleSelector) -> bool {
    let n = arena.get(node);

    match selector {
        SimpleSelector::Tag(tag) => {
            // Tag::Unknown in a selector never matches — we can't distinguish
            // different unknown elements.
            if *tag == Tag::Unknown {
                return false;
            }
            n.tag == *tag
        }
        SimpleSelector::UnknownTag(tag_name) => {
            n.tag == Tag::Unknown
                && arena
                    .unknown_tag_name(node)
                    .is_some_and(|name| name.eq_ignore_ascii_case(tag_name))
        }

        SimpleSelector::Class(class_name) => {
            let attrs = arena.attrs(node);
            attrs.iter().any(|a| {
                a.name == "class"
                    && a.value
                        .as_ref()
                        .is_some_and(|v| contains_class_token(v, class_name))
            })
        }

        SimpleSelector::Id(id) => {
            let attrs = arena.attrs(node);
            attrs
                .iter()
                .any(|a| a.name == "id" && a.value.as_deref() == Some(id.as_str()))
        }

        SimpleSelector::Universal => {
            // Match any element (not text, comment, doctype).
            !n.flags.has(NodeFlags::IS_TEXT)
                && !n.flags.has(NodeFlags::IS_COMMENT)
                && !n.flags.has(NodeFlags::IS_DOCTYPE)
        }

        SimpleSelector::Attr(attr_sel) => match_attr(arena, node, attr_sel),

        SimpleSelector::PseudoFirstChild => is_first_element_child(arena, node),
        SimpleSelector::PseudoLastChild => is_last_element_child(arena, node),
        SimpleSelector::PseudoNthChild { a, b } => is_nth_element_child(arena, node, *a, *b),
        SimpleSelector::PseudoNot(inner) => !match_compound(arena, node, inner),
    }
}

/// Match an attribute selector against a node's attributes.
#[inline]
fn match_attr(arena: &Arena, node: NodeId, sel: &AttrSelector) -> bool {
    let attrs = arena.attrs(node);
    for attr in attrs {
        if attr.name != sel.name {
            continue;
        }
        match sel.op {
            AttrOp::Exists => return true,
            AttrOp::Equals => {
                return attr.value.as_deref() == sel.value.as_deref();
            }
            AttrOp::Includes => {
                if let (Some(val), Some(sel_val)) = (&attr.value, &sel.value) {
                    return val.split_whitespace().any(|w| w == sel_val.as_str());
                }
            }
            AttrOp::StartsWith => {
                if let (Some(val), Some(sel_val)) = (&attr.value, &sel.value) {
                    return val.starts_with(sel_val.as_str());
                }
            }
            AttrOp::EndsWith => {
                if let (Some(val), Some(sel_val)) = (&attr.value, &sel.value) {
                    return val.ends_with(sel_val.as_str());
                }
            }
            AttrOp::Substring => {
                if let (Some(val), Some(sel_val)) = (&attr.value, &sel.value) {
                    return val.contains(sel_val.as_str());
                }
            }
        }
    }
    false
}

/// Fast ASCII class-token matcher for `class="..."`.
///
/// CSS class lists are ASCII whitespace-separated in HTML practice; this avoids
/// Unicode-aware `split_whitespace()` overhead in the hot path.
#[inline]
fn contains_class_token(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    let mut i = 0usize;

    while i < h.len() {
        while i < h.len() && h[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= h.len() {
            break;
        }
        let start = i;
        while i < h.len() && !h[i].is_ascii_whitespace() {
            i += 1;
        }
        let len = i - start;
        if len == n.len() && &h[start..i] == n {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Pseudo-class helpers
// ---------------------------------------------------------------------------

/// Check if the node is the first element child of its parent.
fn is_first_element_child(arena: &Arena, node: NodeId) -> bool {
    let n = arena.get(node);
    if n.parent.is_null() {
        return false;
    }
    let parent = arena.get(n.parent);
    let mut child = parent.first_child;
    while !child.is_null() {
        let c = arena.get(child);
        if is_element(c) {
            return child == node;
        }
        child = c.next_sibling;
    }
    false
}

/// Check if the node is the last element child of its parent.
fn is_last_element_child(arena: &Arena, node: NodeId) -> bool {
    let n = arena.get(node);
    if n.parent.is_null() {
        return false;
    }
    let parent = arena.get(n.parent);
    let mut child = parent.last_child;
    while !child.is_null() {
        let c = arena.get(child);
        if is_element(c) {
            return child == node;
        }
        child = c.prev_sibling;
    }
    false
}

/// Check if the node is the nth element child (1-based) matching `an+b`.
fn is_nth_element_child(arena: &Arena, node: NodeId, a: i32, b: i32) -> bool {
    let n = arena.get(node);
    if n.parent.is_null() {
        return false;
    }
    let parent = arena.get(n.parent);
    let mut child = parent.first_child;
    let mut index: i32 = 0;
    while !child.is_null() {
        let c = arena.get(child);
        if is_element(c) {
            index += 1;
            if child == node {
                return matches_nth(a, b, index);
            }
        }
        child = c.next_sibling;
    }
    false
}

/// Check if a 1-based `index` satisfies `an+b`.
#[inline]
fn matches_nth(a: i32, b: i32, index: i32) -> bool {
    if a == 0 {
        return index == b;
    }
    let diff = index - b;
    // diff must be a non-negative multiple of a (if a > 0),
    // or a non-positive multiple (if a < 0).
    if diff == 0 {
        return true;
    }
    // diff / a must be a non-negative integer.
    diff % a == 0 && (diff / a) >= 0
}

/// Returns `true` for element nodes (not text, comment, doctype).
#[inline]
fn is_element(n: &hp_tree::node::Node) -> bool {
    !n.flags.has(NodeFlags::IS_TEXT)
        && !n.flags.has(NodeFlags::IS_COMMENT)
        && !n.flags.has(NodeFlags::IS_DOCTYPE)
}

// ---------------------------------------------------------------------------
// Complex selector matching (right-to-left)
// ---------------------------------------------------------------------------

/// Match a single complex selector against a node using right-to-left matching.
pub fn match_selector(arena: &Arena, node: NodeId, selector: &Selector) -> bool {
    // Step 1: match the subject (rightmost compound).
    if !match_compound(arena, node, &selector.subject) {
        return false;
    }

    // Step 2: walk the chain right-to-left.
    let mut current = node;
    for (combinator, compound) in &selector.chain {
        match combinator {
            Combinator::Descendant => {
                let n = arena.get(current);
                let mut ancestor = n.parent;
                let mut found = false;
                while !ancestor.is_null() {
                    if match_compound(arena, ancestor, compound) {
                        current = ancestor;
                        found = true;
                        break;
                    }
                    ancestor = arena.get(ancestor).parent;
                }
                if !found {
                    return false;
                }
            }

            Combinator::Child => {
                let n = arena.get(current);
                if n.parent.is_null() || !match_compound(arena, n.parent, compound) {
                    return false;
                }
                current = n.parent;
            }

            Combinator::AdjacentSibling => {
                // Find the immediately preceding *element* sibling.
                let prev = prev_element_sibling(arena, current);
                match prev {
                    Some(p) if match_compound(arena, p, compound) => {
                        current = p;
                    }
                    _ => return false,
                }
            }

            Combinator::GeneralSibling => {
                // Find any preceding element sibling that matches.
                let n = arena.get(current);
                let mut prev = n.prev_sibling;
                let mut found = false;
                while !prev.is_null() {
                    let p = arena.get(prev);
                    if is_element(p) && match_compound(arena, prev, compound) {
                        current = prev;
                        found = true;
                        break;
                    }
                    prev = p.prev_sibling;
                }
                if !found {
                    return false;
                }
            }
        }
    }

    true
}

/// Match a selector list: a node matches if it matches ANY selector.
pub fn match_selector_list(arena: &Arena, node: NodeId, list: &SelectorList) -> bool {
    list.selectors
        .iter()
        .any(|sel| match_selector(arena, node, sel))
}

/// Find the immediately preceding element sibling (skipping text/comments).
fn prev_element_sibling(arena: &Arena, node: NodeId) -> Option<NodeId> {
    let n = arena.get(node);
    let mut prev = n.prev_sibling;
    while !prev.is_null() {
        let p = arena.get(prev);
        if is_element(p) {
            return Some(prev);
        }
        prev = p.prev_sibling;
    }
    None
}

// ---------------------------------------------------------------------------
// Selection: collect matching nodes
// ---------------------------------------------------------------------------

/// Select all nodes matching a single selector (no bloom optimization).
pub fn select_all(arena: &Arena, root: NodeId, selector: &Selector) -> Vec<NodeId> {
    select_all_with_blooms(arena, root, selector, None)
}

/// Select all nodes matching a single selector, optionally reusing precomputed
/// ancestor blooms.
fn select_all_with_blooms(
    arena: &Arena,
    root: NodeId,
    selector: &Selector,
    shared_blooms: Option<&[AncestorBloom]>,
) -> Vec<NodeId> {
    if let Some(tag) = simple_tag_selector(selector) {
        let mut results = Vec::new();
        collect_tag(arena, root, tag, &mut results);
        return results;
    }

    if let Some(id) = simple_id_selector(selector) {
        let mut results = Vec::new();
        collect_id(arena, root, id, &mut results);
        return results;
    }

    let has_descendant = has_descendant_combinator(selector);

    if has_descendant {
        let local_blooms;
        let blooms = if let Some(b) = shared_blooms {
            b
        } else {
            local_blooms = build_ancestor_blooms(arena, root);
            &local_blooms
        };
        let bloom_hashes = compute_descendant_hashes(selector);
        let mut results = Vec::new();
        collect_bloom(arena, root, selector, blooms, &bloom_hashes, &mut results);
        results
    } else {
        let mut results = Vec::new();
        collect_simple(arena, root, selector, &mut results);
        results
    }
}

/// Select all nodes matching a selector list.
pub fn select_all_list(arena: &Arena, root: NodeId, list: &SelectorList) -> Vec<NodeId> {
    if list.selectors.len() == 1 {
        return select_all(arena, root, &list.selectors[0]);
    }
    let has_any_descendant = list.selectors.iter().any(has_descendant_combinator);
    let shared_blooms = has_any_descendant.then(|| build_ancestor_blooms(arena, root));
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for sel in &list.selectors {
        for id in select_all_with_blooms(arena, root, sel, shared_blooms.as_deref()) {
            if seen.insert(id) {
                results.push(id);
            }
        }
    }
    results
}

#[inline]
fn has_descendant_combinator(selector: &Selector) -> bool {
    selector
        .chain
        .iter()
        .any(|(c, _)| *c == Combinator::Descendant)
}

/// Select the first matching node.
pub fn select_first(arena: &Arena, root: NodeId, selector: &Selector) -> Option<NodeId> {
    if let Some(tag) = simple_tag_selector(selector) {
        return find_first_tag(arena, root, tag);
    }
    if let Some(id) = simple_id_selector(selector) {
        return find_first_id(arena, root, id);
    }
    find_first(arena, root, selector)
}

/// Select the first matching node from a selector list.
pub fn select_first_list(arena: &Arena, root: NodeId, list: &SelectorList) -> Option<NodeId> {
    // DFS finds the first match in document order.
    find_first_list(arena, root, list)
}

// ---------------------------------------------------------------------------
// Internal collection helpers
// ---------------------------------------------------------------------------

/// DFS collection without bloom.
fn collect_simple(arena: &Arena, node: NodeId, selector: &Selector, results: &mut Vec<NodeId>) {
    if node.is_null() {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && match_selector(arena, node, selector) {
        results.push(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_simple(arena, child, selector, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS collection optimized for a single `#id` selector.
fn collect_id(arena: &Arena, node: NodeId, target_id: &str, results: &mut Vec<NodeId>) {
    if node.is_null() {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && node_has_id(arena, node, target_id) {
        results.push(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_id(arena, child, target_id, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS collection optimized for a single tag selector.
fn collect_tag(arena: &Arena, node: NodeId, target_tag: Tag, results: &mut Vec<NodeId>) {
    if node.is_null() {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && n.tag == target_tag {
        results.push(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_tag(arena, child, target_tag, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS collection with bloom pre-filtering.
fn collect_bloom(
    arena: &Arena,
    node: NodeId,
    selector: &Selector,
    blooms: &[AncestorBloom],
    bloom_hashes: &[u32],
    results: &mut Vec<NodeId>,
) {
    if node.is_null() {
        return;
    }

    let n = arena.get(node);
    if is_element(n) {
        // Quick bloom check: ancestors must contain the required elements.
        let bloom = &blooms[node.index()];
        let bloom_pass = bloom_hashes.iter().all(|&h| bloom.may_contain(h));

        if bloom_pass && match_selector(arena, node, selector) {
            results.push(node);
        }
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_bloom(arena, child, selector, blooms, bloom_hashes, results);
        child = arena.get(child).next_sibling;
    }
}

/// Compute bloom hashes for all descendant-combinator compounds in the chain.
fn compute_descendant_hashes(selector: &Selector) -> Vec<u32> {
    let mut hashes = Vec::new();
    for (combinator, compound) in &selector.chain {
        if *combinator != Combinator::Descendant {
            continue;
        }
        for part in &compound.parts {
            match part {
                SimpleSelector::Tag(tag) if *tag != Tag::Unknown => {
                    hashes.push(hash_tag(*tag));
                }
                // Ancestor blooms store only hashed Tag discriminants, not
                // unknown/custom element names. Hashing the literal unknown
                // tag name here causes false negatives for descendant checks
                // (e.g. `my-widget span`), so skip UnknownTag pre-filtering.
                SimpleSelector::UnknownTag(_) => {}
                SimpleSelector::Class(class) => {
                    hashes.push(hash_str(class));
                }
                SimpleSelector::Id(id) => {
                    hashes.push(hash_str(id));
                }
                _ => {}
            }
        }
    }
    hashes
}

/// DFS to find first matching node.
fn find_first(arena: &Arena, node: NodeId, selector: &Selector) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n) && match_selector(arena, node, selector) {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first(arena, child, selector) {
            return Some(found);
        }
        child = arena.get(child).next_sibling;
    }
    None
}

/// DFS to find first match optimized for a single `#id` selector.
fn find_first_id(arena: &Arena, node: NodeId, target_id: &str) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n) && node_has_id(arena, node, target_id) {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first_id(arena, child, target_id) {
            return Some(found);
        }
        child = arena.get(child).next_sibling;
    }
    None
}

/// DFS to find first match optimized for a single tag selector.
fn find_first_tag(arena: &Arena, node: NodeId, target_tag: Tag) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n) && n.tag == target_tag {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first_tag(arena, child, target_tag) {
            return Some(found);
        }
        child = arena.get(child).next_sibling;
    }
    None
}

/// DFS to find first matching node for a selector list.
fn find_first_list(arena: &Arena, node: NodeId, list: &SelectorList) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n) && match_selector_list(arena, node, list) {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first_list(arena, child, list) {
            return Some(found);
        }
        child = arena.get(child).next_sibling;
    }
    None
}

#[inline]
fn simple_id_selector(selector: &Selector) -> Option<&str> {
    if !selector.chain.is_empty() {
        return None;
    }
    match selector.subject.parts.as_slice() {
        [SimpleSelector::Id(id)] => Some(id.as_str()),
        _ => None,
    }
}

#[inline]
fn simple_tag_selector(selector: &Selector) -> Option<Tag> {
    if !selector.chain.is_empty() {
        return None;
    }
    match selector.subject.parts.as_slice() {
        [SimpleSelector::Tag(tag)] if *tag != Tag::Unknown => Some(*tag),
        _ => None,
    }
}

#[inline]
fn node_has_id(arena: &Arena, node: NodeId, target_id: &str) -> bool {
    arena
        .attrs(node)
        .iter()
        .any(|a| a.name == "id" && a.value.as_deref() == Some(target_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_selector;

    fn parse_and_match(html: &str, css: &str) -> Vec<NodeId> {
        let doc = hp_tree::parse(html).unwrap();
        let list = parse_selector(css).unwrap();
        select_all_list(doc.arena(), doc.root_id(), &list)
    }

    #[test]
    fn match_tag() {
        let ids = parse_and_match("<div><p>text</p></div>", "p");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_tag_multiple_nodes() {
        let ids = parse_and_match("<div><p>a</p><p>b</p></div>", "p");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn match_tag_first_is_document_order() {
        let doc = hp_tree::parse("<div><p>a</p><p>b</p></div>").unwrap();
        let list = parse_selector("p").unwrap();
        let first = select_first_list(doc.arena(), doc.root_id(), &list).unwrap();
        assert_eq!(doc.get(first).text_content(), "a");
    }

    #[test]
    fn match_class() {
        let ids = parse_and_match("<div class=\"a\"><span class=\"b\">x</span></div>", ".b");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_id() {
        let ids = parse_and_match("<div id=\"main\">x</div>", "#main");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_id_duplicates() {
        let ids = parse_and_match("<div id=\"dup\">a</div><span id=\"dup\">b</span>", "#dup");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn match_id_first_is_document_order() {
        let doc = hp_tree::parse("<div id=\"dup\">a</div><span id=\"dup\">b</span>").unwrap();
        let list = parse_selector("#dup").unwrap();
        let first = select_first_list(doc.arena(), doc.root_id(), &list).unwrap();
        assert_eq!(doc.get(first).text_content(), "a");
    }

    #[test]
    fn match_universal() {
        let ids = parse_and_match("<div><p>text</p></div>", "*");
        // root (Unknown), div, p — all elements match *
        assert!(ids.len() >= 2); // at least div and p
    }

    #[test]
    fn match_descendant() {
        let ids = parse_and_match("<div><p>a</p></div><p>b</p>", "div p");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_descendant_with_unknown_ancestor() {
        let ids = parse_and_match(
            "<my-widget><span>a</span></my-widget><span>b</span>",
            "my-widget span",
        );
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_child() {
        let ids = parse_and_match("<div><p>a</p><span><p>b</p></span></div>", "div > p");
        // Only the direct child <p>, not the nested one.
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_adjacent_sibling() {
        let ids = parse_and_match(
            "<div><h1>title</h1><p>first</p><p>second</p></div>",
            "h1 + p",
        );
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_general_sibling() {
        let ids = parse_and_match(
            "<div><h1>title</h1><p>first</p><p>second</p></div>",
            "h1 ~ p",
        );
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn match_compound_tag_class() {
        let ids = parse_and_match("<div class=\"a\"><div class=\"b\">x</div></div>", "div.b");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_attr_exists() {
        let ids = parse_and_match("<a href=\"x\">link</a><span>text</span>", "[href]");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_attr_equals() {
        let ids = parse_and_match("<a href=\"x\">a</a><a href=\"y\">b</a>", "[href=\"x\"]");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_attr_starts_with() {
        let ids = parse_and_match(
            "<a href=\"https://a\">a</a><a href=\"http://b\">b</a>",
            "[href^=\"https\"]",
        );
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_attr_ends_with() {
        let ids = parse_and_match(
            "<a href=\"a.html\">a</a><a href=\"b.php\">b</a>",
            "[href$=\".html\"]",
        );
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_attr_substring() {
        let ids = parse_and_match(
            "<a href=\"https://example.com\">a</a><a href=\"other\">b</a>",
            "[href*=\"example\"]",
        );
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_first_child() {
        let ids = parse_and_match("<ul><li>1</li><li>2</li><li>3</li></ul>", "li:first-child");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_last_child() {
        let ids = parse_and_match("<ul><li>1</li><li>2</li><li>3</li></ul>", "li:last-child");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_nth_child_odd() {
        let ids = parse_and_match(
            "<ul><li>1</li><li>2</li><li>3</li><li>4</li></ul>",
            "li:nth-child(odd)",
        );
        assert_eq!(ids.len(), 2); // 1st and 3rd
    }

    #[test]
    fn match_nth_child_even() {
        let ids = parse_and_match(
            "<ul><li>1</li><li>2</li><li>3</li><li>4</li></ul>",
            "li:nth-child(even)",
        );
        assert_eq!(ids.len(), 2); // 2nd and 4th
    }

    #[test]
    fn match_nth_child_number() {
        let ids = parse_and_match("<ul><li>1</li><li>2</li><li>3</li></ul>", "li:nth-child(2)");
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn match_not() {
        let ids = parse_and_match(
            "<div class=\"a\">x</div><div class=\"b\">y</div><div>z</div>",
            "div:not(.a)",
        );
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn match_comma_list() {
        let ids = parse_and_match("<div>a</div><span>b</span><p>c</p>", "div, span");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn matches_nth_formula() {
        // 2n+1: 1, 3, 5, ...
        assert!(matches_nth(2, 1, 1));
        assert!(!matches_nth(2, 1, 2));
        assert!(matches_nth(2, 1, 3));

        // 0n+3: only 3
        assert!(!matches_nth(0, 3, 1));
        assert!(matches_nth(0, 3, 3));

        // 3n: 3, 6, 9
        assert!(matches_nth(3, 0, 3));
        assert!(matches_nth(3, 0, 6));
        assert!(!matches_nth(3, 0, 1));
    }

    #[test]
    fn class_token_exact_word() {
        assert!(contains_class_token("a b c", "b"));
        assert!(!contains_class_token("a bc d", "b"));
        assert!(!contains_class_token("abc", "b"));
    }

    #[test]
    fn class_token_ascii_whitespace() {
        assert!(contains_class_token("a\tb\nc", "b"));
        assert!(contains_class_token("  a   b  ", "a"));
        assert!(!contains_class_token("  a   b  ", "c"));
    }
}
