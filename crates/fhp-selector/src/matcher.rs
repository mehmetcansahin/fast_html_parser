//! Right-to-left CSS selector matching engine.
//!
//! Matching starts from the subject (rightmost compound) and walks
//! left through the combinator chain, checking ancestors, parents,
//! or siblings as required.
//!
//! When a selector contains descendant combinators, an ancestor filter
//! carried down the selection walk is used for fast rejection.

use std::collections::HashMap;

use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::node::{NodeFlags, NodeId};

use crate::ast::{
    AttrOp, AttrSelector, Combinator, CompoundSelector, Selector, SelectorList, SimpleSelector,
};
use crate::bloom::AncestorFilter;
use crate::space::contains_html_space_token;

// ---------------------------------------------------------------------------
// Simple & compound matching
// ---------------------------------------------------------------------------

/// Check if a node matches a compound selector (all parts must match).
///
/// Only elements match: the synthetic document root, text, comment, and
/// doctype nodes never do, so `:not(div)` cannot match the root either.
#[inline]
fn match_compound(arena: &Arena, node: NodeId, compound: &CompoundSelector) -> bool {
    if !is_element(arena.get(node)) {
        return false;
    }
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
                        .is_some_and(|v| contains_html_space_token(v, class_name))
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

        // `match_compound` only passes elements.
        SimpleSelector::Universal => true,

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
                    return contains_html_space_token(val, sel_val.as_str());
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

/// Largest memo table represented as a dense tri-state byte array.
///
/// Larger `(node, chain-index)` spaces use a sparse map so a selector cannot
/// force an unbounded up-front allocation on a large document.
const DENSE_MEMO_STATE_LIMIT: usize = 8_388_608;

const MEMO_UNKNOWN: u8 = 0;
const MEMO_FALSE: u8 = 1;
const MEMO_TRUE: u8 = 2;

enum MatchMemo {
    Empty,
    Dense { width: usize, states: Vec<u8> },
    Sparse(HashMap<(NodeId, usize), bool>),
}

impl MatchMemo {
    #[cfg(test)]
    fn new(node_count: usize, chain_len: usize) -> Self {
        Self::new_with_dense(node_count, chain_len, true)
    }

    fn new_with_dense(node_count: usize, chain_len: usize, allow_dense: bool) -> Self {
        if chain_len == 0 {
            return Self::Empty;
        }

        match node_count.checked_mul(chain_len) {
            Some(state_count) if allow_dense && state_count <= DENSE_MEMO_STATE_LIMIT => {
                Self::Dense {
                    width: chain_len,
                    states: vec![MEMO_UNKNOWN; state_count],
                }
            }
            _ => Self::Sparse(HashMap::new()),
        }
    }

    #[inline]
    fn get(&self, node: NodeId, chain_index: usize) -> Option<bool> {
        match self {
            Self::Empty => None,
            Self::Dense { width, states } => {
                let state = states[node.index() * width + chain_index];
                match state {
                    MEMO_FALSE => Some(false),
                    MEMO_TRUE => Some(true),
                    _ => None,
                }
            }
            Self::Sparse(states) => states.get(&(node, chain_index)).copied(),
        }
    }

    #[inline]
    fn insert(&mut self, node: NodeId, chain_index: usize, matched: bool) {
        match self {
            Self::Empty => {}
            Self::Dense { width, states } => {
                states[node.index() * *width + chain_index] =
                    if matched { MEMO_TRUE } else { MEMO_FALSE };
            }
            Self::Sparse(states) => {
                states.insert((node, chain_index), matched);
            }
        }
    }

    #[cfg(test)]
    fn populated_len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Dense { states, .. } => states
                .iter()
                .filter(|&&state| state != MEMO_UNKNOWN)
                .count(),
            Self::Sparse(states) => states.len(),
        }
    }
}

/// Selector-specific matcher whose memo table is reused across every subject
/// candidate visited by a selection traversal.
struct SelectorMatcher<'a> {
    selector: &'a Selector,
    memo: MatchMemo,
}

impl<'a> SelectorMatcher<'a> {
    fn new(node_count: usize, selector: &'a Selector) -> Self {
        Self::new_with_dense(node_count, selector, true)
    }

