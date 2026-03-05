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
    /// Stack of open element node ids with cached tags.
    open_elements: Vec<(NodeId, Tag)>,
    /// The synthetic root node (document root).
    root: NodeId,
    /// Base address of the original input (for source-backed text).
    source_base: usize,
    /// Length of the original input in bytes.
    source_len: usize,
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
        // Source-backed text uses offsets, not slab — smaller alloc suffices.
        let text_cap = (input_len / 16).max(4096);
        let attr_cap = (input_len / 128).max(64);
        let mut arena = Arena::with_capacity(node_cap, text_cap, attr_cap);
        // Create a synthetic document root.
        let root = arena.new_element(Tag::Unknown, 0);
        let mut open_elements = Vec::with_capacity(32);
        open_elements.push((root, Tag::Unknown));
        Self {
            arena,
            open_elements,
            root,
            source_base: 0,
            source_len: 0,
        }
    }

    /// Enable the inline tag index for O(1) tag lookups after parsing.
    ///
    /// When enabled, each element's tag is indexed during tree construction,
    /// eliminating the need for a separate DFS pass in
    /// [`DocumentIndex::build`](crate::arena::Arena::tag_index).
    pub fn enable_tag_index(&mut self) {
        self.arena.enable_tag_index();
    }

    /// Enable source-backed text nodes.
    ///
    /// Stores an owned copy of `input` in the arena. Text nodes whose content
    /// is borrowed from `input` (entity-free) will reference the source
    /// instead of copying to the text slab.
    pub fn set_source(&mut self, input: &str) {
        self.source_base = input.as_ptr() as usize;
        self.source_len = input.len();
        self.arena.set_source(input);
    }

    /// Set source pointer tracking without copying data to the arena.
    ///
    /// Use this with [`Arena::set_source_owned`] after tokenization to
    /// avoid a redundant memcpy when the caller owns the input `String`.
    pub fn set_source_ptr(&mut self, input: &str) {
        self.source_base = input.as_ptr() as usize;
        self.source_len = input.len();
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
        self.open_elements
            .last()
            .map(|&(id, _)| id)
            .unwrap_or(self.root)
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
            self.open_elements.push((node, tag));
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
        // Walk backwards to find the nearest match — read tag from cached tuple.
        let mut match_idx = None;
        for i in (1..self.open_elements.len()).rev() {
            let (open_id, open_tag) = self.open_elements[i];
            if open_tag == tag {
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

    /// Handle text content (already entity-decoded by the tokenizer).
    ///
    /// If the text is borrowed from the original source (pointer falls within
    /// the source range), creates a source-backed text node to avoid copying.
    fn handle_text(&mut self, content: &str) -> Option<NodeId> {
        if content.is_empty() {
            return None;
        }
        let depth = self.current_depth();
        let parent = self.current_parent();
        let node = self.try_source_ref(depth, content);
        self.arena.append_child(parent, node);
        Some(node)
    }

    /// Handle raw text from TreeSink (not entity-decoded).
    ///
    /// Performs entity decoding if the `entity-decode` feature is enabled,
    /// then creates a source-backed or slab text node.
    fn handle_raw_text(&mut self, raw: &str) -> Option<NodeId> {
        if raw.is_empty() {
            return None;
        }
        let depth = self.current_depth();
        let parent = self.current_parent();

        #[cfg(feature = "entity-decode")]
        let node = {
            let decoded = fhp_tokenizer::entity::decode_entities(raw);
            match decoded {
                std::borrow::Cow::Borrowed(s) => self.try_source_ref(depth, s),
                std::borrow::Cow::Owned(s) => self.arena.new_text(depth, &s),
            }
        };

        #[cfg(not(feature = "entity-decode"))]
        let node = self.try_source_ref(depth, raw);

        self.arena.append_child(parent, node);
        Some(node)
    }

    /// Create a text node, using a source-backed ref if the pointer is within
    /// the original source range.
    #[inline]
    fn try_source_ref(&mut self, depth: u16, content: &str) -> NodeId {
        if self.source_len > 0 {
            let ptr = content.as_ptr() as usize;
            if ptr >= self.source_base && ptr + content.len() <= self.source_base + self.source_len
            {
                let offset = ptr - self.source_base;
                return self
                    .arena
                    .new_text_ref(depth, offset as u32, content.len() as u32);
            }
        }
        self.arena.new_text(depth, content)
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
        // Tag is read from the cached tuple — no arena access needed.
        while self.open_elements.len() > 1 {
            let (_, current_tag) = *self.open_elements.last().unwrap();

            if should_implicit_close(current_tag, new_tag) {
                self.open_elements.pop();
            } else {
                break;
            }
        }
    }
}

impl fhp_tokenizer::TreeSink for TreeBuilder {
    fn open_tag(&mut self, tag: Tag, name: &str, attr_raw: &str, self_closing: bool) {
        self.apply_implicit_close(tag);

        if self.current_depth() >= MAX_DEPTH {
            return;
        }

        let depth = self.current_depth();
        let parent = self.current_parent();
        let node = self.arena.new_element(tag, depth);
        if tag == Tag::Unknown {
            self.arena.set_unknown_tag_name(node, name);
        }

        // Parse attributes directly into the slab (no intermediate Vec).
        self.arena.set_attrs_from_raw(node, attr_raw);

        self.arena.append_child(parent, node);

        if tag.is_void() || self_closing {
            if self_closing {
                self.arena.set_self_closing(node);
            }
            if tag.is_void() {
                self.arena.set_self_closing(node);
            }
        } else {
            self.open_elements.push((node, tag));
        }
    }

    fn close_tag(&mut self, tag: Tag, name: &str) {
        self.handle_close_tag(tag, name);
    }

    fn text(&mut self, raw: &str) {
        self.handle_raw_text(raw);
    }

    fn comment(&mut self, content: &str) {
        self.handle_comment(content);
    }

    fn doctype(&mut self, content: &str) {
        self.handle_doctype(content);
    }

    fn cdata(&mut self, content: &str) {
        // CDATA treated as text.
        self.handle_raw_text(content);
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
