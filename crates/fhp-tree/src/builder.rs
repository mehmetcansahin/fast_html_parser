//! Tree construction from a token stream.
//!
//! [`TreeBuilder`](crate::builder::TreeBuilder) consumes [`Token`](fhp_tokenizer::token::Token)s and builds an arena-based DOM tree.
//! It handles implicit close rules, void elements, and common malformed-HTML
//! recovery strategies.

use fhp_core::{error::ParseError, tag::Tag};
use fhp_tokenizer::token::{Attribute as TokenAttribute, Token};

use crate::arena::Arena;
use crate::node::NodeId;

/// Maximum nesting depth to prevent stack overflow on pathological input.
const MAX_DEPTH: u16 = 512;

/// Cap capacity heuristics for very large inputs.
///
/// The vectors still grow normally when a document is genuinely node-dense,
/// but a large plain-text document no longer reserves hundreds of megabytes
/// for nodes and attributes that it will never create.
const MAX_CAPACITY_HINT_INPUT: usize = 8 * 1024 * 1024;

/// Private element categories used by the pragmatic insertion-mode machine.
/// Public tag interning remains compact and unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum ElementKind {
    Root,
    Other,
    Paragraph,
    ListItem,
    DefinitionItem,
    Heading,
    Table,
    TableBody,
    Row,
    Cell,
    Select,
    Option,
    OptGroup,
    Plaintext,
    Formatting,
}

impl ElementKind {
    const COUNT: usize = Self::Formatting as usize + 1;

    #[inline]
    const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    fn classify(tag: Tag, name: &str) -> Self {
        match tag {
            Tag::P => Self::Paragraph,
            Tag::Li => Self::ListItem,
            Tag::H1 | Tag::H2 | Tag::H3 | Tag::H4 | Tag::H5 | Tag::H6 => Self::Heading,
            Tag::Table => Self::Table,
            Tag::Tbody | Tag::Thead => Self::TableBody,
            Tag::Tr => Self::Row,
            Tag::Td | Tag::Th => Self::Cell,
            Tag::Select => Self::Select,
            Tag::A | Tag::B | Tag::Em | Tag::I | Tag::Strong => Self::Formatting,
            Tag::Unknown if name.eq_ignore_ascii_case("dt") || name.eq_ignore_ascii_case("dd") => {
                Self::DefinitionItem
            }
            Tag::Unknown if name.eq_ignore_ascii_case("tfoot") => Self::TableBody,
            Tag::Unknown if name.eq_ignore_ascii_case("option") => Self::Option,
            Tag::Unknown if name.eq_ignore_ascii_case("optgroup") => Self::OptGroup,
            Tag::Unknown if name.eq_ignore_ascii_case("plaintext") => Self::Plaintext,
            Tag::Unknown
                if matches_ascii_name(
                    name,
                    &[
                        "big", "code", "font", "nobr", "s", "small", "strike", "tt", "u",
                    ],
                ) =>
            {
                Self::Formatting
            }
            _ => Self::Other,
        }
    }

