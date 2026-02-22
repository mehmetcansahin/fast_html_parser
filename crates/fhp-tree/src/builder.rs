//! Tree construction from a token stream.
//!
//! [`TreeBuilder`](crate::builder::TreeBuilder) consumes [`Token`](fhp_tokenizer::token::Token)s and builds an arena-based DOM tree.
//! It handles implicit close rules, void elements, and common malformed-HTML
//! recovery strategies.

use fhp_core::tag::Tag;
use fhp_tokenizer::token::Token;

use crate::arena::Arena;
use crate::node::NodeId;

/// Maximum nesting depth to prevent stack overflow on pathological input.
const MAX_DEPTH: u16 = 512;

/// Implicit close lookup table.
///
/// `IMPLICIT_CLOSE[open_tag][new_tag]` is `true` when the arrival of
/// `new_tag` should implicitly close the currently-open `open_tag`.
///
/// Only a subset of tags participate in implicit closing. Tags are mapped
/// to compact indices 0..=7 via [`implicit_close_index`].
const IMPLICIT_CLOSE_SIZE: usize = 8;
static IMPLICIT_CLOSE: [[bool; IMPLICIT_CLOSE_SIZE]; IMPLICIT_CLOSE_SIZE] = {
    let mut table = [[false; IMPLICIT_CLOSE_SIZE]; IMPLICIT_CLOSE_SIZE];

    // <p> is closed by another block-level element.
    let p = 0usize;
    let li = 1;
    let td = 2;
    let th = 3;
    let tr = 4;
    let thead = 5;
    let tbody = 6;
    let option = 7;

    // <p> closed by <p>, <div>, <h1>-<h6>, <ul>, <ol>, <table>, <pre>, etc.
    // Simplified: <p> closed by <p>
    table[p][p] = true;

    // <li> closed by <li>
    table[li][li] = true;

    // <td> closed by <td> or <th>
    table[td][td] = true;
    table[td][th] = true;

    // <th> closed by <td> or <th>
    table[th][td] = true;
    table[th][th] = true;

    // <tr> closed by <tr>
    table[tr][tr] = true;

    // <thead> closed by <tbody>
    table[thead][tbody] = true;

    // <tbody> closed by <tbody>
    table[tbody][tbody] = true;

    // <option> closed by <option>
    table[option][option] = true;

    table
};

/// Map a tag to its implicit close table index, or `None` if it doesn't
/// participate in implicit closing.
fn implicit_close_index(tag: Tag) -> Option<usize> {
    match tag {
        Tag::P => Some(0),
        Tag::Li => Some(1),
        Tag::Td => Some(2),
        Tag::Th => Some(3),
        Tag::Tr => Some(4),
        Tag::Thead => Some(5),
        Tag::Tbody => Some(6),
        // Option tag not in Tag enum — skip
        _ => None,
    }
}

/// Check whether `new_tag` should implicitly close `open_tag`.
#[inline]
fn should_implicit_close(open_tag: Tag, new_tag: Tag) -> bool {
    if let (Some(open_idx), Some(new_idx)) = (
        implicit_close_index(open_tag),
        implicit_close_index(new_tag),
    ) {
        IMPLICIT_CLOSE[open_idx][new_idx]
    } else {
        false
    }
}

/// Builds a DOM tree from a token stream.
///
/// Maintains an open-elements stack and processes each token to either
/// push new nodes, pop closed elements, or append text/comment/doctype nodes.
pub struct TreeBuilder {
    /// The arena that owns all nodes.
    pub(crate) arena: Arena,
    /// Stack of open element node ids.
    open_elements: Vec<NodeId>,
    /// The synthetic root node (document root).
    root: NodeId,
}

impl TreeBuilder {
    /// Create a new tree builder with default capacity.
    pub fn new() -> Self {
        Self::with_capacity_hint(0)
    }

    /// Create a tree builder with capacity tuned to the expected input size.
    ///
    /// Uses heuristics to estimate node, text, and attribute counts from the
    /// input byte length, reducing reallocations for large documents.
    pub fn with_capacity_hint(input_len: usize) -> Self {
        let node_cap = (input_len / 32).max(256);
        let text_cap = (input_len / 4).max(4096);
        let attr_cap = (input_len / 128).max(64);
        let mut arena = Arena::with_capacity(node_cap, text_cap, attr_cap);
        // Create a synthetic document root.
        let root = arena.new_element(Tag::Unknown, 0);
        Self {
            arena,
            open_elements: vec![root],
            root,
        }
    }

    /// Process a single token and insert it into the tree.
    ///
    /// Returns the [`NodeId`] of the newly created node, or `None` if the
    /// token did not produce a node (e.g. a close tag or depth limit hit).
    pub fn process(&mut self, token: &Token<'_>) -> Option<NodeId> {
        match token {
            Token::OpenTag {
                tag,
                name,
                attributes,
                self_closing,
                ..
            } => self.handle_open_tag(*tag, name.as_ref(), attributes, *self_closing),
            Token::CloseTag { tag, name } => {
                self.handle_close_tag(*tag, name.as_ref());
                None
            }
            Token::Text { content } => self.handle_text(content.as_ref()),
            Token::Comment { content } => self.handle_comment(content.as_ref()),
            Token::Doctype { content } => self.handle_doctype(content.as_ref()),
            Token::CData { content } => {
                // Treat CDATA as text.
                self.handle_text(content.as_ref())
            }
        }
    }

