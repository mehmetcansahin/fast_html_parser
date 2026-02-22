//! XPath expression support (subset for web scraping).
//!
//! Provides parsing and evaluation of a commonly-used XPath subset:
//!
//! - `//tag` — descendant search by tag
//! - `//tag[@attr='value']` — attribute predicate
//! - `/path/to/tag` — absolute path
//! - `//tag[contains(@attr, 'substr')]` — contains predicate
//! - `//tag[position()=N]` — position predicate
//! - `//tag/text()` — text extraction
//! - `..` — parent axis

/// XPath expression AST types — [`XPathExpr`](crate::xpath::ast::XPathExpr) and related enums.
pub mod ast;
/// XPath expression evaluator — walks the DOM matching an [`XPathExpr`](crate::xpath::ast::XPathExpr).
pub mod eval;
/// XPath expression parser — parses strings into [`XPathExpr`](crate::xpath::ast::XPathExpr) nodes.
pub mod parser;
