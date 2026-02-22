//! XPath expression AST types.
//!
//! Represents a subset of XPath 1.0 sufficient for common web scraping tasks.

use fhp_core::tag::Tag;

/// A parsed XPath expression.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathExpr {
    /// `//tag` — find descendants by tag name.
    DescendantByTag(Tag),

    /// `//tag[@attr='value']` — find descendants by tag with an attribute
    /// predicate.
    DescendantByAttr {
        /// The tag to match.
        tag: Tag,
        /// Attribute name.
        attr: String,
        /// Attribute value.
        value: String,
    },

    /// `//tag[@attr]` — descendants by tag with attribute existence predicate.
    DescendantByAttrExists {
        /// The tag to match.
        tag: Tag,
        /// Attribute name.
        attr: String,
    },

    /// `/path/to/tag` — absolute path from root.
    AbsolutePath(Vec<PathStep>),

    /// `//tag[contains(@attr, 'substr')]` — find descendants by tag with a
    /// contains predicate.
    ContainsPredicate {
        /// The tag to match.
        tag: Tag,
        /// Attribute name.
        attr: String,
        /// Substring to search for.
        substr: String,
    },

    /// `//tag[position()=N]` — find descendants by tag at a specific position.
    PositionPredicate {
        /// The tag to match.
        tag: Tag,
        /// 1-based position.
        pos: usize,
    },

    /// `//tag/text()` or expression/text() — extract text from matched nodes.
    TextExtract(Box<XPathExpr>),

    /// `*` in descendant context — `//\*`
    DescendantWildcard,

    /// `//\*[@attr='value']` — wildcard with attribute predicate.
    DescendantWildcardByAttr {
        /// Attribute name.
        attr: String,
        /// Attribute value.
        value: String,
    },

    /// `//\*[@attr]` — wildcard with attribute existence predicate.
    DescendantWildcardByAttrExists {
        /// Attribute name.
        attr: String,
    },

    /// `..` — parent axis (relative to a context node).
    Parent,
}

/// A single step in an absolute path (`/step/step/...`).
#[derive(Debug, Clone, PartialEq)]
pub struct PathStep {
    /// Tag name for this step.
    pub tag: Tag,
    /// Optional predicate for this step.
    pub predicate: Option<Predicate>,
}

/// A predicate inside square brackets `[...]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// `[@attr='value']` — exact attribute match.
    AttrEquals {
        /// Attribute name.
        attr: String,
        /// Expected value.
        value: String,
    },

    /// `[contains(@attr, 'substr')]` — attribute substring match.
    Contains {
        /// Attribute name.
        attr: String,
        /// Substring to find.
        substr: String,
    },

    /// `[position()=N]` — 1-based position among siblings of same type.
    Position(usize),

    /// `[@attr]` — attribute existence check.
    AttrExists {
        /// Attribute name.
        attr: String,
    },
}

/// Result of evaluating an XPath expression.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathResult {
    /// A list of matched node ids.
    Nodes(Vec<fhp_tree::node::NodeId>),
    /// A list of extracted strings (e.g. from `text()`).
    Strings(Vec<String>),
    /// A boolean result.
    Boolean(bool),
}

impl XPathResult {
    /// Returns `true` if the result contains no nodes or strings.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Nodes(v) => v.is_empty(),
            Self::Strings(v) => v.is_empty(),
            Self::Boolean(_) => false,
        }
    }

    /// Returns the number of items in the result.
    pub fn len(&self) -> usize {
        match self {
            Self::Nodes(v) => v.len(),
            Self::Strings(v) => v.len(),
            Self::Boolean(_) => 1,
        }
    }
}
