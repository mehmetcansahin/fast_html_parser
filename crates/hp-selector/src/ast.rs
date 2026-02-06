//! CSS selector abstract syntax tree.
//!
//! The AST is produced by [`crate::parser`] and consumed by [`crate::matcher`].
//! Selectors are stored in right-to-left order for efficient matching.

use hp_core::tag::Tag;

/// A comma-separated list of selectors.
///
/// A node matches the list if it matches **any** of the selectors.
#[derive(Debug, Clone)]
pub struct SelectorList {
    /// Individual selectors.
    pub selectors: Vec<Selector>,
}

/// A single CSS selector (complex selector).
///
/// Stored in right-to-left order: the subject (rightmost compound) is
/// matched first, then combinators walk leftward through ancestors/siblings.
#[derive(Debug, Clone)]
pub struct Selector {
    /// The rightmost compound selector (the subject to match).
    pub subject: CompoundSelector,
    /// Chain of (combinator, compound) walking leftward from the subject.
    pub chain: Vec<(Combinator, CompoundSelector)>,
}

/// A compound selector: one or more simple selectors applied to a single node.
///
/// E.g., `div.active#main[data-x]` is a single compound with 4 parts.
#[derive(Debug, Clone)]
pub struct CompoundSelector {
    /// Simple selectors that must all match the same node.
    pub parts: Vec<SimpleSelector>,
}

/// A single simple selector.
#[derive(Debug, Clone)]
pub enum SimpleSelector {
    /// Match by tag name: `div`, `p`, `span`.
    Tag(Tag),
    /// Match by class: `.class`.
    Class(String),
    /// Match by id: `#id`.
    Id(String),
    /// Universal selector: `*`.
    Universal,
    /// Attribute selector: `[attr]`, `[attr=val]`, etc.
    Attr(AttrSelector),
    /// `:first-child` pseudo-class.
    PseudoFirstChild,
    /// `:last-child` pseudo-class.
    PseudoLastChild,
    /// `:nth-child(an+b)` pseudo-class.
    PseudoNthChild {
        /// The `a` coefficient.
        a: i32,
        /// The `b` offset.
        b: i32,
    },
    /// `:not(selector)` pseudo-class.
    PseudoNot(Box<CompoundSelector>),
}

/// Attribute selector with comparison operator.
#[derive(Debug, Clone)]
pub struct AttrSelector {
    /// Attribute name.
    pub name: String,
    /// Comparison operator.
    pub op: AttrOp,
    /// Value to compare against (`None` for existence check).
    pub value: Option<String>,
}

/// Attribute comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `[attr]` — attribute exists.
    Exists,
    /// `[attr=val]` — exact match.
    Equals,
    /// `[attr~=val]` — word in space-separated list.
    Includes,
    /// `[attr^=val]` — starts with.
    StartsWith,
    /// `[attr$=val]` — ends with.
    EndsWith,
    /// `[attr*=val]` — contains substring.
    Substring,
}

/// Combinator between compound selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    /// ` ` — descendant (any depth).
    Descendant,
    /// `>` — direct child.
    Child,
    /// `+` — adjacent sibling (immediately preceding).
    AdjacentSibling,
    /// `~` — general sibling (any preceding sibling).
    GeneralSibling,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_debug() {
        let sel = Selector {
            subject: CompoundSelector {
                parts: vec![SimpleSelector::Tag(Tag::Div)],
            },
            chain: vec![],
        };
        let debug = format!("{sel:?}");
        assert!(debug.contains("Div"));
    }

    #[test]
    fn attr_op_eq() {
        assert_eq!(AttrOp::Exists, AttrOp::Exists);
        assert_ne!(AttrOp::Equals, AttrOp::Substring);
    }

    #[test]
    fn combinator_eq() {
        assert_eq!(Combinator::Descendant, Combinator::Descendant);
        assert_ne!(Combinator::Child, Combinator::Descendant);
    }
}
