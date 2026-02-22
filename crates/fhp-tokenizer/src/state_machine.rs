//! Branchless state machine for HTML token extraction.
//!
//! The core idea: a 2D lookup table `STATE_TABLE[state][byte_class]` maps
//! every (state, input-byte-class) pair to a `(new_state, action)` without
//! any conditional branches. This eliminates branch misprediction costs
//! that dominate traditional HTML tokenizers.

/// Tokenizer states — models the HTML5 tokenizer states relevant for
/// our structural-index-driven approach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    /// Outside any tag — consuming text content.
    Data = 0,
    /// Saw `<` — deciding if open tag, close tag, comment, or doctype.
    TagOpen,
    /// Inside an open tag name (e.g. reading `div` in `<div`).
    TagName,
    /// Saw `</` — expecting a close tag name.
    EndTagOpen,
    /// Inside a close tag name.
    EndTagName,
    /// After tag name, before attribute name or `>`.
    BeforeAttrName,
    /// Inside an attribute name.
    AttrName,
    /// After attribute name, before `=` or next attribute.
    AfterAttrName,
    /// Saw `=` after attribute name — expecting value.
    BeforeAttrValue,
    /// Inside a quoted attribute value.
    AttrValueQuoted,
    /// Inside an unquoted attribute value.
    AttrValueUnquoted,
    /// Saw `/` inside a tag — expecting `>` for self-closing.
    SelfClosingStartTag,
    /// Inside `<!` — detecting comment vs doctype vs CDATA.
    MarkupDecl,
    /// Inside `<!--` comment body.
    Comment,
    /// Saw first `-` at end of comment (`-`).
    CommentEndDash,
    /// Saw `--` at end of comment.
    CommentEnd,
    /// Inside `<!DOCTYPE` content.
    Doctype,
    /// Inside `<![CDATA[` content.
    CData,
    /// Inside raw text elements (`<script>`, `<style>`).
    RawText,
}

/// Number of states — used for table dimensions.
pub const STATE_COUNT: usize = 19;

/// Byte classification — maps raw bytes to a small enum for table indexing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ByteClass {
    /// `<`
    Lt = 0,
    /// `>`
    Gt,
    /// `/`
    Slash,
    /// `=`
    Eq,
    /// `"` or `'`
    Quot,
    /// `&`
    Amp,
    /// `!`
    Bang,
    /// `-`
    Dash,
    /// `a-z`, `A-Z`
    Alpha,
    /// Space, tab, newline, carriage return
    Whitespace,
    /// Everything else
    Other,
}

/// Number of byte classes — used for table dimensions.
pub const BYTE_CLASS_COUNT: usize = 11;

impl ByteClass {
    /// Classify a raw byte into its [`ByteClass`].
    #[inline(always)]
    pub fn from_byte(b: u8) -> Self {
        match b {
            b'<' => ByteClass::Lt,
            b'>' => ByteClass::Gt,
            b'/' => ByteClass::Slash,
            b'=' => ByteClass::Eq,
            b'"' | b'\'' => ByteClass::Quot,
            b'&' => ByteClass::Amp,
            b'!' => ByteClass::Bang,
            b'-' => ByteClass::Dash,
            b'a'..=b'z' | b'A'..=b'Z' => ByteClass::Alpha,
            b' ' | b'\t' | b'\n' | b'\r' => ByteClass::Whitespace,
            _ => ByteClass::Other,
        }
    }
}

