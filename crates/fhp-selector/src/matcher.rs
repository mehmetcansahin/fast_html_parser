//! Right-to-left CSS selector matching engine.
//!
//! Matching starts from the subject (rightmost compound) and walks
//! left through the combinator chain, checking ancestors, parents,
//! or siblings as required.
//!
//! When a selector contains descendant combinators, ancestor bloom
//! filters are used for fast rejection.

use fhp_core::hash::selector_hash;
use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::node::{NodeFlags, NodeId};

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

        SimpleSelector::Class(class_name, bloom_bit) => {
            // Fast rejection via per-node class bloom filter.
            if n.class_hash & bloom_bit == 0 {
                return false;
            }
            // Bloom matched — verify via real attribute scan.
            let attrs = arena.attrs(node);
            attrs.iter().any(|a| {
                arena.attr_name(a).eq_ignore_ascii_case("class")
                    && arena
                        .attr_value(a)
                        .is_some_and(|v| contains_class_token(v, class_name))
            })
        }

        SimpleSelector::Id(id, target_hash) => {
            // Fast rejection via per-node id hash. `id_hash == 0` doubles as the
            // "no id attribute" sentinel, so it cannot represent a real id that
            // hashes to 0; when the target itself hashes to 0 we skip the fast
            // path and verify directly to avoid a false negative.
            if id_hash_rejects(arena, node, *target_hash) {
                return false;
            }
            // Hash matched (or target hash is 0) — verify via real attribute
            // scan (collision possible).
            let attrs = arena.attrs(node);
            attrs.iter().any(|a| {
                arena.attr_name(a).eq_ignore_ascii_case("id")
                    && arena.attr_value(a) == Some(id.as_str())
            })
        }

        SimpleSelector::Universal => is_element(n),

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
        if !arena
            .attr_name(attr)
            .eq_ignore_ascii_case(sel.name.as_str())
        {
            continue;
        }
        // In HTML, a valueless (boolean) attribute has an empty string value
        // for selector matching. `Arena::attr_value` uses `None` only to
        // preserve whether the value was explicitly written.
        let val = arena.attr_value(attr).unwrap_or("");
        match sel.op {
            AttrOp::Exists => return true,
            AttrOp::Equals => {
                return Some(val) == sel.value.as_deref();
            }
            AttrOp::Includes => {
                if let Some(sel_val) = &sel.value {
                    return contains_class_token(val, sel_val.as_str());
                }
            }
            AttrOp::StartsWith => {
                if let Some(sel_val) = &sel.value {
                    return !sel_val.is_empty() && val.starts_with(sel_val.as_str());
                }
            }
            AttrOp::EndsWith => {
                if let Some(sel_val) = &sel.value {
                    return !sel_val.is_empty() && val.ends_with(sel_val.as_str());
                }
            }
            AttrOp::Substring => {
                if let Some(sel_val) = &sel.value {
                    return !sel_val.is_empty() && val.contains(sel_val.as_str());
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
///
/// Uses the precomputed full sibling index for O(1) lookup. A compact u16
/// mirror remains in `element_index` for layout compatibility.
fn is_nth_element_child(arena: &Arena, node: NodeId, a: i32, b: i32) -> bool {
    let n = arena.get(node);
    if n.parent.is_null() || n.element_index == 0 {
        return false;
    }

    let index = i64::from(if n.attr_raw_offset == 0 {
        u32::from(n.element_index)
    } else {
        n.attr_raw_offset
    });

    matches_nth(a, b, index)
}

/// Check if a 1-based `index` satisfies `an+b`.
///
/// Arithmetic is done in `i64` so an adversarial `b` (e.g. `n-2147483647`)
/// cannot overflow the `index - b` subtraction.
#[inline]
fn matches_nth(a: i32, b: i32, index: i64) -> bool {
    let (a, b) = (i64::from(a), i64::from(b));
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

/// Returns `true` for real element nodes (not the synthetic root wrapper,
/// text, comment, or doctype).
#[inline]
fn is_element(n: &fhp_tree::node::Node) -> bool {
    n.depth > 0
        && !n.flags.has(NodeFlags::IS_TEXT)
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

    // Step 2: walk the chain right-to-left with backtracking.
    match_chain(arena, node, &selector.chain)
}

/// Recursively match the remaining right-to-left combinator chain against
/// ancestors/siblings of `current`.
///
/// Descendant and general-sibling steps try every candidate and backtrack on
/// failure: committing to the nearest match (as a greedy walk does) drops valid
/// matches for selectors like `A > B C`, where the nearest `B` is not a child
/// of `A` but an outer `B` is. Child and adjacent-sibling steps are
/// deterministic (a single candidate) but still recurse on the chain remainder.
fn match_chain(arena: &Arena, current: NodeId, chain: &[(Combinator, CompoundSelector)]) -> bool {
    let Some(((combinator, compound), rest)) = chain.split_first() else {
        return true;
    };

    match combinator {
        Combinator::Descendant => {
            let mut ancestor = arena.get(current).parent;
            while !ancestor.is_null() {
                if match_compound(arena, ancestor, compound) && match_chain(arena, ancestor, rest) {
                    return true;
                }
                ancestor = arena.get(ancestor).parent;
            }
            false
        }

        Combinator::Child => {
            let parent = arena.get(current).parent;
            !parent.is_null()
                && match_compound(arena, parent, compound)
                && match_chain(arena, parent, rest)
        }

        Combinator::AdjacentSibling => match prev_element_sibling(arena, current) {
            Some(p) => match_compound(arena, p, compound) && match_chain(arena, p, rest),
            None => false,
        },

        Combinator::GeneralSibling => {
            let mut prev = arena.get(current).prev_sibling;
            while !prev.is_null() {
                let p = arena.get(prev);
                if is_element(p)
                    && match_compound(arena, prev, compound)
                    && match_chain(arena, prev, rest)
                {
                    return true;
                }
                prev = p.prev_sibling;
            }
            false
        }
    }
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
    let mut results = Vec::new();
    collect_list(arena, root, list, &mut results);
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

/// Single DFS for a selector list. Traversing the actual links, rather than
/// sorting allocation ids, preserves document order for public arenas whose
/// nodes were allocated before they were attached.
fn collect_list(arena: &Arena, node: NodeId, list: &SelectorList, results: &mut Vec<NodeId>) {
    if node.is_null() {
        return;
    }

    let current = arena.get(node);
    if is_element(current) && match_selector_list(arena, node, list) {
        results.push(node);
    }

    let mut child = current.first_child;
    while !child.is_null() {
        collect_list(arena, child, list, results);
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
                SimpleSelector::Class(class, _) => {
                    hashes.push(hash_str(class));
                }
                SimpleSelector::Id(id, _) => {
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
        [SimpleSelector::Id(id, _)] => Some(id.as_str()),
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
    let target_hash = selector_hash(target_id.as_bytes());
    // `id_hash == 0` is also the "no id" sentinel; when the target hashes to 0,
    // skip the fast path and verify directly (see SimpleSelector::Id).
    if id_hash_rejects(arena, node, target_hash) {
        return false;
    }
    arena.attrs(node).iter().any(|a| {
        arena.attr_name(a).eq_ignore_ascii_case("id") && arena.attr_value(a) == Some(target_id)
    })
}

/// Return whether the cached id hash can conclusively reject this node.
///
/// The arena stores one hash, while malformed HTML can retain duplicate `id`
/// attributes. A mismatch is therefore conclusive only when at most one `id`
/// attribute exists; duplicate attributes require the full value scan.
#[inline]
fn id_hash_rejects(arena: &Arena, node: NodeId, target_hash: u32) -> bool {
    let n = arena.get(node);
    if target_hash == 0 || n.id_hash == target_hash {
        return false;
    }

    arena
        .attrs(node)
        .iter()
        .filter(|attr| arena.attr_name(attr).eq_ignore_ascii_case("id"))
        .nth(1)
        .is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_selector;

    fn parse_and_match(html: &str, css: &str) -> Vec<NodeId> {
        let doc = fhp_tree::parse(html).unwrap();
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
        let doc = fhp_tree::parse("<div><p>a</p><p>b</p></div>").unwrap();
        let list = parse_selector("p").unwrap();
        let first = select_first_list(doc.arena(), doc.root_id(), &list).unwrap();
        assert_eq!(doc.get(first).text_content(), "a");
    }

    #[test]
    fn selector_list_uses_tree_order_not_allocation_order() {
        let mut arena = Arena::new();
        let root = arena.new_element(Tag::Unknown, 0);
        let span = arena.new_element(Tag::Span, 1);
        let div = arena.new_element(Tag::Div, 1);
        arena.append_child(root, div);
        arena.append_child(root, span);

        let list = parse_selector("span, div").unwrap();
        assert_eq!(select_all_list(&arena, root, &list), [div, span]);
        assert_eq!(select_first_list(&arena, root, &list), Some(div));
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
        let doc = fhp_tree::parse("<div id=\"dup\">a</div><span id=\"dup\">b</span>").unwrap();
        let list = parse_selector("#dup").unwrap();
        let first = select_first_list(doc.arena(), doc.root_id(), &list).unwrap();
        assert_eq!(doc.get(first).text_content(), "a");
    }

    #[test]
    fn match_universal() {
        let ids = parse_and_match("<div><p>text</p></div>", "*");
        assert_eq!(ids.len(), 2);
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