    fn new_with_dense(node_count: usize, selector: &'a Selector, allow_dense: bool) -> Self {
        Self {
            selector,
            memo: MatchMemo::new_with_dense(node_count, selector.chain.len(), allow_dense),
        }
    }

    #[inline]
    fn matches(&mut self, arena: &Arena, node: NodeId) -> bool {
        if !match_compound(arena, node, &self.selector.subject) {
            return false;
        }
        self.match_chain(arena, node, 0)
    }

    fn match_chain(&mut self, arena: &Arena, current: NodeId, chain_index: usize) -> bool {
        let selector = self.selector;
        let Some((combinator, compound)) = selector.chain.get(chain_index) else {
            return true;
        };

        if let Some(matched) = self.memo.get(current, chain_index) {
            return matched;
        }

        let next_index = chain_index + 1;
        let matched = match combinator {
            Combinator::Descendant => {
                let mut ancestor = arena.get(current).parent;
                let mut found = false;
                while !ancestor.is_null() {
                    if match_compound(arena, ancestor, compound)
                        && self.match_chain(arena, ancestor, next_index)
                    {
                        found = true;
                        break;
                    }
                    ancestor = arena.get(ancestor).parent;
                }
                found
            }

            Combinator::Child => {
                let parent = arena.get(current).parent;
                !parent.is_null()
                    && match_compound(arena, parent, compound)
                    && self.match_chain(arena, parent, next_index)
            }

            Combinator::AdjacentSibling => match prev_element_sibling(arena, current) {
                Some(previous) => {
                    match_compound(arena, previous, compound)
                        && self.match_chain(arena, previous, next_index)
                }
                None => false,
            },

            Combinator::GeneralSibling => {
                self.match_general_sibling(arena, current, chain_index, compound)
            }
        };

        self.memo.insert(current, chain_index, matched);
        matched
    }

    /// Evaluate `compound ~ current` at `chain_index` in amortized O(1) per
    /// element sibling.
    ///
    /// With `prev` the preceding element sibling of `node`, the chain value
    /// is `chain(node) = chain(prev) || (compound(prev) && match_chain(prev,
    /// chain_index + 1))`, and the first element sibling's value is `false`.
    /// Walk back to the nearest sibling with a memoized value (or to the first
    /// element sibling), then forward to `current`, memoizing every sibling on
    /// the way, so each sibling is evaluated once per `(parent, chain_index)`.
    fn match_general_sibling(
        &mut self,
        arena: &Arena,
        current: NodeId,
        chain_index: usize,
        compound: &CompoundSelector,
    ) -> bool {
        let next_index = chain_index + 1;

        // Invariant after the backward walk: `matched == chain(node)`.
        let mut node = current;
        let mut matched = false;
        while let Some(previous) = prev_element_sibling(arena, node) {
            node = previous;
            if let Some(known) = self.memo.get(previous, chain_index) {
                matched = known;
                break;
            }
        }

        while node != current {
            if !matched {
                matched = match_compound(arena, node, compound)
                    && self.match_chain(arena, node, next_index);
            }
            let Some(next) = next_element_sibling(arena, node) else {
                break;
            };
            self.memo.insert(next, chain_index, matched);
            node = next;
        }
        matched
    }
}

/// Build all branch matchers while keeping the aggregate dense memo allocation
/// within the same global state bound. If the selector list would exceed the
/// bound, every branch uses sparse storage rather than allocating one maximum-
/// sized dense table per branch.
fn build_list_matchers<'a>(
    node_count: usize,
    selectors: &'a [Selector],
) -> Vec<SelectorMatcher<'a>> {
    let total_states = selectors.iter().try_fold(0usize, |total, selector| {
        let branch_states = node_count.checked_mul(selector.chain.len())?;
        total.checked_add(branch_states)
    });
    let allow_dense = total_states.is_some_and(|states| states <= DENSE_MEMO_STATE_LIMIT);

    selectors
        .iter()
        .map(|selector| SelectorMatcher::new_with_dense(node_count, selector, allow_dense))
        .collect()
}

/// Match a single complex selector against a node using right-to-left matching.
pub fn match_selector(arena: &Arena, node: NodeId, selector: &Selector) -> bool {
    SelectorMatcher::new_with_dense(arena.len(), selector, false).matches(arena, node)
}

