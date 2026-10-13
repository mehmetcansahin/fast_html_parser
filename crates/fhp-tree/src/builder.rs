//! Tree construction from a token stream.
//!
//! [`TreeBuilder`] consumes [`Token`](fhp_tokenizer::token::Token)s and builds an arena-based DOM tree.
//! It handles implicit close rules, void elements, and common malformed-HTML
//! recovery strategies.

use fhp_core::{attr::iter_raw_attrs, error::ParseError, is_html_whitespace, tag::Tag};
use fhp_tokenizer::token::{Attribute as TokenAttribute, Token};

use crate::arena::Arena;
use crate::node::{NodeFlags, NodeId};

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
    Caption,
    ColGroup,
    Column,
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
            Tag::Col => Self::Column,
            Tag::Select => Self::Select,
            Tag::Plaintext => Self::Plaintext,
            Tag::A | Tag::B | Tag::Em | Tag::I | Tag::Strong => Self::Formatting,
            Tag::Unknown if name.eq_ignore_ascii_case("dt") || name.eq_ignore_ascii_case("dd") => {
                Self::DefinitionItem
            }
            Tag::Unknown if name.eq_ignore_ascii_case("tfoot") => Self::TableBody,
            Tag::Unknown if name.eq_ignore_ascii_case("caption") => Self::Caption,
            Tag::Unknown if name.eq_ignore_ascii_case("colgroup") => Self::ColGroup,
            Tag::Unknown if name.eq_ignore_ascii_case("option") => Self::Option,
            Tag::Unknown if name.eq_ignore_ascii_case("optgroup") => Self::OptGroup,
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
            Self::Caption => InsertionMode::InCaption,
            Self::ColGroup => InsertionMode::InColumnGroup,
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
    InCaption,
    InColumnGroup,
    InSelect,
}

#[derive(Clone, Copy, Debug)]
struct OpenElement {
    id: NodeId,
    tag: Tag,
    kind: ElementKind,
    /// Whether the element is in the HTML "special" category, which bounds
    /// generic end-tag and list-item searches. Cached at push time so stack
    /// walks never compare unknown element names.
    special: bool,
    /// Effective insertion mode for the stack prefix ending at this entry.
    insertion_mode: InsertionMode,
    element_child_count: u32,
}

/// Element sets that end an "has an element in scope" stack walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    /// The default scope: tables, cells, and captions.
    Standard,
    /// Default scope plus `ol` and `ul`.
    ListItem,
    /// Default scope plus `button`.
    Button,
    /// Table scope: only a table ends the walk.
    Table,
}

impl Scope {
    #[inline]
    fn stops_at(self, entry: &OpenElement) -> bool {
        let standard = matches!(
            entry.kind,
            ElementKind::Table | ElementKind::Cell | ElementKind::Caption
        );
        match self {
            Self::Standard => standard,
            Self::ListItem => standard || matches!(entry.tag, Tag::Ol | Tag::Ul),
            Self::Button => standard || entry.tag == Tag::Button,
            Self::Table => entry.kind == ElementKind::Table,
        }
    }
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

impl InsertionLocation {
    #[inline]
    fn parent(self) -> NodeId {
        match self {
            Self::Append(parent) | Self::Before { parent, .. } => parent,
        }
    }
}

/// Whether the tokenizer stays in a literal text mode after this start tag
/// (raw text through the matching end tag, or plaintext through EOF).
#[inline]
fn opens_raw_text(tag: Tag) -> bool {
    tag.is_raw_text() || tag == Tag::Plaintext
}

#[inline]
fn matches_ascii_name(name: &str, expected: &[&str]) -> bool {
    expected
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Whether an element belongs to the HTML "special" category, restricted to
/// the elements this builder can produce. `unknown_name` is the preserved
/// name of a [`Tag::Unknown`] element.
fn is_special_element(tag: Tag, kind: ElementKind, unknown_name: Option<&str>) -> bool {
    matches!(
        kind,
        ElementKind::Table
            | ElementKind::TableBody
            | ElementKind::Row
            | ElementKind::Cell
            | ElementKind::Caption
            | ElementKind::ColGroup
            | ElementKind::Select
            | ElementKind::Heading
            | ElementKind::ListItem
            | ElementKind::DefinitionItem
            | ElementKind::Paragraph
            | ElementKind::Plaintext
    ) || matches!(
        tag,
        Tag::Article
            | Tag::Aside
            | Tag::Body
            | Tag::Button
            | Tag::Div
            | Tag::Footer
            | Tag::Form
            | Tag::Head
            | Tag::Header
            | Tag::Html
            | Tag::Iframe
            | Tag::Main
            | Tag::Nav
            | Tag::Ol
            | Tag::Pre
            | Tag::Script
            | Tag::Section
            | Tag::Style
            | Tag::Textarea
            | Tag::Title
            | Tag::Ul
            | Tag::Xmp
            | Tag::Noembed
            | Tag::Noframes
    ) || (tag == Tag::Unknown
        && unknown_name.is_some_and(|name| {
            matches_ascii_name(
                name,
                &[
                    "address",
                    "applet",
                    "blockquote",
                    "center",
                    "details",
                    "dialog",
                    "dir",
                    "dl",
                    "fieldset",
                    "figcaption",
                    "figure",
                    "frameset",
                    "hgroup",
                    "listing",
                    "marquee",
                    "menu",
                    "noscript",
                    "object",
                    "search",
                    "summary",
                    "template",
                ],
            )
        }))
}

/// Whether the first `type` attribute is ASCII case-insensitively `hidden`
/// (the `<input type=hidden>` exception to table foster parenting).
fn attrs_type_hidden(attributes: &PendingAttrs<'_, '_>) -> bool {
    match attributes {
        PendingAttrs::Parsed(attributes) => attributes
            .iter()
            .find(|attribute| attribute.name.eq_ignore_ascii_case("type"))
            .and_then(|attribute| attribute.value.as_deref())
            .is_some_and(|value| value.eq_ignore_ascii_case("hidden")),
        PendingAttrs::Raw(raw) => iter_raw_attrs(raw)
            .find(|attribute| attribute.name.eq_ignore_ascii_case("type"))
            .and_then(|attribute| attribute.value)
            .is_some_and(|value| {
                // Match the decoded value the token path compares.
                #[cfg(feature = "entity-decode")]
                let value = fhp_tokenizer::entity::decode_attribute_entities(value);
                value.eq_ignore_ascii_case("hidden")
            }),
        PendingAttrs::None => false,
    }
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
            | Tag::Li
            | Tag::Main
            | Tag::Nav
            | Tag::Ol
            | Tag::P
            | Tag::Pre
            | Tag::Section
            | Tag::Table
            | Tag::Ul
            | Tag::Xmp
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
    /// Whether a line feed at the start of the next character token is
    /// dropped (the token right after a `pre`, `listing`, or `textarea`
    /// start tag).
    skip_next_lf: bool,
    /// The synthetic root node (document root).
    root: NodeId,
    /// Base address of the original input (for source-backed text).
    source_base: usize,
    /// Length of the original input in bytes.
    source_len: usize,
    /// Terminal construction error. The infallible `TreeSink` path records it
    /// here and [`TreeBuilder::finish`] surfaces it.
    error: Option<ParseError>,
    /// Text node emitted by the current uninterrupted character-token run.
    /// Non-text tokens clear it so coalescing is independent of storage mode.
    last_text_node: NodeId,
    /// Whitespace-only fragments of a table character-token run. HTML decides
    /// whether to foster the whole run only after a non-space character or the
    /// terminating non-text token is seen.
    pending_table_text: String,
    /// Whether the current table character-token run is known to require
    /// foster parenting.
    table_text_run_foster: bool,
    /// Text node flushed immediately before the most recently processed
    /// non-text token.
    flushed_text_node: Option<NodeId>,
    /// Raw-text start tag (`xmp`, `style`, `plaintext`, ...) the builder
    /// ignored, e.g. inside `select`. The tokenizer still emits that
    /// element's content undecoded, while no raw-text parent exists; the token
    /// path then decodes it exactly as the sink path does.
    ignored_raw_text: Option<Tag>,
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
            special: true,
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
            skip_next_lf: false,
            root,
            source_base: 0,
            source_len: 0,
            error: None,
            last_text_node: NodeId::NULL,
            pending_table_text: String::new(),
            table_text_run_foster: false,
            flushed_text_node: None,
            ignored_raw_text: None,
        }
    }

