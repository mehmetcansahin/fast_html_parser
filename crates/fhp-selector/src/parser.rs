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
//!
//! Names may contain non-ASCII characters and CSS escapes (`#ürün`,
//! `.md\:flex`, `#\31 23`), and quoted attribute values may contain escapes.
//! Pseudo-class names are ASCII case-insensitive.
//!
//! [`ast::SelectorList`]: crate::ast::SelectorList

use fhp_core::error::SelectorError;
use fhp_core::hash::{class_bloom_bit, selector_hash};
use fhp_core::is_html_whitespace;
use fhp_core::tag::Tag;

use crate::ast::{
    AttrOp, AttrSelector, Combinator, CompoundSelector, Selector, SelectorList, SimpleSelector,
};

/// Maximum accepted selector source length, in UTF-8 bytes.
pub const MAX_SELECTOR_INPUT_LEN: usize = 16 * 1024;
/// Maximum number of comma-separated selector branches.
pub const MAX_SELECTOR_BRANCHES: usize = 64;
/// Maximum number of combinators in one selector branch.
pub const MAX_COMBINATORS_PER_BRANCH: usize = 64;
/// Maximum number of simple-selector AST nodes across the full selector list.
pub const MAX_SIMPLE_PARTS: usize = 256;
/// Maximum supported nesting depth for `:not(...)`.
pub const MAX_NOT_DEPTH: usize = 16;

fn check_input_len(input: &str) -> Result<(), SelectorError> {
    if input.len() > MAX_SELECTOR_INPUT_LEN {
        return Err(limit_error(format!(
            "selector input exceeds {MAX_SELECTOR_INPUT_LEN} bytes"
        )));
    }
    Ok(())
}

fn limit_error(reason: String) -> SelectorError {
    SelectorError::Invalid { reason }
}

/// Whether the byte at `index` is escaped: preceded by an odd number of
/// backslashes and not a newline, which a backslash cannot escape.
fn is_escaped(bytes: &[u8], index: usize) -> bool {
    !matches!(bytes[index], b'\n' | b'\r' | b'\x0C')
        && bytes[..index]
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'\\')
            .count()
            % 2
            == 1
}

/// Parse a CSS selector string into a [`SelectorList`].
///
/// Supports comma-separated selector lists.
///
/// # Errors
///
/// Returns [`SelectorError::Invalid`] if the input is not a valid CSS selector.
/// Positions in the error reason are byte offsets into `input`.
pub fn parse_selector(input: &str) -> Result<SelectorList, SelectorError> {
    check_input_len(input)?;
    let mut parser = Parser::new(input);
    if parser.is_eof() {
        return Err(SelectorError::Invalid {
            reason: "empty selector".to_string(),
        });
    }
    let list = parser.parse_selector_list()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(parser.unexpected_character());
    }
    Ok(list)
}

/// Parse a single CSS selector (no commas).
pub fn parse_single_selector(input: &str) -> Result<Selector, SelectorError> {
    check_input_len(input)?;
    let mut parser = Parser::new(input);
    if parser.is_eof() {
        return Err(SelectorError::Invalid {
            reason: "empty selector".to_string(),
        });
    }
    let sel = parser.parse_complex_selector()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(parser.unexpected_character());
    }
    Ok(sel)
}

/// Recursive-descent parser for CSS selectors.
///
/// `pos` is a byte offset that always lies on a character boundary of `src`.
struct Parser<'a> {
    src: &'a str,
    input: &'a [u8],
    pos: usize,
    simple_parts: usize,
    not_depth: usize,
}

impl<'a> Parser<'a> {
    /// Create a parser over `input` without its surrounding whitespace, keeping
    /// positions relative to the untrimmed input for error messages.
    ///
    /// Only the whitespace CSS itself skips is trimmed. Other characters, such
    /// as U+00A0, are identifier code points, and a trailing space or tab
    /// escaped by a backslash (`.a\ `) belongs to the selector.
    fn new(input: &'a str) -> Self {
        let bytes = input.as_bytes();
        let mut end = bytes.len();
        while end > 0 && is_html_whitespace(bytes[end - 1]) && !is_escaped(bytes, end - 1) {
            end -= 1;
        }
        // Only ASCII bytes were trimmed, so `end` is a character boundary.
        let src = &input[..end];
        Self {
            src,
            input: src.as_bytes(),
            pos: src.bytes().take_while(|&b| is_html_whitespace(b)).count(),
            simple_parts: 0,
            not_depth: 0,
        }
    }