    /// Finish building and return the root node id and the arena.
    pub fn finish(self) -> (Arena, NodeId) {
        // Any unclosed elements are implicitly closed (they stay in the tree).
        (self.arena, self.root)
    }

    /// Current parent node (top of open_elements stack).
    #[inline]
    fn current_parent(&self) -> NodeId {
        *self.open_elements.last().unwrap_or(&self.root)
    }

    /// Current depth.
    #[inline]
    fn current_depth(&self) -> u16 {
        (self.open_elements.len() as u16).min(MAX_DEPTH)
    }

    /// Handle an open tag token.
    fn handle_open_tag(
        &mut self,
        tag: Tag,
        name: &str,
        attributes: &[fhp_tokenizer::token::Attribute<'_>],
        self_closing: bool,
    ) -> Option<NodeId> {
        // Apply implicit close rules.
        self.apply_implicit_close(tag);

        // Enforce depth limit.
        if self.current_depth() >= MAX_DEPTH {
            return None;
        }

        let depth = self.current_depth();
        let parent = self.current_parent();
        let node = self.arena.new_element(tag, depth);
        if tag == Tag::Unknown {
            self.arena.set_unknown_tag_name(node, name);
        }

        // Set attributes.
        if !attributes.is_empty() {
            self.arena.set_attrs(node, attributes);
        }

        // Append to parent.
        self.arena.append_child(parent, node);

        // Void elements and self-closing tags don't go on the stack.
        if tag.is_void() || self_closing {
            if self_closing {
                self.arena.set_self_closing(node);
            }
            // Void elements are always marked self-closing in the tree.
            if tag.is_void() {
                self.arena.set_self_closing(node);
            }
        } else {
            self.open_elements.push(node);
        }

        Some(node)
    }

    /// Handle a close tag token.
    fn handle_close_tag(&mut self, tag: Tag, name: &str) {
        // Ignore close tags for void elements.
        if tag.is_void() {
            return;
        }

        // Find the matching open element on the stack.
        // Walk backwards to find the nearest match.
        let mut match_idx = None;
        for i in (1..self.open_elements.len()).rev() {
            let open_id = self.open_elements[i];
            let open_node = &self.arena.nodes[open_id.index()];
            if open_node.tag == tag {
                if tag == Tag::Unknown {
                    let open_name = self.arena.unknown_tag_name(open_id).unwrap_or("");
                    if !open_name.eq_ignore_ascii_case(name) {
                        continue;
                    }
                }
                match_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = match_idx {
            // Pop everything from idx onwards (implicitly closing).
            self.open_elements.truncate(idx);
        }
        // If no match found, ignore the close tag (broken HTML recovery).
    }

    /// Handle text content.
    fn handle_text(&mut self, content: &str) -> Option<NodeId> {
        if content.is_empty() {
            return None;
        }
        let depth = self.current_depth();
        let parent = self.current_parent();
        let node = self.arena.new_text(depth, content);
        self.arena.append_child(parent, node);
        Some(node)
    }

    /// Handle a comment.
    fn handle_comment(&mut self, content: &str) -> Option<NodeId> {
        let depth = self.current_depth();
        let parent = self.current_parent();
        let node = self.arena.new_comment(depth, content);
        self.arena.append_child(parent, node);
        Some(node)
    }

    /// Handle a doctype declaration.
    fn handle_doctype(&mut self, content: &str) -> Option<NodeId> {
        let depth = self.current_depth();
        let parent = self.current_parent();
        let node = self.arena.new_doctype(depth, content);
        self.arena.append_child(parent, node);
        Some(node)
    }

    /// Apply implicit close rules based on the new tag.
    fn apply_implicit_close(&mut self, new_tag: Tag) {
        // Check if the current open element should be implicitly closed.
        while self.open_elements.len() > 1 {
            let current = *self.open_elements.last().unwrap();
            let current_tag = self.arena.nodes[current.index()].tag;

            if should_implicit_close(current_tag, new_tag) {
                self.open_elements.pop();
            } else {
                break;
            }
        }
    }
}

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeFlags;
    use std::borrow::Cow;

    fn make_open(tag: Tag) -> Token<'static> {
        Token::OpenTag {
            tag,
            name: Cow::Borrowed(tag.as_str().unwrap_or("unknown")),
            attributes: vec![],
            self_closing: false,
        }
    }

    fn make_close(tag: Tag) -> Token<'static> {
        Token::CloseTag {
            tag,
            name: Cow::Borrowed(tag.as_str().unwrap_or("unknown")),
        }
    }