    /// Enable the inline tag index for O(1) tag lookups after parsing.
    ///
    /// When enabled, each element's tag is indexed during tree construction.
    /// Buckets follow allocation order, which can differ from document order
    /// after foster parenting; see [`Arena::tag_index`].
    pub fn enable_tag_index(&mut self) {
        self.arena.enable_tag_index();
    }

    /// Enable source-backed text nodes.
    ///
    /// Stores an owned copy of `input` in the arena. Text nodes whose content
    /// is borrowed from `input` (entity-free) will reference the source
    /// instead of copying to the text slab.
    pub(crate) fn set_source(&mut self, input: &str) {
        self.source_base = input.as_ptr() as usize;
        self.source_len = input.len();
        self.arena.set_source(input);
    }

    /// Set source pointer tracking without copying data to the arena.
    ///
    /// Use this with [`Arena::set_source_owned`] after tokenization to
    /// avoid a redundant memcpy when the caller owns the input `String`.
    pub(crate) fn set_source_ptr(&mut self, input: &str) {
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
        self.flushed_text_node = None;
        match token {
            Token::OpenTag {
                tag,
                name,
                attributes,
                self_closing,
            } => {
                let created = self.handle_open_tag(
                    *tag,
                    name.as_ref(),
                    PendingAttrs::Parsed(attributes),
                    *self_closing,
                );
                if matches!(created, Ok(None)) && opens_raw_text(*tag) {
                    self.ignored_raw_text = Some(*tag);
                }
                created
            }
            Token::CloseTag { tag, name } => {
                // The tokenizer leaves raw-text mode at the matching end tag.
                if self.ignored_raw_text == Some(*tag) {
                    self.ignored_raw_text = None;
                }
                self.handle_close_tag(*tag, name.as_ref())
            }
            Token::Text { content } if self.ignored_raw_text.is_some() => {
                Ok(self.handle_raw_text(content.as_ref()))
            }
            Token::Text { content } => Ok(self.handle_text(content.as_ref())),
            Token::Comment { content } => Ok(self.handle_comment(content.as_ref())),
            Token::Doctype { content } => Ok(self.handle_doctype(content.as_ref())),
            Token::CData { content } => Ok(self.handle_text(content.as_ref())),
        }
    }