    #[inline]
    const fn pushed_mode(self, current: InsertionMode) -> InsertionMode {
        match self {
            Self::Table => InsertionMode::InTable,
            Self::TableBody => InsertionMode::InTableBody,
            Self::Row => InsertionMode::InRow,
            Self::Cell => InsertionMode::InCell,
            Self::Select => InsertionMode::InSelect,
            _ => current,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Names mirror HTML insertion-mode terminology.
enum InsertionMode {
    InBody,
    InTable,
    InTableBody,
    InRow,
    InCell,
    InSelect,
}

#[derive(Clone, Copy, Debug)]
struct OpenElement {
    id: NodeId,
    tag: Tag,
    kind: ElementKind,
    /// Effective insertion mode for the stack prefix ending at this entry.
    insertion_mode: InsertionMode,
    element_child_count: u32,
}

enum PendingAttrs<'slice, 'input> {
    Parsed(&'slice [TokenAttribute<'input>]),
    Raw(&'slice str),
    None,
}

#[derive(Clone, Copy)]
enum InsertionLocation {
    Append(NodeId),
    Before { parent: NodeId, reference: NodeId },
}

#[inline]
fn matches_ascii_name(name: &str, expected: &[&str]) -> bool {
    expected
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Known block-level starts that imply the end of an open `<p>` element.
///
/// This covers every such element represented by the compact [`Tag`] enum;
/// unknown/custom elements do not implicitly close a paragraph.
#[inline]
fn closes_p_element(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::Article
            | Tag::Aside
            | Tag::Div
            | Tag::Footer
            | Tag::Form
            | Tag::H1
            | Tag::H2
            | Tag::H3
            | Tag::H4
            | Tag::H5
            | Tag::H6
            | Tag::Header
            | Tag::Hr
            | Tag::Main
            | Tag::Nav
            | Tag::Ol
            | Tag::P
            | Tag::Pre
            | Tag::Section
            | Tag::Table
            | Tag::Ul
    )
}

/// Builds a DOM tree from a token stream.
///
/// Maintains an open-elements stack and processes each token to either
/// push new nodes, pop closed elements, or append text/comment/doctype nodes.
pub struct TreeBuilder {
    /// The arena that owns all nodes.
    pub(crate) arena: Arena,
    /// Stack of open element node ids with cached tags and element child count.
    open_elements: Vec<OpenElement>,
    /// Counts by private element category for absence checks before stack scans.
    kind_counts: [u16; ElementKind::COUNT],
    /// Effective insertion mode of the final open-element entry.
    insertion_mode: InsertionMode,
    /// Active formatting nodes used by the common adoption-repair path.
    active_formatting: Vec<NodeId>,
    /// The synthetic root node (document root).
    root: NodeId,
    /// Base address of the original input (for source-backed text).
    source_base: usize,
    /// Length of the original input in bytes.
    source_len: usize,
    /// Terminal construction error. The infallible `TreeSink` path records it
    /// here and [`TreeBuilder::finish`] surfaces it.
    error: Option<ParseError>,
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
        let capacity_hint_len = input_len.min(MAX_CAPACITY_HINT_INPUT);
        let node_cap = (capacity_hint_len / 32).max(256);
        // Source-backed text uses offsets, not slab — smaller alloc suffices.
        let text_cap = (capacity_hint_len / 16).max(4096);
        let attr_cap = (capacity_hint_len / 128).max(64);
        let mut arena = Arena::with_capacity(node_cap, text_cap, attr_cap);
        // Create a synthetic document root.
        let root = arena.new_element(Tag::Unknown, 0);
        let mut open_elements = Vec::with_capacity(32);
        open_elements.push(OpenElement {
            id: root,
            tag: Tag::Unknown,
            kind: ElementKind::Root,
            insertion_mode: InsertionMode::InBody,
            element_child_count: 0,
        });
        let mut kind_counts = [0; ElementKind::COUNT];
        kind_counts[ElementKind::Root.index()] = 1;
        Self {
            arena,
            open_elements,
            kind_counts,
            insertion_mode: InsertionMode::InBody,
            active_formatting: Vec::new(),
            root,
            source_base: 0,
            source_len: 0,
            error: None,
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
    /// The builder becomes terminal after a construction error. No partial
    /// document is returned after the nesting limit is exceeded.
    #[inline]
    pub fn process(&mut self, token: &Token<'_>) -> Result<Option<NodeId>, ParseError> {
        self.ensure_active()?;
        match token {
            Token::OpenTag {
                tag,
                name,
                attributes,
                self_closing,
            } => self.handle_open_tag(
                *tag,
                name.as_ref(),
                PendingAttrs::Parsed(attributes),
                *self_closing,
            ),
            Token::CloseTag { tag, name } => {
                self.handle_close_tag(*tag, name.as_ref());
                Ok(None)
            }
            Token::Text { content } => Ok(self.handle_text(content.as_ref())),
            Token::Comment { content } => Ok(self.handle_comment(content.as_ref())),
            Token::Doctype { content } => Ok(self.handle_doctype(content.as_ref())),
            Token::CData { content } => Ok(self.handle_text(content.as_ref())),
        }
    }

    /// Finish building and return the root node id and arena.
    pub fn finish(self) -> Result<(Arena, NodeId), ParseError> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            Ok((self.arena, self.root))
        }
    }

    /// Whether a node is still present on the open-elements stack.
    #[inline]
    #[cfg(feature = "encoding")]
    pub(crate) fn is_open(&self, node: NodeId) -> bool {
        self.open_elements.iter().any(|entry| entry.id == node)
    }

    #[inline]
    fn ensure_active(&self) -> Result<(), ParseError> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    #[inline]
    fn current_parent(&self) -> NodeId {
        self.open_elements
            .last()
            .map(|entry| entry.id)
            .unwrap_or(self.root)
    }

    #[inline]
    fn kind_is_open(&self, kind: ElementKind) -> bool {
        self.kind_counts[kind.index()] != 0
    }

    #[inline]
    fn push_open_element(&mut self, id: NodeId, tag: Tag, kind: ElementKind) {
        let insertion_mode = kind.pushed_mode(self.insertion_mode);
        self.open_elements.push(OpenElement {
            id,
            tag,
            kind,
            insertion_mode,
            element_child_count: 0,
        });
        self.kind_counts[kind.index()] += 1;
        self.insertion_mode = insertion_mode;
    }

    /// Pop the common single-entry close path without setting up a truncate
    /// loop. The synthetic root is never removed.
    #[inline]
    fn pop_open_element(&mut self) {
        debug_assert!(self.open_elements.len() > 1);
        let entry = self
            .open_elements
            .pop()
            .expect("open element stack is non-empty");
        let count = &mut self.kind_counts[entry.kind.index()];
        debug_assert!(*count > 0);
        *count -= 1;
        if entry.kind == ElementKind::Formatting {
            self.remove_active_formatting(entry.id);
        }
        self.insertion_mode = self
            .open_elements
            .last()
            .expect("synthetic root is never removed")
            .insertion_mode;
    }

    fn handle_open_tag<'slice, 'input>(
        &mut self,
        tag: Tag,
        name: &str,
        attributes: PendingAttrs<'slice, 'input>,
        source_self_closing: bool,
    ) -> Result<Option<NodeId>, ParseError> {
        let kind = ElementKind::classify(tag, name);

        if self.insertion_mode == InsertionMode::InSelect {
            if is_select_breakout_start(tag, name) {
                self.close_last_kind(ElementKind::Select);
                return self.handle_open_tag(tag, name, attributes, source_self_closing);
            }
            match kind {
                ElementKind::Option => self.close_last_kind(ElementKind::Option),
                ElementKind::OptGroup => {
                    self.close_last_kind(ElementKind::Option);
                    self.close_last_kind(ElementKind::OptGroup);
                }
                ElementKind::Select => {
                    self.close_last_kind(ElementKind::Select);
                    return Ok(None);
                }
                _ => return Ok(None),
            }
        }

        let mut foster = false;
        match self.insertion_mode {
            InsertionMode::InCell if is_table_structure(kind) => {
                self.close_last_kind(ElementKind::Cell);
                return self.handle_open_tag(tag, name, attributes, source_self_closing);
            }
            InsertionMode::InRow => match kind {
                ElementKind::Cell => {}
                ElementKind::Row | ElementKind::TableBody | ElementKind::Table => {
                    self.close_last_kind(ElementKind::Row);
                    return self.handle_open_tag(tag, name, attributes, source_self_closing);
                }
                _ => foster = true,
            },
            InsertionMode::InTableBody => match kind {
                ElementKind::Row => {}
                ElementKind::Cell => {
                    self.insert_implied(Tag::Tr, "tr", ElementKind::Row)?;
                }
                ElementKind::TableBody | ElementKind::Table => {
                    self.close_last_kind(ElementKind::TableBody);
                    return self.handle_open_tag(tag, name, attributes, source_self_closing);
                }
                _ => foster = true,
            },
            InsertionMode::InTable => match kind {
                ElementKind::TableBody => {}
                ElementKind::Row => {
                    self.insert_implied(Tag::Tbody, "tbody", ElementKind::TableBody)?;
                }
                ElementKind::Cell => {
                    self.insert_implied(Tag::Tbody, "tbody", ElementKind::TableBody)?;
                    self.insert_implied(Tag::Tr, "tr", ElementKind::Row)?;
                }
                ElementKind::Table => {
                    self.close_last_kind(ElementKind::Table);
                }
                _ => foster = true,
            },
            _ => {}
        }

        if tag == Tag::A {
            // A nested anchor start repairs the previous active anchor before
            // the new element is inserted. Reuse the deliberately simplified
            // adoption path used by formatting end tags.
            self.adoption_close(tag, name);
        }
        self.apply_body_implicit_close(tag, kind);
        let location = if foster {
            self.foster_location()
        } else {
            InsertionLocation::Append(self.current_parent())
        };
        let node =
            self.insert_element(tag, name, kind, attributes, source_self_closing, location)?;
        Ok(Some(node))
    }

    fn insert_implied(
        &mut self,
        tag: Tag,
        name: &str,
        kind: ElementKind,
    ) -> Result<NodeId, ParseError> {
        self.insert_element(
            tag,
            name,
            kind,
            PendingAttrs::None,
            false,
            InsertionLocation::Append(self.current_parent()),
        )
    }

    fn insert_element<'slice, 'input>(
        &mut self,
        tag: Tag,
        name: &str,
        kind: ElementKind,
        attributes: PendingAttrs<'slice, 'input>,
        source_self_closing: bool,
        location: InsertionLocation,
    ) -> Result<NodeId, ParseError> {
        if self.open_elements.len() > usize::from(MAX_DEPTH) {
            let error = ParseError::NestingTooDeep {
                depth: u32::from(MAX_DEPTH) + 1,
                limit: u32::from(MAX_DEPTH),
            };
            self.error = Some(error.clone());
            return Err(error);
        }

        let parent = match location {
            InsertionLocation::Append(parent) => parent,
            InsertionLocation::Before { parent, .. } => parent,
        };
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.arena.new_element(tag, depth);
        if tag == Tag::Unknown {
            self.arena.set_unknown_tag_name(node, name);
        }
        match attributes {
            PendingAttrs::Parsed(attributes) if !attributes.is_empty() => {
                self.arena.set_attrs(node, attributes);
            }
            PendingAttrs::Raw(raw) if !raw.is_empty() => {
                self.arena.set_attrs_from_raw(node, raw);
            }
            _ => {}
        }

        match location {
            InsertionLocation::Append(parent) => {
                self.arena.append_child_trusted(parent, node);
                self.register_appended_element(parent, node);
            }
            InsertionLocation::Before { parent, reference } => {
                self.arena.insert_before(parent, reference, node);
                let count = self.arena.recompute_element_indices(parent);
                if let Some(entry) = self
                    .open_elements
                    .iter_mut()
                    .find(|entry| entry.id == parent)
                {
                    entry.element_child_count = count;
                }
            }
        }

        let is_void = tag.is_void();
        if source_self_closing || is_void {
            self.arena.set_self_closing(node);
        }
        // A slash on a non-void HTML element is syntactic metadata only.
        if !is_void {
            self.push_open_element(node, tag, kind);
            if kind == ElementKind::Formatting {
                self.active_formatting.push(node);
            }
        }
        Ok(node)
    }

    fn handle_close_tag(&mut self, tag: Tag, name: &str) {
        if tag.is_void() {
            return;
        }

        // The overwhelmingly common close is the current element. Handle it
        // before classifying the tag or entering table/formatting recovery.
        // All specialized well-formed paths reduce to this same single pop.
        let last_index = self.open_elements.len() - 1;
        if last_index > 0 && self.entry_matches(last_index, tag, name) {
            self.pop_open_element();
            return;
        }

        let kind = ElementKind::classify(tag, name);
        if self.insertion_mode == InsertionMode::InSelect {
            match kind {
                ElementKind::Option => self.close_last_kind(ElementKind::Option),
                ElementKind::OptGroup => {
                    self.close_last_kind(ElementKind::Option);
                    self.close_last_kind(ElementKind::OptGroup);
                }
                ElementKind::Select => self.close_last_kind(ElementKind::Select),
                _ => {}
            }
            return;
        }

        if kind == ElementKind::Formatting {
            self.adoption_close(tag, name);
            return;
        }

        match kind {
            ElementKind::Cell | ElementKind::Row | ElementKind::TableBody | ElementKind::Table => {
                self.close_table_through(kind)
            }
            _ => self.close_matching(tag, name, kind),
        }
    }

    fn close_matching(&mut self, tag: Tag, name: &str, kind: ElementKind) {
        if !self.kind_is_open(kind) {
            return;
        }
        let last_index = self.open_elements.len() - 1;
        if let Some(index) = (1..last_index)
            .rev()
            .find(|&index| self.entry_matches(index, tag, name))
        {
            self.truncate_stack(index);
        }
    }

    fn entry_matches(&self, index: usize, tag: Tag, name: &str) -> bool {
        let entry = self.open_elements[index];
        if entry.tag != tag {
            return false;
        }
        tag != Tag::Unknown
            || self
                .arena
                .unknown_tag_name(entry.id)
                .is_some_and(|open_name| open_name.eq_ignore_ascii_case(name))
    }

    fn close_last_kind(&mut self, kind: ElementKind) {
        if !self.kind_is_open(kind) {
            return;
        }
        let last_index = self.open_elements.len() - 1;
        if self.open_elements[last_index].kind == kind {
            self.pop_open_element();
            return;
        }
        if let Some(index) = self.open_elements[..last_index]
            .iter()
            .rposition(|entry| entry.kind == kind)
        {
            if index > 0 {
                self.truncate_stack(index);
            }
        }
    }

    /// Close table-scoped categories in the same order as the recovery
    /// algorithm, using one reverse traversal of the open-elements stack.
    fn close_table_through(&mut self, through: ElementKind) {
        const TABLE_KINDS: [ElementKind; 4] = [
            ElementKind::Cell,
            ElementKind::Row,
            ElementKind::TableBody,
            ElementKind::Table,
        ];

        let through_index = match through {
            ElementKind::Cell => 0,
            ElementKind::Row => 1,
            ElementKind::TableBody => 2,
            ElementKind::Table => 3,
            _ => return,
        };
        let mut remaining = TABLE_KINDS.map(|kind| self.kind_counts[kind.index()]);
        let mut stage = 0;
        while stage <= through_index && remaining[stage] == 0 {
            stage += 1;
        }
        if stage > through_index {
            return;
        }

        let mut truncate_to = self.open_elements.len();
        for index in (1..self.open_elements.len()).rev() {
            let kind = self.open_elements[index].kind;
            if let Some(kind_index) = table_kind_index(kind) {
                remaining[kind_index] -= 1;
            }
            if kind != TABLE_KINDS[stage] {
                continue;
            }

            truncate_to = index;
            stage += 1;
            while stage <= through_index && remaining[stage] == 0 {
                stage += 1;
            }
            if stage > through_index {
                break;
            }
        }

        debug_assert!(stage > through_index);
        self.truncate_stack(truncate_to);
    }

    fn adoption_close(&mut self, tag: Tag, name: &str) {
        if !self.kind_is_open(ElementKind::Formatting) {
            return;
        }
        let Some(index) = (1..self.open_elements.len())
            .rev()
            .find(|&index| self.entry_matches(index, tag, name))
        else {
            return;
        };
        // Well-formed formatting closes dominate real-world input. They need
        // no recovery allocation and no shallow element clones.
        if index + 1 == self.open_elements.len() {
            self.pop_open_element();
            return;
        }

        let has_formatting_to_reopen = self.open_elements[index + 1..]
            .iter()
            .any(|entry| entry.kind == ElementKind::Formatting);
        if !has_formatting_to_reopen {
            self.truncate_stack(index);
            return;
        }

        let reopen: Vec<OpenElement> = self.open_elements[index + 1..]
            .iter()
            .copied()
            .filter(|entry| entry.kind == ElementKind::Formatting)
            .collect();
        self.truncate_stack(index);

        for old in reopen {
            let parent = self.current_parent();
            let depth = self.arena.get(parent).depth.saturating_add(1);
            let node = self.arena.clone_element_shallow(old.id, depth);
            self.arena.append_child_trusted(parent, node);
            self.register_appended_element(parent, node);
            self.push_open_element(node, old.tag, old.kind);
            self.active_formatting.push(node);
        }
    }

    #[inline]
    fn remove_active_formatting(&mut self, node: NodeId) {
        if self.active_formatting.last() == Some(&node) {
            self.active_formatting.pop();
        } else {
            self.active_formatting.retain(|&active| active != node);
        }
    }

    fn register_appended_element(&mut self, parent: NodeId, node: NodeId) {
        if let Some(entry) = self.open_elements.last_mut() {
            if entry.id == parent {
                entry.element_child_count = entry.element_child_count.saturating_add(1);
                self.arena
                    .set_full_element_index(node, entry.element_child_count);
                return;
            }
        }

        // Recovery/foster paths may target an ancestor or an arena parent no
        // longer present on the stack.
        if let Some(entry) = self
            .open_elements
            .iter_mut()
            .rev()
            .find(|entry| entry.id == parent)
        {
            entry.element_child_count = entry.element_child_count.saturating_add(1);
            self.arena
                .set_full_element_index(node, entry.element_child_count);
            return;
        }
        self.arena.recompute_element_indices(parent);
    }

    fn truncate_stack(&mut self, len: usize) {
        let len = len.max(1);
        if len >= self.open_elements.len() {
            return;
        }
        if len + 1 == self.open_elements.len() {
            self.pop_open_element();
            return;
        }
        for index in (len..self.open_elements.len()).rev() {
            let entry = self.open_elements[index];
            let count = &mut self.kind_counts[entry.kind.index()];
            debug_assert!(*count > 0);
            *count -= 1;
            if entry.kind == ElementKind::Formatting {
                // Most truncations remove no formatting nodes and touch no
                // formatting storage. When they do, reverse stack order makes
                // the well-formed case a sequence of allocation-free Vec pops
                // while still cleaning malformed non-tail entries.
                self.remove_active_formatting(entry.id);
            }
        }
        self.open_elements.truncate(len);
        self.insertion_mode = self
            .open_elements
            .last()
            .expect("synthetic root is never removed")
            .insertion_mode;
    }

    fn apply_body_implicit_close(&mut self, tag: Tag, kind: ElementKind) {
        if self.kind_is_open(ElementKind::Paragraph)
            && (closes_p_element(tag) || kind == ElementKind::Paragraph)
        {
            self.close_last_kind(ElementKind::Paragraph);
        }
        match kind {
            ElementKind::ListItem => self.close_last_kind(ElementKind::ListItem),
            ElementKind::DefinitionItem => self.close_last_kind(ElementKind::DefinitionItem),
            ElementKind::Heading => self.close_last_kind(ElementKind::Heading),
            ElementKind::Option => self.close_last_kind(ElementKind::Option),
            ElementKind::OptGroup => {
                self.close_last_kind(ElementKind::Option);
                self.close_last_kind(ElementKind::OptGroup);
            }
            _ => {}
        }
    }

    fn foster_location(&self) -> InsertionLocation {
        let Some(table) = self
            .open_elements
            .iter()
            .rev()
            .find(|entry| entry.kind == ElementKind::Table)
            .map(|entry| entry.id)
        else {
            return InsertionLocation::Append(self.current_parent());
        };
        let table_node = self.arena.get(table);
        if table_node.parent.is_null() {
            return InsertionLocation::Append(self.root);
        }
        let parent = table_node.parent;
        InsertionLocation::Before {
            parent,
            reference: table,
        }
    }

    /// Handle text content (already entity-decoded by the tokenizer).
    fn handle_text(&mut self, content: &str) -> Option<NodeId> {
        if content.is_empty() {
            return None;
        }
        let foster = matches!(
            self.insertion_mode,
            InsertionMode::InTable | InsertionMode::InTableBody | InsertionMode::InRow
        ) && !content.bytes().all(|byte| byte.is_ascii_whitespace());
        let location = if foster {
            self.foster_location()
        } else {
            InsertionLocation::Append(self.current_parent())
        };
        let parent = match location {
            InsertionLocation::Append(parent) => parent,
            InsertionLocation::Before { parent, .. } => parent,
        };
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.try_source_ref(depth, content);
        self.insert_non_element(location, node);
        Some(node)
    }

    /// Handle raw text from TreeSink (not entity-decoded).
    fn handle_raw_text(&mut self, raw: &str) -> Option<NodeId> {
        if raw.is_empty() {
            return None;
        }
        let foster = matches!(
            self.insertion_mode,
            InsertionMode::InTable | InsertionMode::InTableBody | InsertionMode::InRow
        ) && !raw.bytes().all(|byte| byte.is_ascii_whitespace());
        let location = if foster {
            self.foster_location()
        } else {
            InsertionLocation::Append(self.current_parent())
        };
        let parent = match location {
            InsertionLocation::Append(parent) => parent,
            InsertionLocation::Before { parent, .. } => parent,
        };
        let depth = self.arena.get(parent).depth.saturating_add(1);

        #[cfg(feature = "entity-decode")]
        let node = {
            let parent_is_raw_text = self.arena.get(parent).tag.is_raw_text()
                || self
                    .open_elements
                    .last()
                    .is_some_and(|entry| entry.kind == ElementKind::Plaintext);
            if parent_is_raw_text {
                self.try_source_ref(depth, raw)
            } else {
                let decoded = fhp_tokenizer::entity::decode_entities(raw);
                match decoded {
                    std::borrow::Cow::Borrowed(value) => self.try_source_ref(depth, value),
                    std::borrow::Cow::Owned(value) => self.arena.new_text(depth, &value),
                }
            }
        };

        #[cfg(not(feature = "entity-decode"))]
        let node = self.try_source_ref(depth, raw);

        self.insert_non_element(location, node);
        Some(node)
    }

    fn insert_non_element(&mut self, location: InsertionLocation, node: NodeId) {
        match location {
            InsertionLocation::Append(parent) => self.arena.append_child_trusted(parent, node),
            InsertionLocation::Before { parent, reference } => {
                self.arena.insert_before(parent, reference, node);
            }
        }
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
        let parent = self.current_parent();
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.arena.new_comment(depth, content);
        self.arena.append_child_trusted(parent, node);
        Some(node)
    }

    /// Handle a doctype declaration.
    fn handle_doctype(&mut self, content: &str) -> Option<NodeId> {
        let parent = self.current_parent();
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.arena.new_doctype(depth, content);
        self.arena.append_child_trusted(parent, node);
        Some(node)
    }
}

#[inline]
fn is_table_structure(kind: ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Table | ElementKind::TableBody | ElementKind::Row | ElementKind::Cell
    )
}

#[inline]
fn is_select_breakout_start(tag: Tag, name: &str) -> bool {
    matches!(tag, Tag::Input | Tag::Textarea)
        || (tag == Tag::Unknown && name.eq_ignore_ascii_case("keygen"))
}

#[inline]
const fn table_kind_index(kind: ElementKind) -> Option<usize> {
    match kind {
        ElementKind::Cell => Some(0),
        ElementKind::Row => Some(1),
        ElementKind::TableBody => Some(2),
        ElementKind::Table => Some(3),
        _ => None,
    }
}

impl fhp_tokenizer::TreeSink for TreeBuilder {
    fn open_tag(&mut self, tag: Tag, name: &str, attr_raw: &str, self_closing: bool) {
        if self.error.is_none() {
            let _ = self.handle_open_tag(tag, name, PendingAttrs::Raw(attr_raw), self_closing);
        }
    }

