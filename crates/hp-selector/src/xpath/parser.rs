//! XPath expression parser.
//!
//! Parses a subset of XPath 1.0 into [`XPathExpr`] AST nodes.
//!
//! # Supported syntax
//!
//! - `//tag` — descendant search
//! - `//tag[@attr='value']` — attribute predicate
//! - `//tag[contains(@attr, 'substr')]` — contains predicate
//! - `//tag[position()=N]` — position predicate
//! - `//tag/text()` — text extraction
//! - `/path/to/tag` — absolute path
//! - `//*` — descendant wildcard
//! - `..` — parent axis

use hp_core::error::XPathError;
use hp_core::tag::Tag;

use super::ast::{PathStep, Predicate, XPathExpr};

/// Parse an XPath expression string into an AST.
///
/// # Errors
///
/// Returns [`XPathError::Invalid`] if the expression syntax is invalid.
pub fn parse_xpath(input: &str) -> Result<XPathExpr, XPathError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(XPathError::Invalid {
            reason: "empty xpath expression".to_string(),
        });
    }

    let mut parser = XPathParser::new(input);
    let expr = parser.parse()?;

    // Check for trailing /text()
    if parser.remaining().starts_with("/text()") {
        parser.advance(7);
        parser.skip_whitespace();
        if !parser.is_eof() {
            return Err(XPathError::Invalid {
                reason: format!("unexpected trailing: {}", parser.remaining()),
            });
        }
        return Ok(XPathExpr::TextExtract(Box::new(expr)));
    }

    if !parser.is_eof() {
        return Err(XPathError::Invalid {
            reason: format!("unexpected trailing: {}", parser.remaining()),
        });
    }

    Ok(expr)
}