/// Actions to perform during state transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Action {
    /// Do nothing.
    None = 0,
    /// Flush accumulated text as a Text token.
    FlushText,
    /// Begin recording an open tag name.
    StartTag,
    /// Begin recording a close tag name.
    StartEndTag,
    /// Emit the open tag (tag name is complete).
    EmitTagName,
    /// Emit the close tag name.
    EmitEndTagName,
    /// Begin recording an attribute name.
    StartAttrName,
    /// Attribute name is complete.
    EmitAttrName,
    /// Begin recording attribute value.
    StartAttrValue,
    /// Emit attribute value (quoted attribute complete).
    EmitAttrValue,
    /// Emit self-closing tag.
    EmitSelfClose,
    /// Begin comment recording.
    StartComment,
    /// Emit comment token.
    EmitComment,
    /// Begin doctype recording.
    StartDoctype,
    /// Emit doctype token.
    EmitDoctype,
    /// Begin CDATA recording.
    StartCData,
    /// Emit CDATA token.
    EmitCData,
    /// Enter raw text mode (script/style).
    EnterRawText,
    /// Emit open tag and close it (for `>`).
    EmitOpenTagClose,
}

/// A state transition entry: new state + action to perform.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    /// The next state.
    pub state: State,
    /// The action to perform.
    pub action: Action,
}

impl Transition {
    /// No-op transition: stay in current state, do nothing.
    const fn noop(state: State) -> Self {
        Self {
            state,
            action: Action::None,
        }
    }

    /// Transition to a new state with an action.
    const fn new(state: State, action: Action) -> Self {
        Self { state, action }
    }
}

/// The master state transition table.
///
/// `STATE_TABLE[state][byte_class]` yields the `Transition` to apply.
/// Const-initialized at compile time — no runtime cost.
pub static STATE_TABLE: [[Transition; BYTE_CLASS_COUNT]; STATE_COUNT] = build_state_table();