    /// Finish building and return the root node id and arena.
    pub fn finish(mut self) -> Result<(Arena, NodeId), ParseError> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            let _ = self.flush_pending_table_text();
            Ok((self.arena, self.root))
        }
    }

    /// Take a table-whitespace text node that was flushed immediately before
    /// the most recently processed non-text token.
    ///
    /// This is primarily useful to incremental observers that need to inspect
    /// every node creation. Ordinary DOM builders can ignore it.
    #[cfg(feature = "encoding")]
    pub(crate) fn take_flushed_text_node(&mut self) -> Option<NodeId> {
        self.flushed_text_node.take()
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
        let special = is_special_element(tag, kind, self.arena.unknown_tag_name(id));
        self.open_elements.push(OpenElement {
            id,
            tag,
            kind,
            special,
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
        self.end_text_run();
        let kind = ElementKind::classify(tag, name);

        if self.insertion_mode == InsertionMode::InSelect {
            // "In select in table": table-structure starts close the select.
            if is_select_breakout_start(tag, name)
                || (self.kind_is_open(ElementKind::Table) && is_select_in_table_breakout(kind))
            {
                self.close_last_kind(ElementKind::Select);
                return self.handle_open_tag(tag, name, attributes, source_self_closing);
            }
            match kind {
                ElementKind::Option => {
                    self.pop_if_current(ElementKind::Option);
                }
                ElementKind::OptGroup => {
                    self.pop_if_current(ElementKind::Option);
                    self.pop_if_current(ElementKind::OptGroup);
                }
                ElementKind::Select => {
                    self.close_last_kind(ElementKind::Select);
                    return Ok(None);
                }
                // Processed with the "in head" rules: inserted in place.
                _ if tag == Tag::Script => {}
                _ => return Ok(None),
            }
        }

        let mut foster = false;
        match self.insertion_mode {
            InsertionMode::InCell if is_table_section_start(kind) => {
                self.close_last_kind(ElementKind::Cell);
                return self.handle_open_tag(tag, name, attributes, source_self_closing);
            }
            InsertionMode::InCaption if is_table_section_start(kind) => {
                self.close_last_kind(ElementKind::Caption);
                return self.handle_open_tag(tag, name, attributes, source_self_closing);
            }
            InsertionMode::InColumnGroup => {
                if kind == ElementKind::Column {
                    let node = self.insert_element(
                        tag,
                        name,
                        kind,
                        attributes,
                        source_self_closing,
                        InsertionLocation::Append(self.current_parent()),
                    )?;
                    return Ok(Some(node));
                }
                if !self.pop_if_current(ElementKind::ColGroup) {
                    return Ok(None);
                }
                return self.handle_open_tag(tag, name, attributes, source_self_closing);
            }
            InsertionMode::InRow => match kind {
                ElementKind::Cell => self.clear_back_to(ElementKind::Row),
                ElementKind::Row
                | ElementKind::TableBody
                | ElementKind::Table
                | ElementKind::Caption
                | ElementKind::ColGroup
                | ElementKind::Column => {
                    self.close_last_kind(ElementKind::Row);
                    return self.handle_open_tag(tag, name, attributes, source_self_closing);
                }
                _ => foster = true,
            },
            InsertionMode::InTableBody => match kind {
                ElementKind::Row => self.clear_back_to(ElementKind::TableBody),
                ElementKind::Cell => {
                    self.clear_back_to(ElementKind::TableBody);
                    self.insert_implied(Tag::Tr, "tr", ElementKind::Row)?;
                }
                ElementKind::TableBody
                | ElementKind::Table
                | ElementKind::Caption
                | ElementKind::ColGroup
                | ElementKind::Column => {
                    self.close_last_kind(ElementKind::TableBody);
                    return self.handle_open_tag(tag, name, attributes, source_self_closing);
                }
                _ => foster = true,
            },
            InsertionMode::InTable => match kind {
                ElementKind::TableBody | ElementKind::Caption | ElementKind::ColGroup => {
                    self.clear_back_to(ElementKind::Table);
                }
                ElementKind::Row => {
                    self.clear_back_to(ElementKind::Table);
                    self.insert_implied(Tag::Tbody, "tbody", ElementKind::TableBody)?;
                }
                ElementKind::Cell => {
                    self.clear_back_to(ElementKind::Table);
                    self.insert_implied(Tag::Tbody, "tbody", ElementKind::TableBody)?;
                    self.insert_implied(Tag::Tr, "tr", ElementKind::Row)?;
                }
                ElementKind::Column => {
                    self.clear_back_to(ElementKind::Table);
                    self.insert_implied(Tag::Unknown, "colgroup", ElementKind::ColGroup)?;
                }
                ElementKind::Table => self.close_last_kind(ElementKind::Table),
                _ => foster = true,
            },
            // Table parts outside a table are ignored.
            InsertionMode::InBody if is_table_section_start(kind) => return Ok(None),
            _ => {}
        }

        if foster {
            match tag {
                // "In head" elements and hidden inputs stay inside the table.
                Tag::Script | Tag::Style => foster = false,
                Tag::Input if attrs_type_hidden(&attributes) => foster = false,
                Tag::Form => {
                    // A form in a table is inserted empty and closed at once.
                    let node = self.insert_element(
                        tag,
                        name,
                        kind,
                        attributes,
                        source_self_closing,
                        InsertionLocation::Append(self.current_parent()),
                    )?;
                    self.pop_open_element();
                    return Ok(Some(node));
                }
                _ => {}
            }
        }

        if tag == Tag::A {
            // A nested anchor start repairs the previous active anchor before
            // the new element is inserted. Reuse the deliberately simplified
            // adoption path used by formatting end tags.
            self.adoption_close(tag, name);
        }
        self.apply_body_implicit_close(tag, kind);
        // Fostering applies only while the current node is a table part; an
        // element already fostered out of the table receives its content.
        let location = if foster && self.current_node_is_table_part() {
            self.foster_location()
        } else {
            InsertionLocation::Append(self.current_parent())
        };
        let node =
            self.insert_element(tag, name, kind, attributes, source_self_closing, location)?;
        if matches!(tag, Tag::Pre | Tag::Textarea)
            || (tag == Tag::Unknown && name.eq_ignore_ascii_case("listing"))
        {
            self.skip_next_lf = true;
        }
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

        let parent = location.parent();
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

        self.attach_element(node, location);

        let is_void = tag.is_void();
        if source_self_closing || is_void {
            self.arena.set_self_closing(node);
        }
        // A slash on a non-void HTML element is syntactic metadata only.
        if !is_void {
            self.push_open_element(node, tag, kind);
        }
        Ok(node)
    }

    /// Attach a new element at `location`, keeping the element-child indices
    /// and the parent's open-stack child count exact.
    fn attach_element(&mut self, node: NodeId, location: InsertionLocation) {
        match location {
            InsertionLocation::Append(parent) => {
                self.arena.append_child_trusted(parent, node);
                self.register_appended_element(parent, node);
            }
            InsertionLocation::Before { parent, reference } => {
                self.arena.insert_before(parent, reference, node);
                // The new element takes the reference's index and shifts the
                // reference and every later element sibling by one. Fostering
                // targets an open table, which is the last element child of
                // its parent, so this walk is O(1) element siblings.
                let index = self.arena.full_element_index(reference);
                self.arena.set_full_element_index(node, index);
                let mut sibling = reference;
                while !sibling.is_null() {
                    if self.arena.is_element(sibling) {
                        let shifted = self.arena.full_element_index(sibling).saturating_add(1);
                        self.arena.set_full_element_index(sibling, shifted);
                    }
                    sibling = self.arena.get(sibling).next_sibling;
                }
                if let Some(entry) = self
                    .open_elements
                    .iter_mut()
                    .rev()
                    .find(|entry| entry.id == parent)
                {
                    entry.element_child_count = entry.element_child_count.saturating_add(1);
                }
            }
        }
    }

    fn handle_close_tag(&mut self, tag: Tag, name: &str) -> Result<Option<NodeId>, ParseError> {
        self.end_text_run();
        if tag == Tag::Br {
            // `</br>` is parsed as a `<br>` start tag without attributes.
            return self.handle_open_tag(Tag::Br, "br", PendingAttrs::None, false);
        }
        if tag.is_void() {
            return Ok(None);
        }

        // The overwhelmingly common close is the current element. Handle it
        // before classifying the tag or entering table/formatting recovery.
        // All specialized well-formed paths reduce to this same single pop.
        let last_index = self.open_elements.len() - 1;
        if last_index > 0 && self.entry_matches(last_index, tag, name) {
            self.pop_open_element();
            return Ok(None);
        }

        let kind = ElementKind::classify(tag, name);
        if self.insertion_mode == InsertionMode::InSelect {
            // "In select in table": a table end tag closes the select only
            // when an element with that name is in table scope.
            if self.kind_is_open(ElementKind::Table)
                && is_select_in_table_breakout(kind)
                && self.find_named_in_scope(tag, name, Scope::Table).is_some()
            {
                self.close_last_kind(ElementKind::Select);
                return self.handle_close_tag(tag, name);
            }
            match kind {
                ElementKind::Option => self.close_last_kind(ElementKind::Option),
                ElementKind::OptGroup => {
                    self.close_last_kind(ElementKind::Option);
                    self.close_last_kind(ElementKind::OptGroup);
                }
                ElementKind::Select => self.close_last_kind(ElementKind::Select),
                _ => {}
            }
            return Ok(None);
        }

        // "In column group": any other end tag closes the column group and
        // is reprocessed in the table.
        if self.insertion_mode == InsertionMode::InColumnGroup
            && !matches!(kind, ElementKind::ColGroup | ElementKind::Column)
        {
            if self.pop_if_current(ElementKind::ColGroup) {
                return self.handle_close_tag(tag, name);
            }
            return Ok(None);
        }

        match kind {
            ElementKind::Formatting => self.adoption_close(tag, name),
            ElementKind::Cell
            | ElementKind::Row
            | ElementKind::TableBody
            | ElementKind::Table
            | ElementKind::Caption => {
                self.close_in_scope(kind, Scope::Table);
            }
            ElementKind::ColGroup => {
                self.pop_if_current(ElementKind::ColGroup);
            }
            ElementKind::Column => {}
            ElementKind::Paragraph => {
                if !self.close_in_scope(ElementKind::Paragraph, Scope::Button) {
                    // `</p>` without an open paragraph inserts an empty one.
                    let location = if self.current_node_is_table_part() {
                        self.foster_location()
                    } else {
                        InsertionLocation::Append(self.current_parent())
                    };
                    let node = self.insert_element(
                        Tag::P,
                        "p",
                        ElementKind::Paragraph,
                        PendingAttrs::None,
                        false,
                        location,
                    )?;
                    self.pop_open_element();
                    return Ok(Some(node));
                }
            }
            ElementKind::ListItem => {
                self.close_in_scope(ElementKind::ListItem, Scope::ListItem);
            }
            // `</dd>` and `</dt>` close only an element with the same name.
            ElementKind::DefinitionItem => {
                if let Some(index) = self.find_named_in_scope(tag, name, Scope::Standard) {
                    self.truncate_stack(index);
                }
            }
            // Any heading end tag closes the innermost open heading.
            ElementKind::Heading => {
                self.close_in_scope(ElementKind::Heading, Scope::Standard);
            }
            _ => self.close_matching(tag, name, kind),
        }
        Ok(None)
    }

    /// Close an element by name. Ordinary end tags stop at the first special
    /// element; end tags of special elements (`div`, `ul`, `section`, ...)
    /// close through anything except a table, cell, or caption boundary.
    fn close_matching(&mut self, tag: Tag, name: &str, kind: ElementKind) {
        if !self.kind_is_open(kind) {
            return;
        }
        let closes_special = is_special_element(tag, kind, Some(name));
        for index in (1..self.open_elements.len()).rev() {
            if self.entry_matches(index, tag, name) {
                self.truncate_stack(index);
                return;
            }
            let entry = &self.open_elements[index];
            let stops = if closes_special {
                Scope::Standard.stops_at(entry)
            } else {
                entry.special
            };
            if stops {
                return;
            }
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

    /// Pop everything above the innermost open element of `kind` ("clear the
    /// stack back to a table context"). No-op when none is open.
    fn clear_back_to(&mut self, kind: ElementKind) {
        if !self.kind_is_open(kind) {
            return;
        }
        if let Some(index) = self
            .open_elements
            .iter()
            .rposition(|entry| entry.kind == kind)
        {
            self.truncate_stack(index + 1);
        }
    }

    /// Pop the current node if it is of `kind`. Returns whether it popped.
    fn pop_if_current(&mut self, kind: ElementKind) -> bool {
        let is_current = self.open_elements.len() > 1
            && self
                .open_elements
                .last()
                .is_some_and(|entry| entry.kind == kind);
        if is_current {
            self.pop_open_element();
        }
        is_current
    }

    /// Stack index of the innermost open element of `kind`, unless an element
    /// that bounds `scope` is reached first.
    fn find_in_scope(&self, kind: ElementKind, scope: Scope) -> Option<usize> {
        if !self.kind_is_open(kind) {
            return None;
        }
        for index in (1..self.open_elements.len()).rev() {
            let entry = &self.open_elements[index];
            if entry.kind == kind {
                return Some(index);
            }
            if scope.stops_at(entry) {
                return None;
            }
        }
        None
    }

    /// Stack index of the innermost open element named `tag`/`name`, unless
    /// an element that bounds `scope` is reached first.
    fn find_named_in_scope(&self, tag: Tag, name: &str, scope: Scope) -> Option<usize> {
        for index in (1..self.open_elements.len()).rev() {
            if self.entry_matches(index, tag, name) {
                return Some(index);
            }
            if scope.stops_at(&self.open_elements[index]) {
                return None;
            }
        }
        None
    }

    /// Close the innermost open element of `kind` if it is in `scope`.
    /// Returns whether an element was closed.
    fn close_in_scope(&mut self, kind: ElementKind, scope: Scope) -> bool {
        match self.find_in_scope(kind, scope) {
            Some(index) => {
                self.truncate_stack(index);
                true
            }
            None => false,
        }
    }

    /// The `li`/`dd`/`dt` start-tag loop: close an open item of `kind` unless
    /// a special element other than `address`, `div`, or `p` is reached first.
    fn close_list_item_start(&mut self, kind: ElementKind) {
        if !self.kind_is_open(kind) {
            return;
        }
        for index in (1..self.open_elements.len()).rev() {
            let entry = self.open_elements[index];
            if entry.kind == kind {
                self.truncate_stack(index);
                return;
            }
            let transparent = entry.tag == Tag::Div
                || entry.kind == ElementKind::Paragraph
                || self
                    .arena
                    .unknown_tag_name(entry.id)
                    .is_some_and(|name| name.eq_ignore_ascii_case("address"));
            if entry.special && !transparent {
                return;
            }
        }
    }

    /// Whether the current node is a table, table section, or row, where
    /// non-table content is foster-parented.
    #[inline]
    fn current_node_is_table_part(&self) -> bool {
        self.open_elements.last().is_some_and(|entry| {
            matches!(
                entry.kind,
                ElementKind::Table | ElementKind::TableBody | ElementKind::Row
            )
        })
    }

    fn adoption_close(&mut self, tag: Tag, name: &str) {
        if !self.kind_is_open(ElementKind::Formatting) {
            return;
        }
        // Table, cell, and caption boundaries act as formatting markers: a
        // formatting element outside them is not repaired from inside.
        let mut found = None;
        for index in (1..self.open_elements.len()).rev() {
            if self.entry_matches(index, tag, name) {
                found = Some(index);
                break;
            }
            if Scope::Standard.stops_at(&self.open_elements[index]) {
                return;
            }
        }
        let Some(index) = found else {
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
            // Like any other insertion, a clone made while a table part is the
            // current node is foster-parented.
            let location = if self.current_node_is_table_part() {
                self.foster_location()
            } else {
                InsertionLocation::Append(self.current_parent())
            };
            let depth = self.arena.get(location.parent()).depth.saturating_add(1);
            let node = self.arena.clone_element_shallow(old.id, depth);
            self.attach_element(node, location);
            self.push_open_element(node, old.tag, old.kind);
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
        }
        self.open_elements.truncate(len);
        self.insertion_mode = self
            .open_elements
            .last()
            .expect("synthetic root is never removed")
            .insertion_mode;
    }

    fn apply_body_implicit_close(&mut self, tag: Tag, kind: ElementKind) {
        match kind {
            ElementKind::ListItem | ElementKind::DefinitionItem => {
                self.close_list_item_start(kind);
            }
            _ => {}
        }
        if self.kind_is_open(ElementKind::Paragraph)
            && (closes_p_element(tag)
                || matches!(
                    kind,
                    ElementKind::Paragraph
                        | ElementKind::ListItem
                        | ElementKind::DefinitionItem
                        | ElementKind::Plaintext
                ))
        {
            self.close_in_scope(ElementKind::Paragraph, Scope::Button);
        }
        match kind {
            ElementKind::Heading => {
                self.pop_if_current(ElementKind::Heading);
            }
            // In body, `option` and `optgroup` starts only close a current
            // `option`.
            ElementKind::Option | ElementKind::OptGroup => {
                self.pop_if_current(ElementKind::Option);
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
    fn handle_text(&mut self, mut content: &str) -> Option<NodeId> {
        if content.is_empty() {
            return None;
        }
        if self.skip_next_lf {
            self.skip_next_lf = false;
            if let Some(rest) = content.strip_prefix('\n') {
                if rest.is_empty() {
                    return None;
                }
                content = rest;
            }
        }

        let mut inserted = None;
        if self.insertion_mode == InsertionMode::InColumnGroup {
            // Whitespace belongs to the column group; the first other
            // character closes it and continues with the table rules.
            let Some(split) = content.bytes().position(|byte| !is_html_whitespace(byte)) else {
                return self.insert_text(content, false);
            };
            if split > 0 {
                inserted = self.insert_text(&content[..split], false);
                content = &content[split..];
            }
            if !self.pop_if_current(ElementKind::ColGroup) {
                return self.insert_text(content, false).or(inserted);
            }
        }

        if self.current_node_is_table_part() {
            if self.table_text_run_foster {
                return self.insert_text(content, true);
            }
            if content.bytes().all(is_html_whitespace) {
                self.pending_table_text.push_str(content);
                return inserted;
            }

            self.table_text_run_foster = true;
            if !self.pending_table_text.is_empty() {
                let mut run = std::mem::take(&mut self.pending_table_text);
                run.push_str(content);
                return self.insert_text(&run, true);
            }
            return self.insert_text(content, true);
        }

        self.insert_text(content, false)
    }

    fn insert_text(&mut self, content: &str, foster: bool) -> Option<NodeId> {
        let location = if foster {
            self.foster_location()
        } else {
            InsertionLocation::Append(self.current_parent())
        };
        let parent = match location {
            InsertionLocation::Append(parent) => parent,
            InsertionLocation::Before { parent, .. } => parent,
        };
        if let Some(node) = self.coalesce_text(location, content) {
            self.last_text_node = node;
            return Some(node);
        }
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.try_source_ref(depth, content);
        self.insert_non_element(location, node);
        self.last_text_node = node;
        Some(node)
    }

    fn flush_pending_table_text(&mut self) -> Option<NodeId> {
        if self.pending_table_text.is_empty() {
            return None;
        }
        let content = std::mem::take(&mut self.pending_table_text);
        self.insert_text(&content, false)
    }

    fn end_text_run(&mut self) {
        if let Some(node) = self.flush_pending_table_text() {
            self.flushed_text_node = Some(node);
        }
        self.last_text_node = NodeId::NULL;
        self.table_text_run_foster = false;
        self.skip_next_lf = false;
    }

    #[cfg(feature = "encoding")]
    /// End the current logical character-token run and return its stable text
    /// node, including table whitespace that only becomes insertable now.
    pub(crate) fn finish_observed_text_run(&mut self) -> Option<NodeId> {
        self.flushed_text_node = None;
        let stable_text = (!self.last_text_node.is_null()).then_some(self.last_text_node);
        self.end_text_run();
        self.take_flushed_text_node().or(stable_text)
    }

    /// Handle raw text from TreeSink (not entity-decoded).
    fn handle_raw_text(&mut self, raw: &str) -> Option<NodeId> {
        if raw.is_empty() {
            return None;
        }

        #[cfg(feature = "entity-decode")]
        {
            let parent_tag = self.arena.get(self.current_parent()).tag;
            if parent_tag.is_raw_text() || parent_tag == Tag::Plaintext {
                self.handle_text(raw)
            } else {
                let decoded = fhp_tokenizer::entity::decode_entities(raw);
                self.handle_text(decoded.as_ref())
            }
        }

        #[cfg(not(feature = "entity-decode"))]
        {
            self.handle_text(raw)
        }
    }

    /// Append to an immediately preceding text sibling when its storage can be
    /// extended safely. Streaming token boundaries are an implementation
    /// detail and must not change the DOM's text-node cardinality.
    fn coalesce_text(&mut self, location: InsertionLocation, content: &str) -> Option<NodeId> {
        let previous = match location {
            InsertionLocation::Append(parent) => self.arena.get(parent).last_child,
            InsertionLocation::Before { reference, .. } => self.arena.get(reference).prev_sibling,
        };
        if previous.is_null() || !self.arena.get(previous).flags.has(NodeFlags::IS_TEXT) {
            return None;
        }
        if previous != self.last_text_node {
            return None;
        }

        let previous_node = self.arena.get(previous);
        let old_offset = previous_node.text_offset as usize;
        let old_len = previous_node.text_len as usize;
        let old_end = old_offset.checked_add(old_len)?;
        let new_len = old_len.checked_add(content.len())?;
        let new_len = u32::try_from(new_len).ok()?;

        if previous_node.flags.has(NodeFlags::IS_TEXT_FROM_SOURCE) {
            let ptr = content.as_ptr() as usize;
            let source_end = self.source_base.checked_add(self.source_len)?;
            let contiguous = self.source_len != 0
                && ptr >= self.source_base
                && ptr.checked_add(content.len())? <= source_end
                && old_end == ptr - self.source_base;
            if contiguous {
                self.arena.get_mut(previous).text_len = new_len;
                return Some(previous);
            }

            // A decoded fragment no longer points into the original source.
            // Promote the preceding source-backed node into the slab once;
            // subsequent fragments can then use the tail-append path below.
            let new_offset = self
                .arena
                .promote_source_text(old_offset, old_len, content)?;
            let previous_node = self.arena.get_mut(previous);
            previous_node.text_offset = new_offset;
            previous_node.text_len = new_len;
            previous_node.flags.clear(NodeFlags::IS_TEXT_FROM_SOURCE);
            return Some(previous);
        }

        // Relocating a non-tail string on every foster-parented character run
        // would make adversarial table input quadratic. Streaming fragments of
        // one character run are tail-contiguous, which is the case that needs
        // coalescing for chunk-invariant DOM construction.
        if old_end != self.arena.text_slab.len() {
            return None;
        }
        self.arena.text_slab.extend_from_slice(content.as_bytes());
        self.arena.get_mut(previous).text_len = new_len;
        Some(previous)
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
        self.end_text_run();
        let parent = self.current_parent();
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.arena.new_comment(depth, content);
        self.arena.append_child_trusted(parent, node);
        Some(node)
    }

    /// Handle a doctype declaration.
    fn handle_doctype(&mut self, content: &str) -> Option<NodeId> {
        self.end_text_run();
        let parent = self.current_parent();
        let depth = self.arena.get(parent).depth.saturating_add(1);
        let node = self.arena.new_doctype(depth, content);
        self.arena.append_child_trusted(parent, node);
        Some(node)
    }
}

/// Kinds that start a table section, row, cell, caption, or column. They
/// close an open cell or caption and are ignored outside a table.
#[inline]
fn is_table_section_start(kind: ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::TableBody
            | ElementKind::Row
            | ElementKind::Cell
            | ElementKind::Caption
            | ElementKind::ColGroup
            | ElementKind::Column
    )
}

/// Table-structure tags that close a `select` inside a table ("in select in
/// table").
#[inline]
fn is_select_in_table_breakout(kind: ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Table
            | ElementKind::TableBody
            | ElementKind::Row
            | ElementKind::Cell
            | ElementKind::Caption
    )
}

#[inline]
fn is_select_breakout_start(tag: Tag, name: &str) -> bool {
    matches!(tag, Tag::Input | Tag::Textarea)
        || (tag == Tag::Unknown && name.eq_ignore_ascii_case("keygen"))
}

impl fhp_tokenizer::TreeSink for TreeBuilder {
    fn should_continue(&self) -> bool {
        self.error.is_none()
    }

    fn open_tag(&mut self, tag: Tag, name: &str, attr_raw: &str, self_closing: bool) {
        if self.error.is_none() {
            let _ = self.handle_open_tag(tag, name, PendingAttrs::Raw(attr_raw), self_closing);
        }
    }

    fn close_tag(&mut self, tag: Tag, name: &str) {
        if self.error.is_none() {
            let _ = self.handle_close_tag(tag, name);
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
        // CDATA content is literal text, exactly like `Token::CData`.
        if self.error.is_none() {
            self.handle_text(content);
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
    fn vertical_tab_is_foster_parented_as_non_html_whitespace() {
        let (arena, root) = parse("<table>\x0B<tr><td>x</td></tr></table>").unwrap();
        let text = arena.get(root).first_child;
        assert!(arena.get(text).flags.has(NodeFlags::IS_TEXT));
        assert_eq!(arena.text(text), "\x0B");
        assert_eq!(arena.get(arena.get(text).next_sibling).tag, Tag::Table);
    }

    #[test]
    fn adjacent_character_tokens_share_one_text_node() {
        let mut builder = TreeBuilder::new();
        process(&mut builder, &make_text("first"));
        let node = process(&mut builder, &make_text("second")).unwrap();
        let (arena, root) = finish(builder);

        assert_eq!(arena.text(node), "firstsecond");
        assert_eq!(arena.get(root).first_child, node);
        assert_eq!(arena.get(node).next_sibling, NodeId::NULL);
    }

    #[test]
    fn source_backed_text_coalesces_with_owned_text() {
        let source = String::from("first");
        let mut builder = TreeBuilder::new();
        builder.set_source(&source);
        let first = Token::Text {
            content: Cow::Borrowed(&source),
        };
        process(&mut builder, &first);
        let second = Token::Text {
            content: Cow::Owned(String::from("second")),
        };
        let node = process(&mut builder, &second).unwrap();
        let (arena, root) = finish(builder);

        assert_eq!(arena.text(node), "firstsecond");
        assert!(!arena.get(node).flags.has(NodeFlags::IS_TEXT_FROM_SOURCE));
        assert_eq!(arena.get(root).first_child, node);
        assert_eq!(arena.get(node).next_sibling, NodeId::NULL);
    }

    #[test]
    fn zero_copy_source_text_resolves_promoted_ranges() {
        let source = String::from("first");
        let mut builder = TreeBuilder::new();
        builder.set_source_ptr(&source);
        let first = Token::Text {
            content: Cow::Borrowed(&source),
        };
        process(&mut builder, &first);
        let second = Token::Text {
            content: Cow::Owned(String::from("second")),
        };
        let node = process(&mut builder, &second).unwrap();
        let (mut arena, root) = builder.finish().unwrap();
        arena.set_source_owned(source);

        assert_eq!(arena.text(node), "firstsecond");
        assert!(!arena.get(node).flags.has(NodeFlags::IS_TEXT_FROM_SOURCE));
        assert_eq!(arena.get(root).first_child, node);
        assert_eq!(arena.get(node).next_sibling, NodeId::NULL);
    }

    #[test]
    #[should_panic(expected = "source-backed text promotion is not finalized")]
    fn unresolved_zero_copy_promotion_cannot_be_read() {
        let source = String::from("first");
        let mut builder = TreeBuilder::new();
        builder.set_source_ptr(&source);
        let first = Token::Text {
            content: Cow::Borrowed(&source),
        };
        process(&mut builder, &first);
        let second = Token::Text {
            content: Cow::Owned(String::from("second")),
        };
        let node = process(&mut builder, &second).unwrap();
        let (arena, _) = builder.finish().unwrap();

        let _ = arena.text(node);
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
    fn generic_truncation_restores_the_stack() {
        let mut builder = TreeBuilder::new();
        for _ in 0..2_048 {
            process(&mut builder, &make_open(Tag::Div));
            process(&mut builder, &make_open(Tag::B));
            process(&mut builder, &make_close(Tag::Div));
            assert_eq!(builder.open_elements.len(), 1);
            assert_stack_invariants(&builder);
        }
    }

    #[test]
    fn well_formed_formatting_close_creates_no_nodes() {
        let mut builder = TreeBuilder::new();
        process(&mut builder, &make_open(Tag::B));
        let node_count = builder.arena.nodes.len();
        process(&mut builder, &make_close(Tag::B));

        assert_eq!(builder.arena.nodes.len(), node_count);
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
    fn caption_and_column_group_modes_restore_from_stack_prefix() {
        let html = "<table><caption>c<b>d</caption><colgroup><col> <col>x\
                    <tr><td><table><caption>e</table>f</td></tr></table>";
        let mut builder = TreeBuilder::new();
        let mut seen = Vec::new();
        for token in fhp_tokenizer::tokenize(html) {
            process(&mut builder, &token);
            assert_stack_invariants(&builder);
            seen.push(builder.insertion_mode);
        }
        assert!(seen.contains(&InsertionMode::InCaption));
        assert!(seen.contains(&InsertionMode::InColumnGroup));
        assert_eq!(builder.insertion_mode, InsertionMode::InBody);
        assert_eq!(builder.open_elements.len(), 1);
    }

    #[test]
    fn table_end_tags_never_cross_an_inner_table() {
        let (arena, root) =
            parse("<table><tr><td><table></tr></td>a</table>b</td></tr></table>").unwrap();
        let outer = arena.get(root).first_child;
        let tbody = arena.get(outer).first_child;
        let tr = arena.get(tbody).first_child;
        let td = arena.get(tr).first_child;
        assert_eq!(arena.get(td).tag, Tag::Td);

        // The stray `</tr>` and `</td>` belong to the inner table's scope and
        // are ignored; its text is fostered before it, inside the cell.
        let fostered = arena.get(td).first_child;
        let inner = arena.get(fostered).next_sibling;
        let after = arena.get(inner).next_sibling;
        assert_eq!(arena.text(fostered), "a");
        assert_eq!(arena.get(inner).tag, Tag::Table);
        assert!(arena.get(inner).first_child.is_null());
        assert_eq!(arena.text(after), "b");
        assert!(arena.get(after).next_sibling.is_null());
    }

    /// Compact markup outline of the children of `node` (no attributes).
    fn outline(arena: &Arena, node: NodeId) -> String {
        let mut out = String::new();
        let mut child = arena.get(node).first_child;
        while !child.is_null() {
            let flags = arena.get(child).flags;
            if flags.has(NodeFlags::IS_TEXT) {
                out.push_str(arena.text(child));
            } else if !flags.has(NodeFlags::IS_COMMENT) && !flags.has(NodeFlags::IS_DOCTYPE) {
                let tag = arena.get(child).tag;
                let name = tag
                    .as_str()
                    .or_else(|| arena.unknown_tag_name(child))
                    .unwrap_or("?");
                out.push('<');
                out.push_str(name);
                out.push('>');
                if !tag.is_void() {
                    out.push_str(&outline(arena, child));
                    out.push_str("</");
                    out.push_str(name);
                    out.push('>');
                }
            }
            child = arena.get(child).next_sibling;
        }
        out
    }

    fn parse_outline(html: &str) -> String {
        let (arena, root) = parse(html).unwrap();
        outline(&arena, root)
    }

    #[test]
    fn special_end_tags_close_through_paragraphs_and_list_items() {
        assert_eq!(parse_outline("<div><p>a</div>b"), "<div><p>a</p></div>b");
        assert_eq!(parse_outline("<ul><li>a</ul>b"), "<ul><li>a</li></ul>b");
        assert_eq!(
            parse_outline("<button><p>a</button>b"),
            "<button><p>a</p></button>b"
        );
    }

    #[test]
    fn ordinary_end_tags_stop_at_special_elements() {
        assert_eq!(
            parse_outline("<div><span><p>a</span>b</div>"),
            "<div><span><p>ab</p></span></div>"
        );
    }

    #[test]
    fn paragraph_end_tag_in_a_table_fosters_the_empty_paragraph() {
        assert_eq!(
            parse_outline("<table></p><tr><td>x</td></tr></table>"),
            "<p></p><table><tbody><tr><td>x</td></tr></tbody></table>"
        );
    }

    #[test]
    fn column_group_keeps_leading_whitespace_and_fosters_other_text() {
        assert_eq!(
            parse_outline("<table><colgroup>  x <col></table>"),
            "x <table><colgroup>  </colgroup><colgroup><col></colgroup></table>"
        );
    }

    #[cfg(feature = "entity-decode")]
    #[test]
    fn hidden_input_type_is_compared_after_entity_decoding() {
        assert_eq!(
            parse_outline("<table><input type=&#104;idden><tr><td>x</td></tr></table>"),
            "<table><input><tbody><tr><td>x</td></tr></tbody></table>"
        );
    }

    #[test]
    fn select_ignores_style_and_body_optgroup_only_closes_an_option() {
        assert_eq!(
            parse_outline("<select><style>x</style></select>"),
            "<select>x</select>"
        );
        assert_eq!(
            parse_outline("<optgroup>a<optgroup>b"),
            "<optgroup>a<optgroup>b</optgroup></optgroup>"
        );
    }

    #[test]
    fn plaintext_keeps_markup_literal() {
        let (arena, root) = parse("<plaintext><b>x&amp;").unwrap();
        let plaintext = arena.get(root).first_child;
        assert_eq!(arena.get(plaintext).tag, Tag::Plaintext);
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
        assert!(!fhp_tokenizer::TreeSink::should_continue(&builder));
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