/// Hand-rolled XPath parser.
struct XPathParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> XPathParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.input.len());
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn parse(&mut self) -> Result<XPathExpr, XPathError> {
        // Handle `..` (parent)
        if self.remaining().starts_with("..") {
            self.advance(2);
            return Ok(XPathExpr::Parent);
        }

        // Handle `//` (descendant axis)
        if self.remaining().starts_with("//") {
            self.advance(2);
            return self.parse_descendant();
        }

        // Handle `/` (absolute path)
        if self.remaining().starts_with('/') {
            self.advance(1);
            return self.parse_absolute_path();
        }

        Err(XPathError::Invalid {
            reason: format!("expected '/' or '//' at: {}", self.remaining()),
        })
    }

    /// Parse `//tag[...]` or `//*[...]` expressions.
    fn parse_descendant(&mut self) -> Result<XPathExpr, XPathError> {
        self.skip_whitespace();

        // Check for wildcard `*`
        if self.peek() == Some(b'*') {
            self.advance(1);
            return self.parse_descendant_wildcard();
        }

        let tag = self.read_tag_name()?;

        // Check for predicate `[...]`
        if self.peek() == Some(b'[') {
            let pred = self.parse_predicate()?;
            return self.build_descendant_with_predicate(tag, pred);
        }

        // Check for `/text()` suffix (handled by caller)
        Ok(XPathExpr::DescendantByTag(tag))
    }

    /// Parse wildcard descendant `//*` with optional predicate.
    fn parse_descendant_wildcard(&mut self) -> Result<XPathExpr, XPathError> {
        if self.peek() == Some(b'[') {
            let pred = self.parse_predicate()?;
            match pred {
                Predicate::AttrEquals { attr, value } => {
                    Ok(XPathExpr::DescendantWildcardByAttr { attr, value })
                }
                Predicate::AttrExists { attr } => {
                    Ok(XPathExpr::DescendantWildcardByAttrExists { attr })
                }
                _ => Err(XPathError::Invalid {
                    reason: "unsupported predicate on wildcard".to_string(),
                }),
            }
        } else {
            Ok(XPathExpr::DescendantWildcard)
        }
    }

    /// Build a descendant expression with a predicate.
    fn build_descendant_with_predicate(
        &self,
        tag: Tag,
        pred: Predicate,
    ) -> Result<XPathExpr, XPathError> {
        match pred {
            Predicate::AttrEquals { attr, value } => {
                Ok(XPathExpr::DescendantByAttr { tag, attr, value })
            }
            Predicate::Contains { attr, substr } => {
                Ok(XPathExpr::ContainsPredicate { tag, attr, substr })
            }
            Predicate::Position(pos) => Ok(XPathExpr::PositionPredicate { tag, pos }),
            Predicate::AttrExists { attr } => Ok(XPathExpr::DescendantByAttrExists { tag, attr }),
        }
    }

    /// Parse an absolute path `/step/step/...`
    fn parse_absolute_path(&mut self) -> Result<XPathExpr, XPathError> {
        let mut steps = Vec::new();
        loop {
            self.skip_whitespace();
            if self.is_eof() || self.remaining().starts_with("/text()") {
                break;
            }

            let tag = self.read_tag_name()?;
            let predicate = if self.peek() == Some(b'[') {
                Some(self.parse_predicate()?)
            } else {
                None
            };

            steps.push(PathStep { tag, predicate });

            // Expect `/` separator or end
            if self.peek() == Some(b'/') {
                // Check for /text() which is handled by caller
                if self.remaining().starts_with("/text()") {
                    break;
                }
                self.advance(1);
            } else {
                break;
            }
        }

        if steps.is_empty() {
            return Err(XPathError::Invalid {
                reason: "empty absolute path".to_string(),
            });
        }

        Ok(XPathExpr::AbsolutePath(steps))
    }

    /// Parse a predicate `[...]`.
    fn parse_predicate(&mut self) -> Result<Predicate, XPathError> {
        self.expect(b'[')?;
        self.skip_whitespace();

        let pred = if self.remaining().starts_with("contains(") {
            self.parse_contains_predicate()?
        } else if self.remaining().starts_with("position()") {
            self.parse_position_predicate()?
        } else if self.peek() == Some(b'@') {
            self.parse_attr_predicate()?
        } else if self.peek().is_some_and(|b| b.is_ascii_digit()) {
            // Shorthand [N] = [position()=N]
            let n = self.read_number()?;
            Predicate::Position(n)
        } else {
            return Err(XPathError::Invalid {
                reason: format!("unsupported predicate at: {}", self.remaining()),
            });
        };

        self.skip_whitespace();
        self.expect(b']')?;
        Ok(pred)
    }

    /// Parse `@attr='value'` or `@attr` inside a predicate.
    fn parse_attr_predicate(&mut self) -> Result<Predicate, XPathError> {
        self.expect(b'@')?;
        let attr = self.read_ident()?;
        self.skip_whitespace();

        if self.peek() == Some(b'=') {
            self.advance(1);
            self.skip_whitespace();
            let value = self.read_string_literal()?;
            Ok(Predicate::AttrEquals { attr, value })
        } else {
            Ok(Predicate::AttrExists { attr })
        }
    }

    /// Parse `contains(@attr, 'substr')`.
    fn parse_contains_predicate(&mut self) -> Result<Predicate, XPathError> {
        self.advance_str("contains(")?;
        self.skip_whitespace();
        self.expect(b'@')?;
        let attr = self.read_ident()?;
        self.skip_whitespace();
        self.expect(b',')?;
        self.skip_whitespace();
        let substr = self.read_string_literal()?;
        self.skip_whitespace();
        self.expect(b')')?;
        Ok(Predicate::Contains { attr, substr })
    }

    /// Parse `position()=N`.
    fn parse_position_predicate(&mut self) -> Result<Predicate, XPathError> {
        self.advance_str("position()")?;
        self.skip_whitespace();
        self.expect(b'=')?;
        self.skip_whitespace();
        let n = self.read_number()?;
        Ok(Predicate::Position(n))
    }

    /// Read a tag name and resolve to a [`Tag`].
    fn read_tag_name(&mut self) -> Result<Tag, XPathError> {
        let name = self.read_ident()?;
        let tag = Tag::from_bytes(name.as_bytes());
        if tag == Tag::Unknown {
            return Err(XPathError::Invalid {
                reason: format!("unknown tag: {name}"),
            });
        }
        Ok(tag)
    }

    /// Read an identifier (letters, digits, hyphens).
    fn read_ident(&mut self) -> Result<String, XPathError> {
        let start = self.pos;
        while self.pos < self.input.len() {
            let b = self.input.as_bytes()[self.pos];
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(XPathError::Invalid {
                reason: format!("expected identifier at position {}", self.pos),
            });
        }
        Ok(self.input[start..self.pos].to_string())
    }

    /// Read a quoted string literal (`'...'` or `"..."`).
    fn read_string_literal(&mut self) -> Result<String, XPathError> {
        let quote = self.peek().ok_or_else(|| XPathError::Invalid {
            reason: "expected string literal, got EOF".to_string(),
        })?;

        if quote != b'\'' && quote != b'"' {
            return Err(XPathError::Invalid {
                reason: format!("expected quote, got '{}'", quote as char),
            });
        }

        self.advance(1);
        let start = self.pos;
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != quote {
            self.pos += 1;
        }
        if self.pos >= self.input.len() {
            return Err(XPathError::Invalid {
                reason: "unclosed string literal".to_string(),
            });
        }
        let value = self.input[start..self.pos].to_string();
        self.advance(1); // skip closing quote
        Ok(value)
    }

    /// Read a positive integer.
    fn read_number(&mut self) -> Result<usize, XPathError> {
        let start = self.pos;
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(XPathError::Invalid {
                reason: format!("expected number at position {}", self.pos),
            });
        }
        self.input[start..self.pos]
            .parse::<usize>()
            .map_err(|_| XPathError::Invalid {
                reason: "invalid number".to_string(),
            })
    }

    /// Expect and consume a specific byte.
    fn expect(&mut self, expected: u8) -> Result<(), XPathError> {
        if self.peek() == Some(expected) {
            self.advance(1);
            Ok(())
        } else {
            Err(XPathError::Invalid {
                reason: format!(
                    "expected '{}', got '{}'",
                    expected as char,
                    self.peek()
                        .map_or("EOF".to_string(), |b| (b as char).to_string())
                ),
            })
        }
    }

    /// Expect and consume a specific string prefix.
    fn advance_str(&mut self, s: &str) -> Result<(), XPathError> {
        if self.remaining().starts_with(s) {
            self.advance(s.len());
            Ok(())
        } else {
            Err(XPathError::Invalid {
                reason: format!("expected '{}' at: {}", s, self.remaining()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::ast::{Predicate, XPathExpr};

    #[test]
    fn parse_descendant_tag() {
        let expr = parse_xpath("//div").unwrap();
        assert_eq!(expr, XPathExpr::DescendantByTag(Tag::Div));
    }

    #[test]
    fn parse_descendant_p() {
        let expr = parse_xpath("//p").unwrap();
        assert_eq!(expr, XPathExpr::DescendantByTag(Tag::P));
    }

    #[test]
    fn parse_descendant_attr() {
        let expr = parse_xpath("//a[@href='http://example.com']").unwrap();
        assert_eq!(
            expr,
            XPathExpr::DescendantByAttr {
                tag: Tag::A,
                attr: "href".to_string(),
                value: "http://example.com".to_string(),
            }
        );
    }

    #[test]
    fn parse_descendant_attr_double_quote() {
        let expr = parse_xpath("//a[@href=\"url\"]").unwrap();
        assert_eq!(
            expr,
            XPathExpr::DescendantByAttr {
                tag: Tag::A,
                attr: "href".to_string(),
                value: "url".to_string(),
            }
        );
    }

    #[test]
    fn parse_descendant_attr_exists() {
        let expr = parse_xpath("//a[@href]").unwrap();
        assert_eq!(
            expr,
            XPathExpr::DescendantByAttrExists {
                tag: Tag::A,
                attr: "href".to_string(),
            }
        );
    }

    #[test]
    fn parse_contains() {
        let expr = parse_xpath("//a[contains(@class, 'nav')]").unwrap();
        assert_eq!(
            expr,
            XPathExpr::ContainsPredicate {
                tag: Tag::A,
                attr: "class".to_string(),
                substr: "nav".to_string(),
            }
        );
    }

    #[test]
    fn parse_position() {
        let expr = parse_xpath("//li[position()=3]").unwrap();
        assert_eq!(
            expr,
            XPathExpr::PositionPredicate {
                tag: Tag::Li,
                pos: 3,
            }
        );
    }

    #[test]
    fn parse_position_shorthand() {
        let expr = parse_xpath("//li[2]").unwrap();
        assert_eq!(
            expr,
            XPathExpr::PositionPredicate {
                tag: Tag::Li,
                pos: 2,
            }
        );
    }

    #[test]
    fn parse_text_extract() {
        let expr = parse_xpath("//p/text()").unwrap();
        assert_eq!(
            expr,
            XPathExpr::TextExtract(Box::new(XPathExpr::DescendantByTag(Tag::P)))
        );
    }

    #[test]
    fn parse_absolute_path() {
        let expr = parse_xpath("/html/body/div").unwrap();
        assert_eq!(
            expr,
            XPathExpr::AbsolutePath(vec![
                PathStep {
                    tag: Tag::Html,
                    predicate: None,
                },
                PathStep {
                    tag: Tag::Body,
                    predicate: None,
                },
                PathStep {
                    tag: Tag::Div,
                    predicate: None,
                },
            ])
        );
    }

    #[test]
    fn parse_absolute_path_with_predicate() {
        let expr = parse_xpath("/html/body/div[@class='main']").unwrap();
        match expr {
            XPathExpr::AbsolutePath(steps) => {
                assert_eq!(steps.len(), 3);
                assert_eq!(steps[2].tag, Tag::Div);
                assert_eq!(
                    steps[2].predicate,
                    Some(Predicate::AttrEquals {
                        attr: "class".to_string(),
                        value: "main".to_string(),
                    })
                );
            }
            _ => panic!("expected AbsolutePath"),
        }
    }

    #[test]
    fn parse_absolute_path_text() {
        let expr = parse_xpath("/html/body/p/text()").unwrap();
        match expr {
            XPathExpr::TextExtract(inner) => {
                assert!(matches!(*inner, XPathExpr::AbsolutePath(_)));
            }
            _ => panic!("expected TextExtract"),
        }
    }

    #[test]
    fn parse_wildcard() {
        let expr = parse_xpath("//*").unwrap();
        assert_eq!(expr, XPathExpr::DescendantWildcard);
    }

    #[test]
    fn parse_wildcard_attr() {
        let expr = parse_xpath("//*[@id='main']").unwrap();
        assert_eq!(
            expr,
            XPathExpr::DescendantWildcardByAttr {
                attr: "id".to_string(),
                value: "main".to_string(),
            }
        );
    }

    #[test]
    fn parse_wildcard_attr_exists() {
        let expr = parse_xpath("//*[@id]").unwrap();
        assert_eq!(
            expr,
            XPathExpr::DescendantWildcardByAttrExists {
                attr: "id".to_string(),
            }
        );
    }

    #[test]
    fn parse_parent() {
        let expr = parse_xpath("..").unwrap();
        assert_eq!(expr, XPathExpr::Parent);
    }

    #[test]
    fn parse_empty_error() {
        assert!(parse_xpath("").is_err());
    }

    #[test]
    fn parse_unknown_tag_error() {
        assert!(parse_xpath("//foobar").is_err());
    }

    #[test]
    fn parse_unclosed_bracket_error() {
        assert!(parse_xpath("//div[@class='x'").is_err());
    }

    #[test]
    fn parse_trailing_garbage_error() {
        assert!(parse_xpath("//div garbage").is_err());
    }
}