/// Match a selector list: a node matches if it matches ANY selector.
pub fn match_selector_list(arena: &Arena, node: NodeId, list: &SelectorList) -> bool {
    list.selectors.iter().any(|selector| {
        SelectorMatcher::new_with_dense(arena.len(), selector, false).matches(arena, node)
    })
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

/// Find the immediately following element sibling (skipping text/comments).
fn next_element_sibling(arena: &Arena, node: NodeId) -> Option<NodeId> {
    let mut next = arena.get(node).next_sibling;
    while !next.is_null() {
        let n = arena.get(next);
        if is_element(n) {
            return Some(next);
        }
        next = n.next_sibling;
    }
    None
}

// ---------------------------------------------------------------------------
// Selection: collect matching nodes
// ---------------------------------------------------------------------------

/// Select all nodes in `root`'s subtree matching a single selector.
pub fn select_all(arena: &Arena, root: NodeId, selector: &Selector) -> Vec<NodeId> {
    let mut results = Vec::new();
    SelectionCollector::new(arena.len(), std::slice::from_ref(selector)).collect(
        arena,
        root,
        &[],
        &mut results,
    );
    results
}

/// Select all nodes in `root`'s subtree matching a selector list.
pub fn select_all_list(arena: &Arena, root: NodeId, list: &SelectorList) -> Vec<NodeId> {
    let mut results = Vec::new();
    SelectionCollector::new(arena.len(), &list.selectors).collect(arena, root, &[], &mut results);
    results
}

/// Select all nodes matching a selector list within several subtree roots.
///
/// The result is the union of the per-root results of [`select_all_list`]
/// without duplicates: matches within one root are in document order, and
/// roots contribute in the order given, so document-ordered roots yield a
/// document-ordered result. A root inside the subtree of an earlier root adds
/// nothing, and a root's traversal skips the subtrees of earlier roots.
///
/// Matcher memo tables are shared by all roots, so the cost is linear in the
/// visited nodes instead of proportional to `roots.len() * arena.len()`.
pub fn select_all_list_roots(arena: &Arena, roots: &[NodeId], list: &SelectorList) -> Vec<NodeId> {
    let mut results = Vec::new();
    if roots.is_empty() {
        return results;
    }

    let mut collector = SelectionCollector::new(arena.len(), &list.selectors);
    let mut done = vec![false; arena.len()];

    for &root in roots {
        if root.is_null() || is_covered(arena, &done, root) {
            continue;
        }
        collector.collect(arena, root, &done, &mut results);
        done[root.index()] = true;
    }
    results
}

/// Whether `node` or one of its ancestors is an already collected root.
fn is_covered(arena: &Arena, done: &[bool], node: NodeId) -> bool {
    let mut current = node;
    while !current.is_null() {
        if done[current.index()] {
            return true;
        }
        current = arena.get(current).parent;
    }
    false
}

/// Whether `node`'s subtree was already collected. An empty `done` slice
/// means nothing was collected yet (single-root selection).
#[inline]
fn is_done(done: &[bool], node: NodeId) -> bool {
    done.get(node.index()).copied().unwrap_or(false)
}

/// Collection strategy for one selector list.
///
/// Memo entries depend only on the node, not on the traversal root, so one
/// collector serves every root of a multi-root selection.
enum SelectionCollector<'a> {
    /// A lone type selector: `div`.
    Tag(Tag),
    /// A lone id selector, `#id`, with its precomputed hash.
    Id(&'a str, u32),
    /// A single selector with a descendant combinator whose ancestor
    /// compounds name tags, classes, or ids to pre-filter on.
    Filtered {
        matcher: SelectorMatcher<'a>,
        required: AncestorFilter,
    },
    /// A single selector without descendant combinators.
    Simple(SelectorMatcher<'a>),
    /// Several selectors, matched in one traversal.
    List(Vec<SelectorMatcher<'a>>),
}

impl<'a> SelectionCollector<'a> {
    fn new(node_count: usize, selectors: &'a [Selector]) -> Self {
        let [selector] = selectors else {
            return Self::List(build_list_matchers(node_count, selectors));
        };
        if let Some(tag) = simple_tag_selector(selector) {
            return Self::Tag(tag);
        }
        if let Some((id, hash)) = simple_id_selector(selector) {
            return Self::Id(id, hash);
        }

        let matcher = SelectorMatcher::new(node_count, selector);
        let required = AncestorFilter::required_by(selector);
        if has_descendant_combinator(selector) && !required.is_empty() {
            Self::Filtered { matcher, required }
        } else {
            Self::Simple(matcher)
        }
    }

    /// Append the matches in `root`'s subtree to `results` in document order,
    /// skipping every subtree whose root is marked in `done`.
    fn collect(&mut self, arena: &Arena, root: NodeId, done: &[bool], results: &mut Vec<NodeId>) {
        match self {
            Self::Tag(tag) => collect_tag(arena, root, *tag, done, results),
            Self::Id(id, hash) => collect_id(arena, root, id, *hash, done, results),
            Self::Filtered { matcher, required } => {
                let ancestors = AncestorFilter::of_ancestors(arena, root);
                collect_filtered(arena, root, matcher, required, ancestors, done, results);
            }
            Self::Simple(matcher) => collect_simple(arena, root, matcher, done, results),
            Self::List(matchers) => collect_list(arena, root, matchers, done, results),
        }
    }
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
    if let Some((id, hash)) = simple_id_selector(selector) {
        return find_first_id(arena, root, id, hash);
    }
    let mut matcher = SelectorMatcher::new(arena.len(), selector);
    find_first(arena, root, &mut matcher)
}

/// Select the first matching node from a selector list.
pub fn select_first_list(arena: &Arena, root: NodeId, list: &SelectorList) -> Option<NodeId> {
    // DFS finds the first match in document order.
    if list.selectors.len() == 1 {
        return select_first(arena, root, &list.selectors[0]);
    }
    let mut matchers = build_list_matchers(arena.len(), &list.selectors);
    find_first_list(arena, root, &mut matchers)
}

// ---------------------------------------------------------------------------
// Internal collection helpers
// ---------------------------------------------------------------------------

/// DFS collection without bloom.
fn collect_simple(
    arena: &Arena,
    node: NodeId,
    matcher: &mut SelectorMatcher<'_>,
    done: &[bool],
    results: &mut Vec<NodeId>,
) {
    if node.is_null() || is_done(done, node) {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && matcher.matches(arena, node) {
        results.push(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_simple(arena, child, matcher, done, results);
        child = arena.get(child).next_sibling;
    }
}

/// Single DFS for a selector list. Traversing the actual links, rather than
/// sorting allocation ids, preserves document order for public arenas whose
/// nodes were allocated before they were attached.
fn collect_list(
    arena: &Arena,
    node: NodeId,
    matchers: &mut [SelectorMatcher<'_>],
    done: &[bool],
    results: &mut Vec<NodeId>,
) {
    if node.is_null() || is_done(done, node) {
        return;
    }

    let current = arena.get(node);
    if is_element(current)
        && matchers
            .iter_mut()
            .any(|matcher| matcher.matches(arena, node))
    {
        results.push(node);
    }

    let mut child = current.first_child;
    while !child.is_null() {
        collect_list(arena, child, matchers, done, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS collection optimized for a single `#id` selector.
fn collect_id(
    arena: &Arena,
    node: NodeId,
    target_id: &str,
    target_hash: u32,
    done: &[bool],
    results: &mut Vec<NodeId>,
) {
    if node.is_null() || is_done(done, node) {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && node_has_id(arena, node, target_id, target_hash) {
        results.push(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_id(arena, child, target_id, target_hash, done, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS collection optimized for a single tag selector.
fn collect_tag(
    arena: &Arena,
    node: NodeId,
    target_tag: Tag,
    done: &[bool],
    results: &mut Vec<NodeId>,
) {
    if node.is_null() || is_done(done, node) {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && n.tag == target_tag {
        results.push(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        collect_tag(arena, child, target_tag, done, results);
        child = arena.get(child).next_sibling;
    }
}

/// DFS collection that rejects candidates whose ancestors cannot satisfy
/// `required` before running the matcher. `ancestors` describes the
/// ancestors of `node`.
fn collect_filtered(
    arena: &Arena,
    node: NodeId,
    matcher: &mut SelectorMatcher<'_>,
    required: &AncestorFilter,
    ancestors: AncestorFilter,
    done: &[bool],
    results: &mut Vec<NodeId>,
) {
    if node.is_null() || is_done(done, node) {
        return;
    }

    let n = arena.get(node);
    if is_element(n) && ancestors.may_satisfy(required) && matcher.matches(arena, node) {
        results.push(node);
    }

    let child_ancestors = ancestors.with(n);
    let mut child = n.first_child;
    while !child.is_null() {
        collect_filtered(
            arena,
            child,
            matcher,
            required,
            child_ancestors,
            done,
            results,
        );
        child = arena.get(child).next_sibling;
    }
}

/// DFS to find first matching node.
fn find_first(arena: &Arena, node: NodeId, matcher: &mut SelectorMatcher<'_>) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n) && matcher.matches(arena, node) {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first(arena, child, matcher) {
            return Some(found);
        }
        child = arena.get(child).next_sibling;
    }
    None
}

/// DFS to find first match optimized for a single `#id` selector.
fn find_first_id(arena: &Arena, node: NodeId, target_id: &str, target_hash: u32) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n) && node_has_id(arena, node, target_id, target_hash) {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first_id(arena, child, target_id, target_hash) {
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
fn find_first_list(
    arena: &Arena,
    node: NodeId,
    matchers: &mut [SelectorMatcher<'_>],
) -> Option<NodeId> {
    if node.is_null() {
        return None;
    }

    let n = arena.get(node);
    if is_element(n)
        && matchers
            .iter_mut()
            .any(|matcher| matcher.matches(arena, node))
    {
        return Some(node);
    }

    let mut child = n.first_child;
    while !child.is_null() {
        if let Some(found) = find_first_list(arena, child, matchers) {
            return Some(found);
        }
        child = arena.get(child).next_sibling;
    }
    None
}

/// The id and its precomputed hash when `selector` is a lone `#id`.
#[inline]
fn simple_id_selector(selector: &Selector) -> Option<(&str, u32)> {
    if !selector.chain.is_empty() {
        return None;
    }
    match selector.subject.parts.as_slice() {
        [SimpleSelector::Id(id, hash)] => Some((id.as_str(), *hash)),
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
fn node_has_id(arena: &Arena, node: NodeId, target_id: &str, target_hash: u32) -> bool {
    // `id_hash == 0` is also the "no id" sentinel; when the target hashes to 0,
    // skip the fast path and verify directly (see SimpleSelector::Id).
    if id_hash_rejects(arena, node, target_hash) {
        return false;
    }
    arena.attrs(node).iter().any(|a| {
        arena.attr_name(a).eq_ignore_ascii_case("id") && arena.attr_value(a) == Some(target_id)
    })
}

/// Return whether the cached id hash conclusively rejects this node.
///
/// Both arena attribute writers drop duplicate attribute names, so a node has
/// at most one `id` attribute and its cached hash covers it. A target that
/// hashes to the "no id" sentinel 0 is never rejected here.
#[inline]
fn id_hash_rejects(arena: &Arena, node: NodeId, target_hash: u32) -> bool {
    debug_assert!(
        arena
            .attrs(node)
            .iter()
            .filter(|attr| arena.attr_name(attr).eq_ignore_ascii_case("id"))
            .nth(1)
            .is_none(),
        "arena attributes must not contain duplicate `id` names"
    );
    target_hash != 0 && arena.get(node).id_hash != target_hash
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
        assert!(contains_html_space_token("a b c", "b"));
        assert!(!contains_html_space_token("a bc d", "b"));
        assert!(!contains_html_space_token("abc", "b"));
    }

    #[test]
    fn class_token_ascii_whitespace() {
        assert!(contains_html_space_token("a\tb\nc", "b"));
        assert!(contains_html_space_token("  a   b  ", "a"));
        assert!(!contains_html_space_token("  a   b  ", "c"));
    }

    #[test]
    fn memo_uses_dense_and_sparse_representations_at_the_state_bound() {
        let dense = MatchMemo::new(32, 4);
        assert!(matches!(
            dense,
            MatchMemo::Dense {
                ref states,
                width: 4
            } if states.len() == 128
        ));

        let sparse = MatchMemo::new(DENSE_MEMO_STATE_LIMIT + 1, 1);
        assert!(matches!(sparse, MatchMemo::Sparse(ref states) if states.is_empty()));
    }

    #[test]
    fn selector_lists_share_the_dense_state_budget() {
        let list = parse_selector("div span, p a").unwrap();
        let matchers = build_list_matchers(DENSE_MEMO_STATE_LIMIT / 2 + 1, &list.selectors);
        assert_eq!(matchers.len(), 2);
        assert!(
            matchers
                .iter()
                .all(|matcher| matches!(&matcher.memo, MatchMemo::Sparse(_)))
        );
    }

    #[test]
    fn repeated_candidates_reuse_memoized_chain_states() {
        let doc = fhp_tree::parse("<main><div><div><div><span>x</span></div></div></div></main>")
            .unwrap();
        let selector = parse_selector("section > div div div span")
            .unwrap()
            .selectors
            .pop()
            .unwrap();
        let mut matcher = SelectorMatcher::new(doc.arena().len(), &selector);

        for index in 0..doc.arena().len() {
            let node = NodeId(index as u32);
            let _ = matcher.matches(doc.arena(), node);
        }
        let first_pass_states = matcher.memo.populated_len();
        assert!(first_pass_states > 0);
        assert!(first_pass_states <= doc.arena().len() * selector.chain.len());

        for index in 0..doc.arena().len() {
            let node = NodeId(index as u32);
            let _ = matcher.matches(doc.arena(), node);
        }
        assert_eq!(matcher.memo.populated_len(), first_pass_states);
    }

    fn texts(doc: &fhp_tree::Document, ids: &[NodeId]) -> Vec<String> {
        ids.iter().map(|&id| doc.get(id).text_content()).collect()
    }

    #[test]
    fn general_sibling_chains_match_in_any_visit_order() {
        let doc = fhp_tree::parse(
            "<div><p class=x>0</p><h1>a</h1><p>1</p><h1>b</h1>t<!--c--><p>2</p></div>\
             <div><h1>c</h1><p>3</p></div>\
             <div><p>4</p><h1>d</h1><p>5</p></div>",
        )
        .unwrap();
        let arena = doc.arena();

        for (css, expected) in [
            ("h1 ~ p", vec!["1", "2", "3", "5"]),
            (".x ~ h1 ~ p", vec!["1", "2"]),
            ("h1 ~ h1 ~ p", vec!["2"]),
            ("p ~ p", vec!["1", "2", "5"]),
        ] {
            let list = parse_selector(css).unwrap();
            let selected = select_all_list(arena, doc.root_id(), &list);
            assert_eq!(texts(&doc, &selected), expected, "{css}");

            // A fresh matcher per node (arena order is document order here).
            let single: Vec<_> = (0..arena.len() as u32)
                .map(NodeId)
                .filter(|&id| match_selector_list(arena, id, &list))
                .collect();
            assert_eq!(single, selected, "{css}");

            // Reverse document order on one shared matcher.
            let mut matcher = SelectorMatcher::new(arena.len(), &list.selectors[0]);
            let mut reversed: Vec<_> = (0..arena.len() as u32)
                .rev()
                .map(NodeId)
                .filter(|&id| matcher.matches(arena, id))
                .collect();
            reversed.reverse();
            assert_eq!(texts(&doc, &reversed), expected, "{css}");
        }
    }

    #[test]
    fn multi_root_selection_is_the_ordered_union_of_root_results() {
        let doc = fhp_tree::parse(
            "<div id=a class=k><p>1</p><div id=b><p id=x>2</p><span>s</span></div></div>\
             <div id=c><p>3</p></div>",
        )
        .unwrap();
        let arena = doc.arena();
        let by_id = |id: &str| {
            select_first_list(
                arena,
                doc.root_id(),
                &parse_selector(&format!("#{id}")).unwrap(),
            )
            .unwrap()
        };
        let (a, b, c) = (by_id("a"), by_id("b"), by_id("c"));

        for css in ["p", "#x", "div p", ".k p", "div > p", "p, span", "div"] {
            let list = parse_selector(css).unwrap();
            for roots in [
                vec![a, b, c],
                vec![b, a, c],
                vec![c, b, a],
                vec![c, c, b],
                vec![],
            ] {
                let mut expected = Vec::new();
                for &root in &roots {
                    for id in select_all_list(arena, root, &list) {
                        if !expected.contains(&id) {
                            expected.push(id);
                        }
                    }
                }
                assert_eq!(
                    select_all_list_roots(arena, &roots, &list),
                    expected,
                    "{css} over {roots:?}"
                );
            }
        }
    }
}
