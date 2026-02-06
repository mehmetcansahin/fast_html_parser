//! Allocation-free tree traversal iterators.
//!
//! All iterators borrow the arena and produce [`NodeId`] values.
//! [`DepthFirst`] and [`Children`] are fully allocation-free.
//! [`BreadthFirst`] uses a [`VecDeque`] internally.

use std::collections::VecDeque;

use crate::arena::Arena;
use crate::node::NodeId;

/// Pre-order depth-first traversal, allocation-free.
///
/// Walks the subtree rooted at `root` using tree links (no separate stack).
pub struct DepthFirst<'a> {
    arena: &'a Arena,
    current: NodeId,
    root: NodeId,
}

impl<'a> DepthFirst<'a> {
    /// Create a new depth-first iterator starting at `root`.
    pub fn new(arena: &'a Arena, root: NodeId) -> Self {
        Self {
            arena,
            current: root,
            root,
        }
    }

    /// Find the next node after `node` that is not a descendant.
    fn next_non_child(&self, node: NodeId) -> NodeId {
        let mut cur = node;
        loop {
            let n = self.arena.get(cur);
            if !n.next_sibling.is_null() {
                return n.next_sibling;
            }
            if n.parent.is_null() || n.parent == self.root {
                return NodeId::NULL;
            }
            cur = n.parent;
        }
    }
}

impl<'a> Iterator for DepthFirst<'a> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.current.is_null() {
            return None;
        }

        let result = self.current;
        let node = self.arena.get(result);

        // Try first child, then next sibling, then ancestor's sibling.
        self.current = if !node.first_child.is_null() {
            node.first_child
        } else {
            self.next_non_child(result)
        };

        Some(result)
    }
}

/// Breadth-first (level-order) traversal.
///
/// Uses a [`VecDeque`] internally — allocation is unavoidable for BFS.
pub struct BreadthFirst<'a> {
    arena: &'a Arena,
    queue: VecDeque<NodeId>,
}

impl<'a> BreadthFirst<'a> {
    /// Create a new breadth-first iterator starting at `root`.
    pub fn new(arena: &'a Arena, root: NodeId) -> Self {
        let mut queue = VecDeque::new();
        if !root.is_null() {
            queue.push_back(root);
        }
        Self { arena, queue }
    }
}

impl<'a> Iterator for BreadthFirst<'a> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.queue.pop_front()?;
        let node = self.arena.get(id);

        // Enqueue all children.
        let mut child = node.first_child;
        while !child.is_null() {
            self.queue.push_back(child);
            child = self.arena.get(child).next_sibling;
        }

        Some(id)
    }
}

/// Iterate over the direct children of a node, allocation-free.
pub struct Children<'a> {
    arena: &'a Arena,
    current: NodeId,
}

impl<'a> Children<'a> {
    /// Create an iterator over children of `parent`.
    pub fn new(arena: &'a Arena, parent: NodeId) -> Self {
        let first = arena.get(parent).first_child;
        Self {
            arena,
            current: first,
        }
    }
}

impl<'a> Iterator for Children<'a> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.current.is_null() {
            return None;
        }
        let result = self.current;
        self.current = self.arena.get(result).next_sibling;
        Some(result)
    }
}

/// Iterate over ancestors of a node (parent chain), allocation-free.
///
/// Does **not** include the starting node itself.
pub struct Ancestors<'a> {
    arena: &'a Arena,
    current: NodeId,
}

impl<'a> Ancestors<'a> {
    /// Create an iterator over ancestors of `node`.
    pub fn new(arena: &'a Arena, node: NodeId) -> Self {
        let parent = arena.get(node).parent;
        Self {
            arena,
            current: parent,
        }
    }
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.current.is_null() {
            return None;
        }
        let result = self.current;
        self.current = self.arena.get(result).parent;
        Some(result)
    }
}

/// Iterate over next siblings of a node, allocation-free.
///
/// Does **not** include the starting node itself.
pub struct Siblings<'a> {
    arena: &'a Arena,
    current: NodeId,
}

impl<'a> Siblings<'a> {
    /// Create an iterator over next siblings of `node`.
    pub fn new(arena: &'a Arena, node: NodeId) -> Self {
        let next = arena.get(node).next_sibling;
        Self {
            arena,
            current: next,
        }
    }
}

