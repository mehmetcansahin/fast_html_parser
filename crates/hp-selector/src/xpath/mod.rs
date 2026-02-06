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

/// XPath expression AST types.
pub mod ast;
/// XPath expression evaluator.
pub mod eval;
/// XPath expression parser.
pub mod parser;