    fn push_simple_part(
        &mut self,
        parts: &mut Vec<SimpleSelector>,
        part: SimpleSelector,
    ) -> Result<(), SelectorError> {
        if self.simple_parts >= MAX_SIMPLE_PARTS {
            return Err(limit_error(format!(
                "selector contains more than {MAX_SIMPLE_PARTS} simple parts"
            )));
        }
        self.simple_parts += 1;
        parts.push(part);
        Ok(())
    }

    fn push_combinator(
        combinators: &mut Vec<Combinator>,
        combinator: Combinator,
    ) -> Result<(), SelectorError> {
        if combinators.len() >= MAX_COMBINATORS_PER_BRANCH {
            return Err(limit_error(format!(
                "selector branch contains more than {MAX_COMBINATORS_PER_BRANCH} combinators"
            )));
        }
        combinators.push(combinator);
        Ok(())
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// The character at the current position.
    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    /// Consume and return the character at the current position.
    fn next_char(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn unexpected_character(&self) -> SelectorError {
        let reason = match self.peek_char() {
            Some(c) => format!("unexpected character '{c}' at position {}", self.pos),
            None => "unexpected end of input".to_string(),
        };
        SelectorError::Invalid { reason }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && is_html_whitespace(self.input[self.pos]) {
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
        match self.peek_char() {
            Some(c) if c == char::from(expected) => {
                self.pos += 1;
                Ok(())
            }
            Some(c) => Err(SelectorError::Invalid {
                reason: format!(
                    "expected '{}', found '{c}' at position {}",
                    char::from(expected),
                    self.pos
                ),
            }),
            None => Err(SelectorError::Invalid {
                reason: format!("expected '{}', found end of input", char::from(expected)),
            }),
        }
    }

    /// Whether the input at the current position is a CSS escape: a `\` not
    /// followed by a newline. A trailing `\` is not treated as an escape.
    fn starts_escape(&self) -> bool {
        self.peek() == Some(b'\\')
            && self
                .input
                .get(self.pos + 1)
                .is_some_and(|&b| !matches!(b, b'\n' | b'\r' | b'\x0C'))
    }

    /// Whether the current position starts a type selector: an ASCII letter,
    /// `_`, a non-ASCII character, or an escape.
    fn starts_type_selector(&self) -> bool {
        match self.peek() {
            Some(b) => {
                b.is_ascii_alphabetic() || b == b'_' || !b.is_ascii() || self.starts_escape()
            }
            None => false,
        }
    }

    /// Consume the code point escaped by the `\` just consumed (CSS Syntax
    /// "consume an escaped code point"). The caller has checked that a
    /// non-newline character follows.
    fn read_escape(&mut self) -> char {
        let start = self.pos;
        while self.pos < self.input.len()
            && self.pos - start < 6
            && self.input[self.pos].is_ascii_hexdigit()
        {
            self.pos += 1;
        }
        if self.pos == start {
            return self.next_char().unwrap_or(char::REPLACEMENT_CHARACTER);
        }

        let hex = &self.src[start..self.pos];
        // One whitespace character after the hex digits ends the escape.
        if self.input[self.pos..].starts_with(b"\r\n") {
            self.pos += 2;
        } else if self.peek().is_some_and(is_html_whitespace) {
            self.pos += 1;
        }
        match u32::from_str_radix(hex, 16) {
            Ok(0) | Err(_) => char::REPLACEMENT_CHARACTER,
            Ok(code) => char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER),
        }
    }

    /// Read a CSS identifier.
    ///
    /// Accepts ASCII alphanumerics, `-`, `_`, non-ASCII characters, and
    /// escapes such as `\:` or `\31 `. Unlike strict CSS, an identifier may
    /// start with a digit or `-`.
    fn read_ident(&mut self) -> Result<String, SelectorError> {
        let start = self.pos;
        let mut ident = String::new();
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' => {
                    ident.push(char::from(b));
                    self.pos += 1;
                }
                Some(b) if !b.is_ascii() => {
                    if let Some(c) = self.next_char() {
                        ident.push(c);
                    }
                }
                Some(b'\\') if self.starts_escape() => {
                    self.pos += 1;
                    ident.push(self.read_escape());
                }
                _ => break,
            }
        }
        if self.pos == start {
            return Err(SelectorError::Invalid {
                reason: format!("expected identifier at position {}", self.pos),
            });
        }
        Ok(ident)
    }

    /// Read a quoted string or bare value, decoding escapes.
    ///
    /// Bare values accept any non-whitespace, non-`]` characters — more
    /// lenient than strict CSS identifiers so that `[href$=.html]` works
    /// without quotes.
    fn read_value(&mut self) -> Result<String, SelectorError> {
        self.skip_whitespace();
        let mut value = String::new();
        match self.peek() {
            Some(quote @ (b'"' | b'\'')) => {
                self.pos += 1;
                loop {
                    match self.peek() {
                        None => {
                            return Err(SelectorError::Invalid {
                                reason: "unclosed quote in attribute value".to_string(),
                            });
                        }
                        Some(b) if b == quote => {
                            self.pos += 1;
                            return Ok(value);
                        }
                        Some(b'\\') => {
                            self.pos += 1;
                            // An escaped newline continues the string.
                            if self.input[self.pos..].starts_with(b"\r\n") {
                                self.pos += 2;
                            } else if matches!(self.peek(), Some(b'\n' | b'\r' | b'\x0C')) {
                                self.pos += 1;
                            } else if !self.is_eof() {
                                value.push(self.read_escape());
                            }
                        }
                        Some(_) => {
                            if let Some(c) = self.next_char() {
                                value.push(c);
                            }
                        }
                    }
                }
            }
            _ => {
                // Bare value: read until whitespace or ']'.
                let start = self.pos;
                loop {
                    match self.peek() {
                        None | Some(b']') => break,
                        Some(b) if is_html_whitespace(b) => break,
                        Some(b'\\') if self.starts_escape() => {
                            self.pos += 1;
                            value.push(self.read_escape());
                        }
                        Some(_) => {
                            if let Some(c) = self.next_char() {
                                value.push(c);
                            }
                        }
                    }
                }
                if self.pos == start {
                    return Err(SelectorError::Invalid {
                        reason: format!("expected value at position {}", self.pos),
                    });
                }
                Ok(value)
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
                if selectors.len() >= MAX_SELECTOR_BRANCHES {
                    return Err(limit_error(format!(
                        "selector list contains more than {MAX_SELECTOR_BRANCHES} branches"
                    )));
                }
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
                    Self::push_combinator(&mut combinators, Combinator::Child)?;
                    compounds.push(self.parse_compound_selector()?);
                }
                Some(b'+') => {
                    self.advance();
                    self.skip_whitespace();
                    Self::push_combinator(&mut combinators, Combinator::AdjacentSibling)?;
                    compounds.push(self.parse_compound_selector()?);
                }
                Some(b'~') => {
                    self.advance();
                    self.skip_whitespace();
                    Self::push_combinator(&mut combinators, Combinator::GeneralSibling)?;
                    compounds.push(self.parse_compound_selector()?);
                }
                Some(b',') | Some(b')') => break,
                _ if had_whitespace && self.is_compound_start() => {
                    Self::push_combinator(&mut combinators, Combinator::Descendant)?;
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
        for (compound, combinator) in compounds.into_iter().zip(combinators).rev() {
            chain.push((combinator, compound));
        }

        Ok(Selector { subject, chain })
    }

    /// Check if the current position starts a compound selector.
    fn is_compound_start(&self) -> bool {
        matches!(self.peek(), Some(b'#' | b'.' | b'[' | b':' | b'*')) || self.starts_type_selector()
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
                    let hash = selector_hash(id.as_bytes());
                    self.push_simple_part(&mut parts, SimpleSelector::Id(id, hash))?;
                }
                Some(b'.') => {
                    self.advance();
                    let class = self.read_ident()?;
                    let bloom = class_bloom_bit(class.as_bytes());
                    self.push_simple_part(&mut parts, SimpleSelector::Class(class, bloom))?;
                }
                Some(b'[') => {
                    let part = self.parse_attr_selector()?;
                    self.push_simple_part(&mut parts, part)?;
                }
                Some(b':') => {
                    let part = self.parse_pseudo()?;
                    self.push_simple_part(&mut parts, part)?;
                }
                Some(b'*') => {
                    if !parts.is_empty() {
                        return Err(SelectorError::Invalid {
                            reason: format!(
                                "type or universal selector must be first at position {}",
                                self.pos
                            ),
                        });
                    }
                    self.advance();
                    self.push_simple_part(&mut parts, SimpleSelector::Universal)?;
                }
                _ if self.starts_type_selector() => {
                    if !parts.is_empty() {
                        return Err(SelectorError::Invalid {
                            reason: format!(
                                "type or universal selector must be first at position {}",
                                self.pos
                            ),
                        });
                    }
                    let name = self.read_ident()?;
                    let tag = Tag::from_bytes(name.as_bytes());
                    if tag == Tag::Unknown {
                        self.push_simple_part(&mut parts, SimpleSelector::UnknownTag(name))?;
                    } else {
                        self.push_simple_part(&mut parts, SimpleSelector::Tag(tag))?;
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

        // Pseudo-class names are ASCII case-insensitive.
        match name.to_ascii_lowercase().as_str() {
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
                if self.not_depth >= MAX_NOT_DEPTH {
                    return Err(limit_error(format!(
                        ":not() nesting exceeds {MAX_NOT_DEPTH} levels"
                    )));
                }
                self.expect(b'(')?;
                self.skip_whitespace();
                self.not_depth += 1;
                let inner = self.parse_compound_selector();
                self.not_depth -= 1;
                let inner = inner?;
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
        let mut sign: i64 = 1;
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
            let magnitude = s.parse::<i64>().map_err(|_| SelectorError::Invalid {
                reason: "integer in :nth-child is out of range".to_string(),
            })?;
            i32::try_from(sign * magnitude).map_err(|_| SelectorError::Invalid {
                reason: "integer in :nth-child is out of range".to_string(),
            })?
        } else {
            sign as i32 // implicit 1 or -1 before n
        };

        if self.peek() == Some(b'n') || self.peek() == Some(b'N') {
            self.advance();
            let a = number;
            self.skip_whitespace();

            let b = match self.peek() {
                Some(b'+') => {
                    self.advance();
                    self.skip_whitespace();
                    self.read_int(1)?
                }
                Some(b'-') => {
                    self.advance();
                    self.skip_whitespace();
                    self.read_int(-1)?
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

    /// Read an integer magnitude and apply `sign` before the i32 range check.
    fn read_int(&mut self, sign: i64) -> Result<i32, SelectorError> {
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
        let magnitude = s.parse::<i64>().map_err(|_| SelectorError::Invalid {
            reason: "integer in :nth-child is out of range".to_string(),
        })?;
        i32::try_from(sign * magnitude).map_err(|_| SelectorError::Invalid {
            reason: "integer in :nth-child is out of range".to_string(),
        })
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
            SimpleSelector::Class(c, _) => assert_eq!(c, "foo"),
            _ => panic!("expected class selector"),
        }
    }

    #[test]
    fn parse_id() {
        let list = parse_selector("#bar").unwrap();
        let sel = &list.selectors[0];
        match &sel.subject.parts[0] {
            SimpleSelector::Id(id, _) => assert_eq!(id, "bar"),
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
        assert!(matches!(&sel.subject.parts[1], SimpleSelector::Class(c, _) if c == "active"));
        assert!(matches!(&sel.subject.parts[2], SimpleSelector::Id(id, _) if id == "main"));
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
                assert!(matches!(&inner.parts[0], SimpleSelector::Class(c, _) if c == "hidden"));
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

    #[test]
    fn rejects_selector_input_over_limit() {
        let input = " ".repeat(MAX_SELECTOR_INPUT_LEN + 1);
        let err = parse_selector(&input).unwrap_err();
        assert!(err.to_string().contains("exceeds 16384 bytes"));

        let err = parse_single_selector(&input).unwrap_err();
        assert!(err.to_string().contains("exceeds 16384 bytes"));
    }

    #[test]
    fn selector_branch_limit_is_inclusive() {
        let at_limit = std::iter::repeat_n("div", MAX_SELECTOR_BRANCHES)
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_selector(&at_limit).unwrap().selectors.len(),
            MAX_SELECTOR_BRANCHES
        );

        let over_limit = format!("{at_limit},div");
        let err = parse_selector(&over_limit).unwrap_err();
        assert!(err.to_string().contains("more than 64 branches"));
    }

    #[test]
    fn combinator_limit_is_inclusive() {
        let at_limit = std::iter::repeat_n("div", MAX_COMBINATORS_PER_BRANCH + 1)
            .collect::<Vec<_>>()
            .join(" > ");
        assert_eq!(
            parse_single_selector(&at_limit).unwrap().chain.len(),
            MAX_COMBINATORS_PER_BRANCH
        );

        let over_limit = format!("{at_limit} > div");
        let err = parse_single_selector(&over_limit).unwrap_err();
        assert!(err.to_string().contains("more than 64 combinators"));
    }

    #[test]
    fn simple_part_limit_is_inclusive() {
        let at_limit = ".a".repeat(MAX_SIMPLE_PARTS);
        assert_eq!(
            parse_single_selector(&at_limit)
                .unwrap()
                .subject
                .parts
                .len(),
            MAX_SIMPLE_PARTS
        );

        let over_limit = format!("{at_limit}.a");
        let err = parse_single_selector(&over_limit).unwrap_err();
        assert!(err.to_string().contains("more than 256 simple parts"));
    }

    #[test]
    fn not_nesting_limit_is_inclusive() {
        let at_limit = format!(
            "{}div{}",
            ":not(".repeat(MAX_NOT_DEPTH),
            ")".repeat(MAX_NOT_DEPTH)
        );
        assert!(parse_single_selector(&at_limit).is_ok());

        let over_limit = format!(
            "{}div{}",
            ":not(".repeat(MAX_NOT_DEPTH + 1),
            ")".repeat(MAX_NOT_DEPTH + 1)
        );
        let err = parse_single_selector(&over_limit).unwrap_err();
        assert!(err.to_string().contains("exceeds 16 levels"));
    }

    fn single_id(css: &str) -> String {
        match &parse_single_selector(css).unwrap().subject.parts[0] {
            SimpleSelector::Id(id, _) => id.clone(),
            other => panic!("expected id selector, got {other:?}"),
        }
    }

    fn attr_value(css: &str) -> String {
        match &parse_single_selector(css).unwrap().subject.parts[0] {
            SimpleSelector::Attr(attr) => attr.value.clone().unwrap(),
            other => panic!("expected attribute selector, got {other:?}"),
        }
    }

    #[test]
    fn identifier_escapes_follow_css_syntax() {
        // Up to six hex digits, then one optional whitespace character.
        assert_eq!(single_id("#\\31 23"), "123");
        assert_eq!(single_id("#\\00003123"), "123");
        assert_eq!(single_id("#\\31\r\n23"), "123");
        assert_eq!(single_id("#a\\:b"), "a:b");
        assert_eq!(single_id("#\\é"), "é");
        assert_eq!(single_id("#ürün"), "ürün");
        // Zero, surrogates, and values above U+10FFFF become U+FFFD.
        assert_eq!(single_id("#\\0"), "\u{FFFD}");
        assert_eq!(single_id("#\\d800"), "\u{FFFD}");
        assert_eq!(single_id("#\\110000"), "\u{FFFD}");
        // A backslash before a newline or at the end is not an escape.
        assert!(parse_single_selector("#a\\").is_err());
        assert!(parse_single_selector("#a\\\nb").is_err());
    }

    #[test]
    fn only_unescaped_css_whitespace_is_trimmed() {
        assert_eq!(single_id(" \t#a \n\x0C"), "a");
        // An escaped trailing space or tab belongs to the identifier.
        assert_eq!(single_id(r"#a\ "), "a ");
        assert_eq!(single_id("#a\\\t  "), "a\t");
        // An escaped backslash does not escape the space after it.
        assert_eq!(single_id(r"#a\\ "), "a\\");
        // Non-CSS whitespace such as U+00A0 is an identifier code point.
        assert_eq!(single_id("#a\u{a0}"), "a\u{a0}");
        assert!(matches!(
            &parse_single_selector("\u{a0}div").unwrap().subject.parts[0],
            SimpleSelector::UnknownTag(name) if name == "\u{a0}div"
        ));
    }

    #[test]
    fn attribute_values_decode_escapes() {
        assert_eq!(attr_value(r#"[a="say \"hi\""]"#), r#"say "hi""#);
        assert_eq!(attr_value(r"[a='it\'s']"), "it's");
        assert_eq!(attr_value(r"[a='\41 B']"), "AB");
        assert_eq!(attr_value("[a='x\\\ny']"), "xy");
        assert_eq!(attr_value(r"[a=x\]y]"), "x]y");
        assert_eq!(attr_value("[a='ürün']"), "ürün");
        assert!(parse_single_selector(r"[a='x\']").is_err());
    }

    #[test]
    fn non_ascii_type_selectors_start_compounds() {
        let sel = parse_single_selector("div ürün-x").unwrap();
        assert_eq!(sel.chain[0].0, Combinator::Descendant);
        assert!(
            matches!(&sel.subject.parts[0], SimpleSelector::UnknownTag(name) if name == "ürün-x")
        );
    }
}
