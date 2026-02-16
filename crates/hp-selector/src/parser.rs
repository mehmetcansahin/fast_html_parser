//! CSS selector parser.
//!
//! Parses a CSS selector string into an [`ast::SelectorList`] using a
//! hand-rolled recursive-descent parser. Supports:
//!
//! - Type selectors: `div`, `p`, `span`
//! - Class selectors: `.class`
//! - ID selectors: `#id`
//! - Universal selector: `*`
//! - Attribute selectors: `[attr]`, `[attr=val]`, `[attr~=val]`, `[attr^=val]`,
//!   `[attr$=val]`, `[attr*=val]`
//! - Pseudo-classes: `:first-child`, `:last-child`, `:nth-child(an+b)`, `:not(sel)`
//! - Compound selectors: `div.class#id[attr]`
//! - Combinators: `A B` (descendant), `A > B` (child), `A + B` (adjacent),
//!   `A ~ B` (general sibling)
//! - Comma-separated selector lists: `div, span`

use hp_core::error::SelectorError;
use hp_core::tag::Tag;

use crate::ast::{
    AttrOp, AttrSelector, Combinator, CompoundSelector, Selector, SelectorList, SimpleSelector,
};

/// Parse a CSS selector string into a [`SelectorList`].
///
/// Supports comma-separated selector lists.
///
/// # Errors
///
/// Returns [`SelectorError::Invalid`] if the input is not a valid CSS selector.
pub fn parse_selector(input: &str) -> Result<SelectorList, SelectorError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SelectorError::Invalid {
            reason: "empty selector".to_string(),
        });
    }
    let mut parser = Parser::new(trimmed);
    let list = parser.parse_selector_list()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(SelectorError::Invalid {
            reason: format!(
                "unexpected character '{}' at position {}",
                parser.peek().unwrap() as char,
                parser.pos
            ),
        });
    }
    Ok(list)
}

/// Parse a single CSS selector (no commas).
pub fn parse_single_selector(input: &str) -> Result<Selector, SelectorError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SelectorError::Invalid {
            reason: "empty selector".to_string(),
        });
    }
    let mut parser = Parser::new(trimmed);
    let sel = parser.parse_complex_selector()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(SelectorError::Invalid {
            reason: format!(
                "unexpected character '{}' at position {}",
                parser.peek().unwrap() as char,
                parser.pos
            ),
        });
    }
    Ok(sel)
}

