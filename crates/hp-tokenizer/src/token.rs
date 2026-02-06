//! Token types emitted by the HTML tokenizer.
//!
//! All variants carry `&'a str` references into the original input,
//! achieving zero-copy tokenization.

use std::borrow::Cow;

use hp_core::tag::Tag;

/// A single token produced by the tokenizer.
///
/// All string payloads borrow from the original input (`'a` lifetime),
/// except entity-decoded values which use `Cow::Owned`.
#[derive(Clone, Debug, PartialEq)]
pub enum Token<'a> {
    /// An opening tag, e.g. `<div class="foo">`.
    OpenTag {
        /// Interned tag name.
        tag: Tag,
        /// Raw tag name slice from the input.
        name: &'a str,
        /// Attribute list (may be empty).
        attributes: Vec<Attribute<'a>>,
        /// Whether the tag is self-closing (`<br/>`).
        self_closing: bool,
    },

    /// A closing tag, e.g. `</div>`.
    CloseTag {
        /// Interned tag name.
        tag: Tag,
        /// Raw tag name slice from the input.
        name: &'a str,
    },

    /// Text content between tags.
    Text {
        /// The text content, entity-decoded if needed.
        content: Cow<'a, str>,
    },

    /// An HTML comment, e.g. `<!-- comment -->`.
    Comment {
        /// The comment body (without `<!--` and `-->`).
        content: &'a str,
    },

    /// A DOCTYPE declaration, e.g. `<!DOCTYPE html>`.
    Doctype {
        /// The content after `DOCTYPE`.
        content: &'a str,
    },

    /// A CDATA section, e.g. `<![CDATA[...]]>`.
    CData {
        /// The CDATA content (without `<![CDATA[` and `]]>`).
        content: &'a str,
    },
}

/// A single HTML attribute (name-value pair).
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute<'a> {
    /// Attribute name, borrowed from input.
    pub name: &'a str,
    /// Attribute value, entity-decoded if needed.
    /// `None` for boolean attributes (e.g. `disabled`).
    pub value: Option<Cow<'a, str>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_clone_and_debug() {
        let tok = Token::Text {
            content: Cow::Borrowed("hello"),
        };
        let tok2 = tok.clone();
        assert_eq!(tok, tok2);
        assert!(format!("{tok:?}").contains("hello"));
    }

    #[test]
    fn attribute_with_value() {
        let attr = Attribute {
            name: "class",
            value: Some(Cow::Borrowed("foo")),
        };
        assert_eq!(attr.name, "class");
        assert_eq!(attr.value.as_deref(), Some("foo"));
    }

    #[test]
    fn boolean_attribute() {
        let attr = Attribute {
            name: "disabled",
            value: None,
        };
        assert!(attr.value.is_none());
    }
}