/// Build the state transition table at compile time.
const fn build_state_table() -> [[Transition; BYTE_CLASS_COUNT]; STATE_COUNT] {
    // Default: stay in same state, do nothing. We'll fill per-state below.
    // Can't use Default in const, so manually init.
    let noop = Transition::noop(State::Data);
    let mut table = [[noop; BYTE_CLASS_COUNT]; STATE_COUNT];

    // ----- Data state -----
    // Default: stay in Data
    table[State::Data as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::FlushText);
    table[State::Data as usize][ByteClass::Gt as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Slash as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Eq as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Quot as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Amp as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Bang as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Dash as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Alpha as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Whitespace as usize] = Transition::noop(State::Data);
    table[State::Data as usize][ByteClass::Other as usize] = Transition::noop(State::Data);

    // ----- TagOpen state (saw '<') -----
    table[State::TagOpen as usize][ByteClass::Alpha as usize] =
        Transition::new(State::TagName, Action::StartTag);
    table[State::TagOpen as usize][ByteClass::Slash as usize] =
        Transition::new(State::EndTagOpen, Action::None);
    table[State::TagOpen as usize][ByteClass::Bang as usize] =
        Transition::new(State::MarkupDecl, Action::None);
    // Malformed: '<' followed by non-alpha — treat as text
    table[State::TagOpen as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::FlushText);
    table[State::TagOpen as usize][ByteClass::Gt as usize] = Transition::noop(State::Data);
    table[State::TagOpen as usize][ByteClass::Other as usize] = Transition::noop(State::Data);
    table[State::TagOpen as usize][ByteClass::Whitespace as usize] = Transition::noop(State::Data);
    table[State::TagOpen as usize][ByteClass::Eq as usize] = Transition::noop(State::Data);
    table[State::TagOpen as usize][ByteClass::Quot as usize] = Transition::noop(State::Data);
    table[State::TagOpen as usize][ByteClass::Amp as usize] = Transition::noop(State::Data);
    table[State::TagOpen as usize][ByteClass::Dash as usize] = Transition::noop(State::Data);

    // ----- TagName state -----
    table[State::TagName as usize][ByteClass::Alpha as usize] = Transition::noop(State::TagName);
    table[State::TagName as usize][ByteClass::Other as usize] = Transition::noop(State::TagName);
    table[State::TagName as usize][ByteClass::Dash as usize] = Transition::noop(State::TagName);
    table[State::TagName as usize][ByteClass::Whitespace as usize] =
        Transition::new(State::BeforeAttrName, Action::EmitTagName);
    table[State::TagName as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitOpenTagClose);
    table[State::TagName as usize][ByteClass::Slash as usize] =
        Transition::new(State::SelfClosingStartTag, Action::EmitTagName);
    table[State::TagName as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitOpenTagClose);
    table[State::TagName as usize][ByteClass::Eq as usize] = Transition::noop(State::TagName);
    table[State::TagName as usize][ByteClass::Quot as usize] = Transition::noop(State::TagName);
    table[State::TagName as usize][ByteClass::Amp as usize] = Transition::noop(State::TagName);
    table[State::TagName as usize][ByteClass::Bang as usize] = Transition::noop(State::TagName);

    // ----- EndTagOpen state (saw '</') -----
    table[State::EndTagOpen as usize][ByteClass::Alpha as usize] =
        Transition::new(State::EndTagName, Action::StartEndTag);
    table[State::EndTagOpen as usize][ByteClass::Gt as usize] = Transition::noop(State::Data); // </> — malformed, ignore
    table[State::EndTagOpen as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::FlushText);
    table[State::EndTagOpen as usize][ByteClass::Other as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Slash as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Eq as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Quot as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Amp as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Bang as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Dash as usize] = Transition::noop(State::Data);
    table[State::EndTagOpen as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::Data);

    // ----- EndTagName state -----
    table[State::EndTagName as usize][ByteClass::Alpha as usize] =
        Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Other as usize] =
        Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Dash as usize] =
        Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitEndTagName);
    table[State::EndTagName as usize][ByteClass::Whitespace as usize] =
        Transition::new(State::EndTagName, Action::EmitEndTagName);
    table[State::EndTagName as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitEndTagName);
    table[State::EndTagName as usize][ByteClass::Slash as usize] =
        Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Eq as usize] = Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Quot as usize] =
        Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Amp as usize] =
        Transition::noop(State::EndTagName);
    table[State::EndTagName as usize][ByteClass::Bang as usize] =
        Transition::noop(State::EndTagName);

    // ----- BeforeAttrName state (after tag name whitespace) -----
    table[State::BeforeAttrName as usize][ByteClass::Alpha as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitOpenTagClose);
    table[State::BeforeAttrName as usize][ByteClass::Slash as usize] =
        Transition::new(State::SelfClosingStartTag, Action::None);
    table[State::BeforeAttrName as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Other as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Dash as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitOpenTagClose);
    table[State::BeforeAttrName as usize][ByteClass::Eq as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Quot as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Amp as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::BeforeAttrName as usize][ByteClass::Bang as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);

    // ----- AttrName state -----
    table[State::AttrName as usize][ByteClass::Alpha as usize] = Transition::noop(State::AttrName);
    table[State::AttrName as usize][ByteClass::Other as usize] = Transition::noop(State::AttrName);
    table[State::AttrName as usize][ByteClass::Dash as usize] = Transition::noop(State::AttrName);
    table[State::AttrName as usize][ByteClass::Eq as usize] =
        Transition::new(State::BeforeAttrValue, Action::EmitAttrName);
    table[State::AttrName as usize][ByteClass::Whitespace as usize] =
        Transition::new(State::AfterAttrName, Action::EmitAttrName);
    table[State::AttrName as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitOpenTagClose);
    table[State::AttrName as usize][ByteClass::Slash as usize] =
        Transition::new(State::SelfClosingStartTag, Action::EmitAttrName);
    table[State::AttrName as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitOpenTagClose);
    table[State::AttrName as usize][ByteClass::Quot as usize] = Transition::noop(State::AttrName);
    table[State::AttrName as usize][ByteClass::Amp as usize] = Transition::noop(State::AttrName);
    table[State::AttrName as usize][ByteClass::Bang as usize] = Transition::noop(State::AttrName);

    // ----- AfterAttrName state (after attr name, looking for = or next attr) -----
    table[State::AfterAttrName as usize][ByteClass::Eq as usize] =
        Transition::new(State::BeforeAttrValue, Action::None);
    table[State::AfterAttrName as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::AfterAttrName);
    table[State::AfterAttrName as usize][ByteClass::Alpha as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::AfterAttrName as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitOpenTagClose);
    table[State::AfterAttrName as usize][ByteClass::Slash as usize] =
        Transition::new(State::SelfClosingStartTag, Action::None);
    table[State::AfterAttrName as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitOpenTagClose);
    table[State::AfterAttrName as usize][ByteClass::Other as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::AfterAttrName as usize][ByteClass::Dash as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::AfterAttrName as usize][ByteClass::Quot as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::AfterAttrName as usize][ByteClass::Amp as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::AfterAttrName as usize][ByteClass::Bang as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);

    // ----- BeforeAttrValue state (saw '=', expecting quote or unquoted) -----
    table[State::BeforeAttrValue as usize][ByteClass::Quot as usize] =
        Transition::new(State::AttrValueQuoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::BeforeAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Alpha as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Other as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitOpenTagClose);
    table[State::BeforeAttrValue as usize][ByteClass::Dash as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitOpenTagClose);
    table[State::BeforeAttrValue as usize][ByteClass::Slash as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Eq as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Amp as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);
    table[State::BeforeAttrValue as usize][ByteClass::Bang as usize] =
        Transition::new(State::AttrValueUnquoted, Action::StartAttrValue);

    // ----- AttrValueQuoted state -----
    // Quotes end the value; everything else stays in quoted value.
    table[State::AttrValueQuoted as usize][ByteClass::Quot as usize] =
        Transition::new(State::BeforeAttrName, Action::EmitAttrValue);
    table[State::AttrValueQuoted as usize][ByteClass::Lt as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Gt as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Slash as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Eq as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Amp as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Bang as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Dash as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Alpha as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::AttrValueQuoted);
    table[State::AttrValueQuoted as usize][ByteClass::Other as usize] =
        Transition::noop(State::AttrValueQuoted);

    // ----- AttrValueUnquoted state -----
    table[State::AttrValueUnquoted as usize][ByteClass::Whitespace as usize] =
        Transition::new(State::BeforeAttrName, Action::EmitAttrValue);
    table[State::AttrValueUnquoted as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitOpenTagClose);
    table[State::AttrValueUnquoted as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitOpenTagClose);
    table[State::AttrValueUnquoted as usize][ByteClass::Alpha as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Other as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Dash as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Slash as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Eq as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Quot as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Amp as usize] =
        Transition::noop(State::AttrValueUnquoted);
    table[State::AttrValueUnquoted as usize][ByteClass::Bang as usize] =
        Transition::noop(State::AttrValueUnquoted);

    // ----- SelfClosingStartTag state (saw '/' inside tag) -----
    table[State::SelfClosingStartTag as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitSelfClose);
    // Not a self-close — treat '/' as ignored, go back to before-attr
    table[State::SelfClosingStartTag as usize][ByteClass::Alpha as usize] =
        Transition::new(State::AttrName, Action::StartAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::EmitSelfClose);
    table[State::SelfClosingStartTag as usize][ByteClass::Other as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Slash as usize] =
        Transition::noop(State::SelfClosingStartTag);
    table[State::SelfClosingStartTag as usize][ByteClass::Eq as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Quot as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Amp as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Bang as usize] =
        Transition::noop(State::BeforeAttrName);
    table[State::SelfClosingStartTag as usize][ByteClass::Dash as usize] =
        Transition::noop(State::BeforeAttrName);

    // ----- MarkupDecl state (saw '<!') -----
    table[State::MarkupDecl as usize][ByteClass::Dash as usize] =
        Transition::new(State::Comment, Action::StartComment);
    table[State::MarkupDecl as usize][ByteClass::Alpha as usize] =
        Transition::new(State::Doctype, Action::StartDoctype);
    // '[' for CDATA — classified as Other
    table[State::MarkupDecl as usize][ByteClass::Other as usize] =
        Transition::new(State::CData, Action::StartCData);
    table[State::MarkupDecl as usize][ByteClass::Gt as usize] = Transition::noop(State::Data);
    table[State::MarkupDecl as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::FlushText);
    table[State::MarkupDecl as usize][ByteClass::Slash as usize] = Transition::noop(State::Data);
    table[State::MarkupDecl as usize][ByteClass::Eq as usize] = Transition::noop(State::Data);
    table[State::MarkupDecl as usize][ByteClass::Quot as usize] = Transition::noop(State::Data);
    table[State::MarkupDecl as usize][ByteClass::Amp as usize] = Transition::noop(State::Data);
    table[State::MarkupDecl as usize][ByteClass::Bang as usize] = Transition::noop(State::Data);
    table[State::MarkupDecl as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::Data);

    // ----- Comment state -----
    table[State::Comment as usize][ByteClass::Dash as usize] =
        Transition::noop(State::CommentEndDash);
    table[State::Comment as usize][ByteClass::Lt as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Gt as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Slash as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Eq as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Quot as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Amp as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Bang as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Alpha as usize] = Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::Comment);
    table[State::Comment as usize][ByteClass::Other as usize] = Transition::noop(State::Comment);

    // ----- CommentEndDash state (saw '-' in comment) -----
    table[State::CommentEndDash as usize][ByteClass::Dash as usize] =
        Transition::noop(State::CommentEnd);
    // Not end of comment — back to Comment
    table[State::CommentEndDash as usize][ByteClass::Lt as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Gt as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Slash as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Eq as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Quot as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Amp as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Bang as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Alpha as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEndDash as usize][ByteClass::Other as usize] =
        Transition::noop(State::Comment);

    // ----- CommentEnd state (saw '--' in comment) -----
    table[State::CommentEnd as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitComment);
    // Not closing yet — back to Comment
    table[State::CommentEnd as usize][ByteClass::Dash as usize] =
        Transition::noop(State::CommentEnd);
    table[State::CommentEnd as usize][ByteClass::Lt as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Slash as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Eq as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Quot as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Amp as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Bang as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Alpha as usize] = Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::Comment);
    table[State::CommentEnd as usize][ByteClass::Other as usize] = Transition::noop(State::Comment);

    // ----- Doctype state -----
    table[State::Doctype as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitDoctype);
    table[State::Doctype as usize][ByteClass::Lt as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Slash as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Eq as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Quot as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Amp as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Bang as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Dash as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Alpha as usize] = Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::Doctype);
    table[State::Doctype as usize][ByteClass::Other as usize] = Transition::noop(State::Doctype);

    // ----- CData state -----
    // CDATA ends with `]]>` — we detect `>` and check context in extraction.
    table[State::CData as usize][ByteClass::Gt as usize] =
        Transition::new(State::Data, Action::EmitCData);
    table[State::CData as usize][ByteClass::Lt as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Slash as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Eq as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Quot as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Amp as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Bang as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Dash as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Alpha as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Whitespace as usize] = Transition::noop(State::CData);
    table[State::CData as usize][ByteClass::Other as usize] = Transition::noop(State::CData);

    // ----- RawText state (script/style content) -----
    // Everything stays in RawText until we see '<' (potential end tag).
    table[State::RawText as usize][ByteClass::Lt as usize] =
        Transition::new(State::TagOpen, Action::FlushText);
    table[State::RawText as usize][ByteClass::Gt as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Slash as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Eq as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Quot as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Amp as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Bang as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Dash as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Alpha as usize] = Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Whitespace as usize] =
        Transition::noop(State::RawText);
    table[State::RawText as usize][ByteClass::Other as usize] = Transition::noop(State::RawText);

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_class_delimiters() {
        assert_eq!(ByteClass::from_byte(b'<'), ByteClass::Lt);
        assert_eq!(ByteClass::from_byte(b'>'), ByteClass::Gt);
        assert_eq!(ByteClass::from_byte(b'/'), ByteClass::Slash);
        assert_eq!(ByteClass::from_byte(b'='), ByteClass::Eq);
        assert_eq!(ByteClass::from_byte(b'"'), ByteClass::Quot);
        assert_eq!(ByteClass::from_byte(b'\''), ByteClass::Quot);
        assert_eq!(ByteClass::from_byte(b'&'), ByteClass::Amp);
        assert_eq!(ByteClass::from_byte(b'!'), ByteClass::Bang);
        assert_eq!(ByteClass::from_byte(b'-'), ByteClass::Dash);
    }

    #[test]
    fn byte_class_alpha() {
        assert_eq!(ByteClass::from_byte(b'a'), ByteClass::Alpha);
        assert_eq!(ByteClass::from_byte(b'z'), ByteClass::Alpha);
        assert_eq!(ByteClass::from_byte(b'A'), ByteClass::Alpha);
        assert_eq!(ByteClass::from_byte(b'Z'), ByteClass::Alpha);
    }

    #[test]
    fn byte_class_whitespace() {
        assert_eq!(ByteClass::from_byte(b' '), ByteClass::Whitespace);
        assert_eq!(ByteClass::from_byte(b'\t'), ByteClass::Whitespace);
        assert_eq!(ByteClass::from_byte(b'\n'), ByteClass::Whitespace);
        assert_eq!(ByteClass::from_byte(b'\r'), ByteClass::Whitespace);
    }

    #[test]
    fn byte_class_other() {
        assert_eq!(ByteClass::from_byte(b'0'), ByteClass::Other);
        assert_eq!(ByteClass::from_byte(b'['), ByteClass::Other);
        assert_eq!(ByteClass::from_byte(0xFF), ByteClass::Other);
    }

    #[test]
    fn data_lt_transitions_to_tag_open() {
        let t = STATE_TABLE[State::Data as usize][ByteClass::Lt as usize];
        assert_eq!(t.state, State::TagOpen);
        assert_eq!(t.action, Action::FlushText);
    }

    #[test]
    fn tag_open_alpha_starts_tag() {
        let t = STATE_TABLE[State::TagOpen as usize][ByteClass::Alpha as usize];
        assert_eq!(t.state, State::TagName);
        assert_eq!(t.action, Action::StartTag);
    }

    #[test]
    fn tag_open_slash_to_end_tag() {
        let t = STATE_TABLE[State::TagOpen as usize][ByteClass::Slash as usize];
        assert_eq!(t.state, State::EndTagOpen);
    }

    #[test]
    fn tag_name_gt_emits_tag() {
        let t = STATE_TABLE[State::TagName as usize][ByteClass::Gt as usize];
        assert_eq!(t.state, State::Data);
        assert_eq!(t.action, Action::EmitOpenTagClose);
    }

    #[test]
    fn self_closing_gt_emits() {
        let t = STATE_TABLE[State::SelfClosingStartTag as usize][ByteClass::Gt as usize];
        assert_eq!(t.state, State::Data);
        assert_eq!(t.action, Action::EmitSelfClose);
    }

    #[test]
    fn comment_dash_dash_gt_emits() {
        // First dash
        let t1 = STATE_TABLE[State::Comment as usize][ByteClass::Dash as usize];
        assert_eq!(t1.state, State::CommentEndDash);
        // Second dash
        let t2 = STATE_TABLE[State::CommentEndDash as usize][ByteClass::Dash as usize];
        assert_eq!(t2.state, State::CommentEnd);
        // >
        let t3 = STATE_TABLE[State::CommentEnd as usize][ByteClass::Gt as usize];
        assert_eq!(t3.state, State::Data);
        assert_eq!(t3.action, Action::EmitComment);
    }
}