/// Recursive-descent parser for CSS selectors.
struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Skip whitespace, returning `true` if any was consumed.
    fn skip_ws_check(&mut self) -> bool {
        let before = self.pos;
        self.skip_whitespace();
        self.pos > before
    }

    fn expect(&mut self, expected: u8) -> Result<(), SelectorError> {
        match self.advance() {
            Some(b) if b == expected => Ok(()),
            Some(b) => Err(SelectorError::Invalid {
                reason: format!(
                    "expected '{}', found '{}' at position {}",
                    expected as char,
                    b as char,
                    self.pos - 1
                ),
            }),
            None => Err(SelectorError::Invalid {
                reason: format!("expected '{}', found end of input", expected as char),
            }),
        }
    }

    /// Read a CSS identifier (alphanumeric, `-`, `_`).
    fn read_ident(&mut self) -> Result<String, SelectorError> {
        let start = self.pos;
        // CSS identifiers can start with `-` or `_` or alpha
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(SelectorError::Invalid {
                reason: format!("expected identifier at position {}", self.pos),
            });
        }
        // SAFETY: input is UTF-8 and we only consumed ASCII bytes.
        Ok(String::from_utf8_lossy(&self.input[start..self.pos]).into_owned())
    }

    /// Read a quoted string or bare value.
    ///
    /// Bare values accept any non-whitespace, non-`]` characters — more
    /// lenient than strict CSS identifiers so that `[href$=.html]` works
    /// without quotes.
    fn read_value(&mut self) -> Result<String, SelectorError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') | Some(b'\'') => {
                let quote = self.advance().unwrap();
                let start = self.pos;
                while self.pos < self.input.len() && self.input[self.pos] != quote {
                    self.pos += 1;
                }
                if self.is_eof() {
                    return Err(SelectorError::Invalid {
                        reason: "unclosed quote in attribute value".to_string(),
                    });
                }
                let value = String::from_utf8_lossy(&self.input[start..self.pos]).into_owned();
                self.pos += 1; // skip closing quote
                Ok(value)
            }
            _ => {
                // Bare value: read until whitespace or ']'.
                let start = self.pos;
                while self.pos < self.input.len() {
                    let b = self.input[self.pos];
                    if b.is_ascii_whitespace() || b == b']' {
                        break;
                    }
                    self.pos += 1;
                }
                if self.pos == start {
                    return Err(SelectorError::Invalid {
                        reason: format!("expected value at position {}", self.pos),
                    });
                }
                Ok(String::from_utf8_lossy(&self.input[start..self.pos]).into_owned())
            }
        }
    }

    /// Parse a comma-separated selector list.
    fn parse_selector_list(&mut self) -> Result<SelectorList, SelectorError> {
        let mut selectors = Vec::new();
        selectors.push(self.parse_complex_selector()?);
        loop {
            self.skip_whitespace();
            if self.peek() == Some(b',') {
                self.advance();
                self.skip_whitespace();
                selectors.push(self.parse_complex_selector()?);
            } else {
                break;
            }
        }
        Ok(SelectorList { selectors })
    }

    /// Parse a complex selector (compound + combinators).
    fn parse_complex_selector(&mut self) -> Result<Selector, SelectorError> {
        self.skip_whitespace();
        let first = self.parse_compound_selector()?;

        let mut compounds = vec![first];
        let mut combinators = Vec::new();

        loop {
            let had_whitespace = self.skip_ws_check();

            if self.is_eof() {
                break;
            }

            match self.peek() {
                Some(b'>') => {
                    self.advance();
                    self.skip_whitespace();
                    combinators.push(Combinator::Child);
                    compounds.push(self.parse_compound_selector()?);
                }
                Some(b'+') => {
                    self.advance();
                    self.skip_whitespace();
                    combinators.push(Combinator::AdjacentSibling);
                    compounds.push(self.parse_compound_selector()?);
                }
                Some(b'~') => {
                    self.advance();
                    self.skip_whitespace();
                    combinators.push(Combinator::GeneralSibling);
                    compounds.push(self.parse_compound_selector()?);
                }
                Some(b',') | Some(b')') => break,
                _ if had_whitespace && self.is_compound_start() => {
                    combinators.push(Combinator::Descendant);
                    compounds.push(self.parse_compound_selector()?);
                }
                _ => break,
            }
        }

        // Convert left-to-right to right-to-left for matching.
        // compounds[0] comb[0] compounds[1] comb[1] compounds[2]
        // → subject = compounds[last], chain = [(comb[n-1], compounds[n-1]), ..., (comb[0], compounds[0])]
        let subject = compounds.pop().unwrap();
        let mut chain = Vec::new();
        for (compound, combinator) in compounds.into_iter().zip(combinators.into_iter()).rev() {
            chain.push((combinator, compound));
        }

        Ok(Selector { subject, chain })
    }

    /// Check if the current position starts a compound selector.
    fn is_compound_start(&self) -> bool {
        matches!(
            self.peek(),
            Some(b'#' | b'.' | b'[' | b':' | b'*')
                | Some(b'a'..=b'z')
                | Some(b'A'..=b'Z')
                | Some(b'_')
        )
    }

    /// Parse a compound selector (one or more simple selectors).
    fn parse_compound_selector(&mut self) -> Result<CompoundSelector, SelectorError> {
        let mut parts = Vec::new();

        loop {
            if self.is_eof() {
                break;
            }

            match self.peek() {
                Some(b'#') => {
                    self.advance();
                    let id = self.read_ident()?;
                    parts.push(SimpleSelector::Id(id));
                }
                Some(b'.') => {
                    self.advance();
                    let class = self.read_ident()?;
                    parts.push(SimpleSelector::Class(class));
                }
                Some(b'[') => {
                    parts.push(self.parse_attr_selector()?);
                }
                Some(b':') => {
                    parts.push(self.parse_pseudo()?);
                }
                Some(b'*') => {
                    self.advance();
                    parts.push(SimpleSelector::Universal);
                }
                Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
                    let name = self.read_ident()?;
                    let tag = Tag::from_bytes(name.as_bytes());
                    if tag == Tag::Unknown {
                        parts.push(SimpleSelector::UnknownTag(name));
                    } else {
                        parts.push(SimpleSelector::Tag(tag));
                    }
                }
                _ => break,
            }
        }

        if parts.is_empty() {
            return Err(SelectorError::Invalid {
                reason: format!("expected selector at position {}", self.pos),
            });
        }

        Ok(CompoundSelector { parts })
    }

    /// Parse an attribute selector `[attr]` or `[attr op val]`.
    fn parse_attr_selector(&mut self) -> Result<SimpleSelector, SelectorError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let name = self.read_ident()?;
        self.skip_whitespace();

        if self.peek() == Some(b']') {
            self.advance();
            return Ok(SimpleSelector::Attr(AttrSelector {
                name,
                op: AttrOp::Exists,
                value: None,
            }));
        }

        let op = match self.peek() {
            Some(b'=') => {
                self.advance();
                AttrOp::Equals
            }
            Some(b'~') => {
                self.advance();
                self.expect(b'=')?;
                AttrOp::Includes
            }
            Some(b'^') => {
                self.advance();
                self.expect(b'=')?;
                AttrOp::StartsWith
            }
            Some(b'$') => {
                self.advance();
                self.expect(b'=')?;
                AttrOp::EndsWith
            }
            Some(b'*') => {
                self.advance();
                self.expect(b'=')?;
                AttrOp::Substring
            }
            _ => {
                return Err(SelectorError::Invalid {
                    reason: format!("expected attribute operator at position {}", self.pos),
                });
            }
        };

        let value = self.read_value()?;
        self.skip_whitespace();
        self.expect(b']')?;

        Ok(SimpleSelector::Attr(AttrSelector {
            name,
            op,
            value: Some(value),
        }))
    }

    /// Parse a pseudo-class (`:first-child`, `:nth-child(...)`, `:not(...)`).
    fn parse_pseudo(&mut self) -> Result<SimpleSelector, SelectorError> {
        self.expect(b':')?;
        let name = self.read_ident()?;

        match name.as_str() {
            "first-child" => Ok(SimpleSelector::PseudoFirstChild),
            "last-child" => Ok(SimpleSelector::PseudoLastChild),
            "nth-child" => {
                self.expect(b'(')?;
                let (a, b) = self.parse_nth()?;
                self.skip_whitespace();
                self.expect(b')')?;
                Ok(SimpleSelector::PseudoNthChild { a, b })
            }
            "not" => {
                self.expect(b'(')?;
                self.skip_whitespace();
                let inner = self.parse_compound_selector()?;
                self.skip_whitespace();
                self.expect(b')')?;
                Ok(SimpleSelector::PseudoNot(Box::new(inner)))
            }
            _ => Err(SelectorError::Invalid {
                reason: format!("unknown pseudo-class ':{name}'"),
            }),
        }
    }

    /// Parse an `an+b` expression inside `:nth-child(...)`.
    fn parse_nth(&mut self) -> Result<(i32, i32), SelectorError> {
        self.skip_whitespace();

        // Handle keywords.
        if self.peek_keyword("odd") {
            self.pos += 3;
            return Ok((2, 1));
        }
        if self.peek_keyword("even") {
            self.pos += 4;
            return Ok((2, 0));
        }

        // Parse an+b
        let mut sign: i32 = 1;
        if self.peek() == Some(b'-') {
            sign = -1;
            self.advance();
        } else if self.peek() == Some(b'+') {
            self.advance();
        }

        let num_start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        let has_number = self.pos > num_start;
        let number = if has_number {
            let s = std::str::from_utf8(&self.input[num_start..self.pos]).unwrap();
            sign * s.parse::<i32>().unwrap_or(0)
        } else {
            sign // implicit 1 or -1 before n
        };

        if self.peek() == Some(b'n') || self.peek() == Some(b'N') {
            self.advance();
            let a = number;
            self.skip_whitespace();

            let b = match self.peek() {
                Some(b'+') => {
                    self.advance();
                    self.skip_whitespace();
                    self.read_int()?
                }
                Some(b'-') => {
                    self.advance();
                    self.skip_whitespace();
                    -self.read_int()?
                }
                _ => 0,
            };

            Ok((a, b))
        } else if has_number {
            // Just a number: :nth-child(3) means 0n+3
            Ok((0, number))
        } else {
            Err(SelectorError::Invalid {
                reason: "invalid :nth-child expression".to_string(),
            })
        }
    }

    /// Read a positive integer.
    fn read_int(&mut self) -> Result<i32, SelectorError> {
        let start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(SelectorError::Invalid {
                reason: "expected number".to_string(),
            });
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
        Ok(s.parse::<i32>().unwrap_or(0))
    }

    /// Check if the current position starts with a keyword (case-insensitive).
    fn peek_keyword(&self, keyword: &str) -> bool {
        let bytes = keyword.as_bytes();
        if self.pos + bytes.len() > self.input.len() {
            return false;
        }
        for (i, &b) in bytes.iter().enumerate() {
            if !self.input[self.pos + i].eq_ignore_ascii_case(&b) {
                return false;
            }
        }
        // Ensure it's a complete word (not followed by alphanumeric).
        let next_pos = self.pos + bytes.len();
        if next_pos < self.input.len() && self.input[next_pos].is_ascii_alphanumeric() {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag() {
        let list = parse_selector("div").unwrap();
        assert_eq!(list.selectors.len(), 1);
        let sel = &list.selectors[0];
        assert!(sel.chain.is_empty());
        assert_eq!(sel.subject.parts.len(), 1);
        assert!(matches!(
            sel.subject.parts[0],
            SimpleSelector::Tag(Tag::Div)
        ));
    }

    #[test]
    fn parse_class() {
        let list = parse_selector(".foo").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Class(c) => assert_eq!(c, "foo"),
            _ => panic!("expected class selector"),
        }
    }

    #[test]
    fn parse_id() {
        let list = parse_selector("#bar").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Id(id) => assert_eq!(id, "bar"),
            _ => panic!("expected id selector"),
        }
    }

    #[test]
    fn parse_universal() {
        let list = parse_selector("*").unwrap();
        let sel = &list.selectors[0];
        assert!(matches!(sel.subject.parts[0], SimpleSelector::Universal));
    }

    #[test]
    fn parse_compound() {
        let list = parse_selector("div.active#main").unwrap();
        let sel = &list.selectors[0];
        assert_eq!(sel.subject.parts.len(), 3);
        assert!(matches!(
            sel.subject.parts[0],
            SimpleSelector::Tag(Tag::Div)
        ));
        assert!(matches!(&sel.subject.parts[1], SimpleSelector::Class(c) if c == "active"));
        assert!(matches!(&sel.subject.parts[2], SimpleSelector::Id(id) if id == "main"));
    }

    #[test]
    fn parse_descendant() {
        let list = parse_selector("div p").unwrap();
        let sel = &list.selectors[0];
        assert!(matches!(sel.subject.parts[0], SimpleSelector::Tag(Tag::P)));
        assert_eq!(sel.chain.len(), 1);
        assert_eq!(sel.chain[0].0, Combinator::Descendant);
        assert!(matches!(
            sel.chain[0].1.parts[0],
            SimpleSelector::Tag(Tag::Div)
        ));
    }

    #[test]
    fn parse_child() {
        let list = parse_selector("div > p").unwrap();
        let sel = &list.selectors[0];
        assert!(matches!(sel.subject.parts[0], SimpleSelector::Tag(Tag::P)));
        assert_eq!(sel.chain[0].0, Combinator::Child);
    }

    #[test]
    fn parse_adjacent_sibling() {
        let list = parse_selector("h1 + p").unwrap();
        let sel = &list.selectors[0];
        assert!(matches!(sel.subject.parts[0], SimpleSelector::Tag(Tag::P)));
        assert_eq!(sel.chain[0].0, Combinator::AdjacentSibling);
    }

    #[test]
    fn parse_general_sibling() {
        let list = parse_selector("h1 ~ p").unwrap();
        let sel = &list.selectors[0];
        assert!(matches!(sel.subject.parts[0], SimpleSelector::Tag(Tag::P)));
        assert_eq!(sel.chain[0].0, Combinator::GeneralSibling);
    }

    #[test]
    fn parse_attr_exists() {
        let list = parse_selector("[data-x]").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Attr(a) => {
                assert_eq!(a.name, "data-x");
                assert_eq!(a.op, AttrOp::Exists);
                assert!(a.value.is_none());
            }
            _ => panic!("expected attr selector"),
        }
    }

    #[test]
    fn parse_attr_equals() {
        let list = parse_selector("[href=\"url\"]").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Attr(a) => {
                assert_eq!(a.name, "href");
                assert_eq!(a.op, AttrOp::Equals);
                assert_eq!(a.value.as_deref(), Some("url"));
            }
            _ => panic!("expected attr selector"),
        }
    }

    #[test]
    fn parse_attr_includes() {
        let list = parse_selector("[class~=active]").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Attr(a) => {
                assert_eq!(a.op, AttrOp::Includes);
                assert_eq!(a.value.as_deref(), Some("active"));
            }
            _ => panic!("expected attr selector"),
        }
    }

    #[test]
    fn parse_attr_starts_with() {
        let list = parse_selector("[href^=https]").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Attr(a) => assert_eq!(a.op, AttrOp::StartsWith),
            _ => panic!("expected attr selector"),
        }
    }

    #[test]
    fn parse_attr_ends_with() {
        let list = parse_selector("[href$=.html]").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Attr(a) => assert_eq!(a.op, AttrOp::EndsWith),
            _ => panic!("expected attr selector"),
        }
    }

    #[test]
    fn parse_attr_substring() {
        let list = parse_selector("[href*=example]").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Attr(a) => assert_eq!(a.op, AttrOp::Substring),
            _ => panic!("expected attr selector"),
        }
    }

    #[test]
    fn parse_first_child() {
        let list = parse_selector(":first-child").unwrap();
        assert!(matches!(
            list.selectors[0].subject.parts[0],
            SimpleSelector::PseudoFirstChild
        ));
    }

    #[test]
    fn parse_last_child() {
        let list = parse_selector(":last-child").unwrap();
        assert!(matches!(
            list.selectors[0].subject.parts[0],
            SimpleSelector::PseudoLastChild
        ));
    }

    #[test]
    fn parse_nth_child_number() {
        let list = parse_selector(":nth-child(3)").unwrap();
        match list.selectors[0].subject.parts[0] {
            SimpleSelector::PseudoNthChild { a, b } => {
                assert_eq!(a, 0);
                assert_eq!(b, 3);
            }
            _ => panic!("expected nth-child"),
        }
    }

    #[test]
    fn parse_nth_child_odd() {
        let list = parse_selector(":nth-child(odd)").unwrap();
        match list.selectors[0].subject.parts[0] {
            SimpleSelector::PseudoNthChild { a, b } => {
                assert_eq!(a, 2);
                assert_eq!(b, 1);
            }
            _ => panic!("expected nth-child"),
        }
    }

    #[test]
    fn parse_nth_child_even() {
        let list = parse_selector(":nth-child(even)").unwrap();
        match list.selectors[0].subject.parts[0] {
            SimpleSelector::PseudoNthChild { a, b } => {
                assert_eq!(a, 2);
                assert_eq!(b, 0);
            }
            _ => panic!("expected nth-child"),
        }
    }

    #[test]
    fn parse_nth_child_formula() {
        let list = parse_selector(":nth-child(2n+1)").unwrap();
        match list.selectors[0].subject.parts[0] {
            SimpleSelector::PseudoNthChild { a, b } => {
                assert_eq!(a, 2);
                assert_eq!(b, 1);
            }
            _ => panic!("expected nth-child"),
        }
    }

    #[test]
    fn parse_nth_child_negative() {
        let list = parse_selector(":nth-child(-n+3)").unwrap();
        match list.selectors[0].subject.parts[0] {
            SimpleSelector::PseudoNthChild { a, b } => {
                assert_eq!(a, -1);
                assert_eq!(b, 3);
            }
            _ => panic!("expected nth-child"),
        }
    }

    #[test]
    fn parse_not() {
        let list = parse_selector(":not(.hidden)").unwrap();
        match &list.selectors[0].subject.parts[0] {
            SimpleSelector::PseudoNot(inner) => {
                assert!(matches!(&inner.parts[0], SimpleSelector::Class(c) if c == "hidden"));
            }
            _ => panic!("expected :not"),
        }
    }

    #[test]
    fn parse_comma_list() {
        let list = parse_selector("div, span, p").unwrap();
        assert_eq!(list.selectors.len(), 3);
    }

    #[test]
    fn parse_complex_chain() {
        // div > ul li > a.link
        let list = parse_selector("div > ul li > a.link").unwrap();
        let sel = &list.selectors[0];
        // Subject: a.link
        assert_eq!(sel.subject.parts.len(), 2);
        assert!(matches!(sel.subject.parts[0], SimpleSelector::Tag(Tag::A)));
        // Chain (right-to-left): [Child, ul li]; [Descendant, div > ul]
        assert_eq!(sel.chain.len(), 3);
        assert_eq!(sel.chain[0].0, Combinator::Child); // > (before a.link)
        assert_eq!(sel.chain[1].0, Combinator::Descendant); // space (before li)
        assert_eq!(sel.chain[2].0, Combinator::Child); // > (before ul)
    }

    #[test]
    fn parse_empty_error() {
        assert!(parse_selector("").is_err());
        assert!(parse_selector("  ").is_err());
    }

    #[test]
    fn parse_attr_quoted_value() {
        let list = parse_selector("[data-value='hello world']").unwrap();
        match &list.selectors[0].subject.parts[0] {
            SimpleSelector::Attr(a) => {
                assert_eq!(a.value.as_deref(), Some("hello world"));
            }
            _ => panic!("expected attr selector"),
        }
    }
}