impl<'a> Iterator for Siblings<'a> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.current.is_null() {
            return None;
        }
        let result = self.current;
        self.current = self.arena.get(result).next_sibling;
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hp_core::tag::Tag;

    fn build_test_tree() -> (Arena, NodeId) {
        // <div>
        //   <span>hello</span>
        //   <p>world</p>
        // </div>
        let mut arena = Arena::new();
        let root = arena.new_element(Tag::Unknown, 0);
        let div = arena.new_element(Tag::Div, 1);
        arena.append_child(root, div);

        let span = arena.new_element(Tag::Span, 2);
        arena.append_child(div, span);
        let hello = arena.new_text(3, "hello");
        arena.append_child(span, hello);

        let p = arena.new_element(Tag::P, 2);
        arena.append_child(div, p);
        let world = arena.new_text(3, "world");
        arena.append_child(p, world);

        (arena, root)
    }

    #[test]
    fn depth_first_order() {
        let (arena, root) = build_test_tree();
        let ids: Vec<NodeId> = DepthFirst::new(&arena, root).collect();

        // root, div, span, hello, p, world
        assert_eq!(ids.len(), 6);
        assert_eq!(arena.get(ids[0]).tag, Tag::Unknown); // root
        assert_eq!(arena.get(ids[1]).tag, Tag::Div);
        assert_eq!(arena.get(ids[2]).tag, Tag::Span);
        assert_eq!(arena.text(ids[3]), "hello");
        assert_eq!(arena.get(ids[4]).tag, Tag::P);
        assert_eq!(arena.text(ids[5]), "world");
    }

    #[test]
    fn breadth_first_order() {
        let (arena, root) = build_test_tree();
        let ids: Vec<NodeId> = BreadthFirst::new(&arena, root).collect();

        // root, div, span, p, hello, world
        assert_eq!(ids.len(), 6);
        assert_eq!(arena.get(ids[0]).tag, Tag::Unknown); // root
        assert_eq!(arena.get(ids[1]).tag, Tag::Div);
        assert_eq!(arena.get(ids[2]).tag, Tag::Span);
        assert_eq!(arena.get(ids[3]).tag, Tag::P);
        assert_eq!(arena.text(ids[4]), "hello");
        assert_eq!(arena.text(ids[5]), "world");
    }

    #[test]
    fn children_iterator() {
        let (arena, root) = build_test_tree();
        let div = arena.get(root).first_child;
        let children: Vec<NodeId> = Children::new(&arena, div).collect();

        assert_eq!(children.len(), 2);
        assert_eq!(arena.get(children[0]).tag, Tag::Span);
        assert_eq!(arena.get(children[1]).tag, Tag::P);
    }

    #[test]
    fn ancestors_iterator() {
        let (arena, root) = build_test_tree();
        // hello text node is: root -> div -> span -> hello
        let div = arena.get(root).first_child;
        let span = arena.get(div).first_child;
        let hello = arena.get(span).first_child;

        let ancestors: Vec<NodeId> = Ancestors::new(&arena, hello).collect();
        assert_eq!(ancestors.len(), 3); // span, div, root
        assert_eq!(arena.get(ancestors[0]).tag, Tag::Span);
        assert_eq!(arena.get(ancestors[1]).tag, Tag::Div);
        assert_eq!(arena.get(ancestors[2]).tag, Tag::Unknown); // root
    }

    #[test]
    fn siblings_iterator() {
        let (arena, root) = build_test_tree();
        let div = arena.get(root).first_child;
        let span = arena.get(div).first_child;

        let siblings: Vec<NodeId> = Siblings::new(&arena, span).collect();
        assert_eq!(siblings.len(), 1);
        assert_eq!(arena.get(siblings[0]).tag, Tag::P);
    }

    #[test]
    fn empty_iterators() {
        let (arena, root) = build_test_tree();
        let div = arena.get(root).first_child;
        let span = arena.get(div).first_child;
        let hello = arena.get(span).first_child;

        // hello has no children.
        assert_eq!(Children::new(&arena, hello).count(), 0);
        // hello has no siblings (it's the only child of span).
        assert_eq!(Siblings::new(&arena, hello).count(), 0);
    }
}