    fn close_tag(&mut self, tag: Tag, name: &str) {
        if self.error.is_none() {
            self.handle_close_tag(tag, name);
        }
    }

    fn text(&mut self, raw: &str) {
        if self.error.is_none() {
            self.handle_raw_text(raw);
        }
    }

    fn comment(&mut self, content: &str) {
        if self.error.is_none() {
            self.handle_comment(content);
        }
    }

    fn doctype(&mut self, content: &str) {
        if self.error.is_none() {
            self.handle_doctype(content);
        }
    }

    fn cdata(&mut self, content: &str) {
        // CDATA treated as text.
        if self.error.is_none() {
            self.handle_raw_text(content);
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

    fn process(builder: &mut TreeBuilder, token: &Token<'_>) -> Option<NodeId> {
        builder.process(token).unwrap()
    }

    fn finish(builder: TreeBuilder) -> (Arena, NodeId) {
        builder.finish().unwrap()
    }

    fn parse(html: &str) -> Result<(Arena, NodeId), ParseError> {
        let mut builder = TreeBuilder::with_capacity_hint(html.len());
        fhp_tokenizer::tokenize_into(html, &mut builder);
        builder.finish()
    }

    fn assert_stack_invariants(builder: &TreeBuilder) {
        let mut counts = [0; ElementKind::COUNT];
        let mut mode = InsertionMode::InBody;
        for entry in &builder.open_elements {
            counts[entry.kind.index()] += 1;
            mode = entry.kind.pushed_mode(mode);
            assert_eq!(entry.insertion_mode, mode);
        }
        assert_eq!(builder.kind_counts, counts);
        assert_eq!(builder.insertion_mode, mode);
    }

    #[test]
    fn simple_tree() {
        let mut builder = TreeBuilder::new();
        process(&mut builder, &make_open(Tag::Div));
        process(&mut builder, &make_text("hello"));
        process(&mut builder, &make_close(Tag::Div));

        let (arena, root) = finish(builder);

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
        process(&mut builder, &make_open(Tag::Div));
        process(
            &mut builder,
            &Token::OpenTag {
                tag: Tag::Br,
                name: Cow::Borrowed("br"),
                attributes: vec![],
                self_closing: false,
            },
        );
        process(&mut builder, &make_text("after br"));
        process(&mut builder, &make_close(Tag::Div));

        let (arena, root) = finish(builder);
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
        process(&mut builder, &make_open(Tag::P));
        process(&mut builder, &make_text("first"));
        process(&mut builder, &make_open(Tag::P));
        process(&mut builder, &make_text("second"));
        process(&mut builder, &make_close(Tag::P));

        let (arena, root) = finish(builder);

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
    fn block_start_closes_nested_paragraph() {
        let mut builder = TreeBuilder::new();
        process(&mut builder, &make_open(Tag::P));
        process(&mut builder, &make_open(Tag::Span));
        process(&mut builder, &make_text("first"));
        process(&mut builder, &make_open(Tag::Div));
        process(&mut builder, &make_text("second"));

        let (arena, root) = finish(builder);
        let p = arena.get(root).first_child;
        let div = arena.get(p).next_sibling;

        assert_eq!(arena.get(p).tag, Tag::P);
        assert_eq!(arena.get(div).tag, Tag::Div);
        assert_eq!(arena.get(div).parent, root);
    }

    #[test]
    fn large_plain_text_hint_does_not_preallocate_for_every_possible_node() {
        let input_len = 256 * 1024 * 1024;
        let builder = TreeBuilder::with_capacity_hint(input_len);

        assert!(
            builder.arena.nodes.capacity() < input_len / 64,
            "node capacity {} is disproportionate to a plain-text input",
            builder.arena.nodes.capacity()
        );
    }

    #[test]
    fn mismatched_close_finds_nearest() {
        // <div><span></div> — should close both span and div.
        let mut builder = TreeBuilder::new();
        process(&mut builder, &make_open(Tag::Div));
        process(&mut builder, &make_open(Tag::Span));
        process(&mut builder, &make_text("hi"));
        process(&mut builder, &make_close(Tag::Div));

        let (arena, root) = finish(builder);
        let div = arena.get(root).first_child;
        assert_eq!(arena.get(div).tag, Tag::Div);
    }

    #[test]
    fn extra_close_tag_ignored() {
        let mut builder = TreeBuilder::new();
        process(&mut builder, &make_close(Tag::Div)); // No matching open — ignored.
        process(&mut builder, &make_open(Tag::P));
        process(&mut builder, &make_text("ok"));
        process(&mut builder, &make_close(Tag::P));

        let (arena, root) = finish(builder);
        let p = arena.get(root).first_child;
        assert_eq!(arena.get(p).tag, Tag::P);
    }

    #[test]
    fn unknown_close_matches_by_name() {
        let mut builder = TreeBuilder::new();
        process(
            &mut builder,
            &Token::OpenTag {
                tag: Tag::Unknown,
                name: Cow::Borrowed("my-widget"),
                attributes: vec![],
                self_closing: false,
            },
        );
        process(
            &mut builder,
            &Token::OpenTag {
                tag: Tag::Unknown,
                name: Cow::Borrowed("x-item"),
                attributes: vec![],
                self_closing: false,
            },
        );
        process(
            &mut builder,
            &Token::CloseTag {
                tag: Tag::Unknown,
                name: Cow::Borrowed("my-widget"),
            },
        );

        let (arena, root) = finish(builder);
        let my_widget = arena.get(root).first_child;
        let x_item = arena.get(my_widget).first_child;
        assert_eq!(arena.unknown_tag_name(my_widget), Some("my-widget"));
        assert_eq!(arena.unknown_tag_name(x_item), Some("x-item"));
    }

    #[test]
    fn non_void_trailing_slash_does_not_close_element() {
        let (arena, root) = parse("<div/>after").unwrap();
        let div = arena.get(root).first_child;
        assert_eq!(arena.get(div).tag, Tag::Div);
        assert_eq!(arena.text(arena.get(div).first_child), "after");
    }

    #[test]
    fn table_cells_gain_implied_body_and_row() {
        let (arena, root) = parse("<table><td>x</td></table>").unwrap();
        let table = arena.get(root).first_child;
        let tbody = arena.get(table).first_child;
        let tr = arena.get(tbody).first_child;
        let td = arena.get(tr).first_child;
        assert_eq!(arena.get(tbody).tag, Tag::Tbody);
        assert_eq!(arena.get(tr).tag, Tag::Tr);
        assert_eq!(arena.get(td).tag, Tag::Td);
        assert_eq!(arena.text(arena.get(td).first_child), "x");
    }

    #[test]
    fn table_text_is_foster_parented_before_table() {
        let (arena, root) = parse("<table>before<tr><td>x</td></tr>after</table>").unwrap();
        let first = arena.get(root).first_child;
        assert!(arena.get(first).flags.has(NodeFlags::IS_TEXT));
        assert_eq!(arena.text(first), "before");
        let second = arena.get(first).next_sibling;
        assert!(arena.get(second).flags.has(NodeFlags::IS_TEXT));
        assert_eq!(arena.text(second), "after");
        assert_eq!(arena.get(arena.get(second).next_sibling).tag, Tag::Table);
    }

    #[test]
    fn foster_text_stays_a_sibling_when_table_has_a_previous_element() {
        let (arena, root) = parse("<p>before</p><table>foster<tr><td>x</td></tr></table>").unwrap();
        let paragraph = arena.get(root).first_child;
        let fostered = arena.get(paragraph).next_sibling;
        let table = arena.get(fostered).next_sibling;

        assert_eq!(arena.get(paragraph).tag, Tag::P);
        assert_eq!(arena.text(arena.get(paragraph).first_child), "before");
        assert!(arena.get(fostered).flags.has(NodeFlags::IS_TEXT));
        assert_eq!(arena.text(fostered), "foster");
        assert_eq!(arena.get(table).tag, Tag::Table);
    }

    #[test]
    fn select_filters_invalid_elements_and_closes_options() {
        let (arena, root) = parse("<select><div>x<option>a<option>b</select>").unwrap();
        let select = arena.get(root).first_child;
        let text = arena.get(select).first_child;
        assert_eq!(arena.text(text), "x");
        let first_option = arena.get(text).next_sibling;
        let second_option = arena.get(first_option).next_sibling;
        assert_eq!(arena.unknown_tag_name(first_option), Some("option"));
        assert_eq!(arena.unknown_tag_name(second_option), Some("option"));
        assert_eq!(arena.text(arena.get(first_option).first_child), "a");
        assert_eq!(arena.text(arena.get(second_option).first_child), "b");
    }

    #[test]
    fn nested_select_closes_the_current_select_and_ignores_the_start() {
        let (arena, root) = parse("<select><option>x<select><input>").unwrap();
        let select = arena.get(root).first_child;
        let input = arena.get(select).next_sibling;

        assert_eq!(arena.get(select).tag, Tag::Select);
        assert_eq!(arena.get(input).tag, Tag::Input);
        assert!(arena.get(input).next_sibling.is_null());
    }

    #[test]
    fn select_breakout_starts_are_reprocessed_outside_the_select() {
        let (arena, root) = parse("<select><option>x<input>tail").unwrap();
        let select = arena.get(root).first_child;
        let input = arena.get(select).next_sibling;
        let tail = arena.get(input).next_sibling;

        assert_eq!(arena.get(select).tag, Tag::Select);
        assert_eq!(arena.get(input).tag, Tag::Input);
        assert_eq!(arena.text(tail), "tail");

        let (arena, root) = parse("<select>x<keygen>tail").unwrap();
        let select = arena.get(root).first_child;
        let keygen = arena.get(select).next_sibling;
        assert_eq!(arena.unknown_tag_name(keygen), Some("keygen"));
        assert_eq!(arena.get(keygen).parent, root);

        let (arena, root) = parse("<select>x<textarea>tail</textarea>").unwrap();
        let select = arena.get(root).first_child;
        let textarea = arena.get(select).next_sibling;
        assert_eq!(arena.get(textarea).tag, Tag::Textarea);
        assert_eq!(arena.get(textarea).parent, root);
        assert_eq!(arena.text(arena.get(textarea).first_child), "tail");
    }

    #[test]
    fn formatting_close_reopens_nested_formatting_element() {
        let (arena, root) = parse("<b><i>x</b>y</i>").unwrap();
        let b = arena.get(root).first_child;
        let reopened_i = arena.get(b).next_sibling;
        assert_eq!(arena.get(b).tag, Tag::B);
        assert_eq!(arena.get(arena.get(b).first_child).tag, Tag::I);
        assert_eq!(arena.get(reopened_i).tag, Tag::I);
        assert_eq!(arena.text(arena.get(reopened_i).first_child), "y");
    }

    #[test]
    fn code_and_tt_participate_in_formatting_repair() {
        for formatting_name in ["code", "tt"] {
            let html = format!("<{formatting_name}><i>x</{formatting_name}>y</i>");
            let (arena, root) = parse(&html).unwrap();
            let formatting = arena.get(root).first_child;
            let reopened_i = arena.get(formatting).next_sibling;

            assert_eq!(arena.unknown_tag_name(formatting), Some(formatting_name));
            assert_eq!(arena.get(arena.get(formatting).first_child).tag, Tag::I);
            assert_eq!(arena.get(reopened_i).tag, Tag::I);
            assert_eq!(arena.text(arena.get(reopened_i).first_child), "y");
        }
    }

    #[test]
    fn nested_anchor_start_repairs_the_previous_anchor() {
        let (arena, root) = parse("<a>one<a>two</a>").unwrap();
        let first = arena.get(root).first_child;
        let second = arena.get(first).next_sibling;

        assert_eq!(arena.get(first).tag, Tag::A);
        assert_eq!(arena.text(arena.get(first).first_child), "one");
        assert_eq!(arena.get(second).tag, Tag::A);
        assert_eq!(arena.text(arena.get(second).first_child), "two");
        assert!(arena.get(second).next_sibling.is_null());
    }

    #[test]
    fn generic_truncation_does_not_accumulate_stale_formatting_entries() {
        let mut builder = TreeBuilder::new();
        for _ in 0..2_048 {
            process(&mut builder, &make_open(Tag::Div));
            process(&mut builder, &make_open(Tag::B));
            process(&mut builder, &make_close(Tag::Div));
            assert!(builder.active_formatting.is_empty());
            assert_stack_invariants(&builder);
        }
    }

    #[test]
    fn well_formed_formatting_close_is_allocation_free() {
        let mut builder = TreeBuilder::new();
        assert_eq!(builder.active_formatting.capacity(), 0);

        process(&mut builder, &make_open(Tag::B));
        let node_count = builder.arena.nodes.len();
        process(&mut builder, &make_close(Tag::B));

        assert_eq!(builder.arena.nodes.len(), node_count);
        assert!(builder.active_formatting.is_empty());
        assert_eq!(builder.open_elements.len(), 1);
        assert_stack_invariants(&builder);
    }

    #[test]
    fn insertion_mode_and_kind_counts_restore_from_stack_prefix() {
        let mut builder = TreeBuilder::new();
        for tag in [Tag::Table, Tag::Tr, Tag::Td, Tag::Select] {
            process(&mut builder, &make_open(tag));
            assert_stack_invariants(&builder);
        }
        assert_eq!(builder.insertion_mode, InsertionMode::InSelect);

        for tag in [Tag::Select, Tag::Td, Tag::Tr, Tag::Tbody, Tag::Table] {
            process(&mut builder, &make_close(tag));
            assert_stack_invariants(&builder);
        }
        assert_eq!(builder.insertion_mode, InsertionMode::InBody);
        assert_eq!(builder.open_elements.len(), 1);
    }

    #[test]
    fn table_scope_close_matches_sequential_recovery_on_malformed_stacks() {
        const TABLE_KINDS: [ElementKind; 4] = [
            ElementKind::Cell,
            ElementKind::Row,
            ElementKind::TableBody,
            ElementKind::Table,
        ];
        let stacks = [
            vec![
                ElementKind::Table,
                ElementKind::TableBody,
                ElementKind::Row,
                ElementKind::Cell,
            ],
            vec![
                ElementKind::TableBody,
                ElementKind::Cell,
                ElementKind::TableBody,
            ],
            vec![
                ElementKind::Table,
                ElementKind::Row,
                ElementKind::Cell,
                ElementKind::Row,
            ],
            vec![
                ElementKind::Cell,
                ElementKind::Table,
                ElementKind::TableBody,
                ElementKind::Row,
            ],
        ];

        for stack in stacks {
            for (through_index, through) in TABLE_KINDS.into_iter().enumerate() {
                let mut expected_len = stack.len() + 1;
                for expected_kind in &TABLE_KINDS[..=through_index] {
                    if let Some(index) = (1..expected_len)
                        .rev()
                        .find(|&index| stack[index - 1] == *expected_kind)
                    {
                        expected_len = index;
                    }
                }

                let mut builder = TreeBuilder::new();
                for kind in &stack {
                    let node = builder.arena.new_element(Tag::Div, 1);
                    builder.push_open_element(node, Tag::Div, *kind);
                }
                builder.close_table_through(through);

                assert_eq!(builder.open_elements.len(), expected_len);
                assert_stack_invariants(&builder);
            }
        }
    }

    #[test]
    fn plaintext_keeps_markup_literal() {
        let (arena, root) = parse("<plaintext><b>x&amp;").unwrap();
        let plaintext = arena.get(root).first_child;
        assert_eq!(arena.unknown_tag_name(plaintext), Some("plaintext"));
        assert_eq!(arena.text(arena.get(plaintext).first_child), "<b>x&amp;");
    }

    #[test]
    fn duplicate_attributes_are_first_wins() {
        let (arena, root) = parse("<div ID=first id='second' class=a CLASS=b></div>").unwrap();
        let div = arena.get(root).first_child;
        let attrs = arena.attrs(div);
        assert_eq!(attrs.len(), 2);
        assert_eq!(arena.attr_name(&attrs[0]), "ID");
        assert_eq!(arena.attr_value(&attrs[0]), Some("first"));
        assert_eq!(arena.attr_value(&attrs[1]), Some("a"));
    }

    #[test]
    fn depth_limit_is_strict_and_terminal() {
        let mut builder = TreeBuilder::new();
        for _ in 0..MAX_DEPTH {
            process(&mut builder, &make_open(Tag::Div));
        }
        let error = builder.process(&make_open(Tag::Div)).unwrap_err();
        assert_eq!(
            error,
            ParseError::NestingTooDeep {
                depth: 513,
                limit: 512,
            }
        );
        match builder.finish() {
            Err(finish_error) => assert_eq!(finish_error, error),
            Ok(_) => panic!("terminal builder unexpectedly returned a document"),
        }
    }

    #[test]
    fn void_element_below_512_open_elements_exceeds_depth_limit() {
        let mut builder = TreeBuilder::new();
        for _ in 0..MAX_DEPTH {
            process(&mut builder, &make_open(Tag::Div));
        }
        assert_eq!(
            builder.process(&make_open(Tag::Br)).unwrap_err(),
            ParseError::NestingTooDeep {
                depth: 513,
                limit: 512,
            }
        );
    }
}