    fn make_text(content: &'static str) -> Token<'static> {
        Token::Text {
            content: Cow::Borrowed(content),
        }
    }

    #[test]
    fn simple_tree() {
        let mut builder = TreeBuilder::new();
        builder.process(&make_open(Tag::Div));
        builder.process(&make_text("hello"));
        builder.process(&make_close(Tag::Div));

        let (arena, root) = builder.finish();

        // Root should have one child (div).
        let div = arena.get(root).first_child;
        assert!(!div.is_null());
        assert_eq!(arena.get(div).tag, Tag::Div);

        // Div should have one child (text).
        let text = arena.get(div).first_child;
        assert!(!text.is_null());
        assert!(arena.get(text).flags.has(NodeFlags::IS_TEXT));
        assert_eq!(arena.text(text), "hello");
    }

    #[test]
    fn void_element_not_pushed() {
        let mut builder = TreeBuilder::new();
        builder.process(&make_open(Tag::Div));
        builder.process(&Token::OpenTag {
            tag: Tag::Br,
            name: Cow::Borrowed("br"),
            attributes: vec![],
            self_closing: false,
        });
        builder.process(&make_text("after br"));
        builder.process(&make_close(Tag::Div));

        let (arena, root) = builder.finish();
        let div = arena.get(root).first_child;
        let br = arena.get(div).first_child;
        assert_eq!(arena.get(br).tag, Tag::Br);

        // Text should be a sibling of br, not a child.
        let text = arena.get(br).next_sibling;
        assert!(!text.is_null());
        assert_eq!(arena.text(text), "after br");
    }

    #[test]
    fn implicit_close_p() {
        let mut builder = TreeBuilder::new();
        builder.process(&make_open(Tag::P));
        builder.process(&make_text("first"));
        builder.process(&make_open(Tag::P));
        builder.process(&make_text("second"));
        builder.process(&make_close(Tag::P));

        let (arena, root) = builder.finish();

        // Both <p> elements should be children of root.
        let p1 = arena.get(root).first_child;
        assert_eq!(arena.get(p1).tag, Tag::P);

        let p2 = arena.get(p1).next_sibling;
        assert!(!p2.is_null());
        assert_eq!(arena.get(p2).tag, Tag::P);

        // p1 has "first", p2 has "second".
        assert_eq!(arena.text(arena.get(p1).first_child), "first");
        assert_eq!(arena.text(arena.get(p2).first_child), "second");
    }

    #[test]
    fn mismatched_close_finds_nearest() {
        // <div><span></div> — should close both span and div.
        let mut builder = TreeBuilder::new();
        builder.process(&make_open(Tag::Div));
        builder.process(&make_open(Tag::Span));
        builder.process(&make_text("hi"));
        builder.process(&make_close(Tag::Div));

        let (arena, root) = builder.finish();
        let div = arena.get(root).first_child;
        assert_eq!(arena.get(div).tag, Tag::Div);
    }

    #[test]
    fn extra_close_tag_ignored() {
        let mut builder = TreeBuilder::new();
        builder.process(&make_close(Tag::Div)); // No matching open — ignored.
        builder.process(&make_open(Tag::P));
        builder.process(&make_text("ok"));
        builder.process(&make_close(Tag::P));

        let (arena, root) = builder.finish();
        let p = arena.get(root).first_child;
        assert_eq!(arena.get(p).tag, Tag::P);
    }

    #[test]
    fn unknown_close_matches_by_name() {
        let mut builder = TreeBuilder::new();
        builder.process(&Token::OpenTag {
            tag: Tag::Unknown,
            name: Cow::Borrowed("my-widget"),
            attributes: vec![],
            self_closing: false,
        });
        builder.process(&Token::OpenTag {
            tag: Tag::Unknown,
            name: Cow::Borrowed("x-item"),
            attributes: vec![],
            self_closing: false,
        });
        builder.process(&Token::CloseTag {
            tag: Tag::Unknown,
            name: Cow::Borrowed("my-widget"),
        });

        let (arena, root) = builder.finish();
        let my_widget = arena.get(root).first_child;
        let x_item = arena.get(my_widget).first_child;
        assert_eq!(arena.unknown_tag_name(my_widget), Some("my-widget"));
        assert_eq!(arena.unknown_tag_name(x_item), Some("x-item"));
    }

    #[test]
    fn should_implicit_close_rules() {
        assert!(should_implicit_close(Tag::P, Tag::P));
        assert!(should_implicit_close(Tag::Li, Tag::Li));
        assert!(should_implicit_close(Tag::Td, Tag::Td));
        assert!(should_implicit_close(Tag::Td, Tag::Th));
        assert!(should_implicit_close(Tag::Th, Tag::Td));
        assert!(should_implicit_close(Tag::Tr, Tag::Tr));

        assert!(!should_implicit_close(Tag::Div, Tag::Div));
        assert!(!should_implicit_close(Tag::Span, Tag::Span));
        assert!(!should_implicit_close(Tag::P, Tag::Span));
    }
}
