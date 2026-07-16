#![cfg_attr(docsrs, feature(doc_cfg))]
//! # fast-html-parser — SIMD-Optimized HTML Parser
//!
//! A high-performance HTML parser designed for web scraping workloads.
//! Uses SIMD instructions (SSE4.2, AVX2, NEON) for tokenization and builds
//! a cache-line aligned arena-based DOM tree for fast traversal.
//!
//! ## Quick Start
//!
//! ```
//! use fast_html_parser::HtmlParser;
//!
//! let doc = HtmlParser::parse("<div><p>Hello</p></div>").unwrap();
//! assert_eq!(doc.root().text_content(), "Hello");
//! ```
//!
//! ## Builder Pattern
//!
//! ```
//! use fast_html_parser::HtmlParser;
//!
//! let doc = HtmlParser::builder()
//!     .max_input_size(64 * 1024 * 1024) // 64 MiB
//!     .build()
//!     .parse_str("<div>Hello</div>")
//!     .unwrap();
//! ```
//!
//! ## CSS Selectors
//!
//! ```
//! # #[cfg(feature = "css-selector")]
//! # {
//! use fast_html_parser::prelude::*;
//!
//! let doc = HtmlParser::parse("<ul><li>one</li><li>two</li></ul>").unwrap();
//! let items = doc.select("li").unwrap();
//! assert_eq!(items.len(), 2);
//! # }
//! ```
//!
//! ## Streaming
//!
//! ```
//! # #[cfg(feature = "encoding")]
//! # {
//! use fast_html_parser::streaming::parse_stream;
//!
//! let html = b"<div><p>Hello</p></div>";
//! let doc = parse_stream(html.chunks(8)).unwrap();
//! assert_eq!(doc.root().text_content(), "Hello");
//! # }
//! ```
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---|---|---|
//! | `css-selector` | Yes | CSS selector engine |
//! | `entity-decode` | Yes | HTML entity decoding |
//! | `simd` | Yes | Runtime-dispatched SIMD tokenizer; disable for forced scalar execution |
//! | `xpath` | No | XPath expression support |
//! | `encoding` | Yes | Raw-byte and streaming parsing with encoding detection |
//! | `async-tokio` | No | Async parsing via Tokio |
//! | `async-async-std` | No | Async parsing via async-std |

// ---------------------------------------------------------------------------
// Re-exports: core types
// ---------------------------------------------------------------------------

/// Core types: interned tags, entity table, error definitions.
pub use fhp_core as core_types;

/// Interned HTML tag enum.
pub use fhp_core::tag::Tag;

/// Tokenizer (low-level).
pub use fhp_tokenizer as tokenizer;

/// DOM tree types.
pub use fhp_tree as tree;

/// Parsed document and node reference.
pub use fhp_tree::{Document, HtmlError, NodeRef};

/// Node identity type.
pub use fhp_tree::node::NodeId;

/// Streaming and incremental parsing.
#[cfg(feature = "encoding")]
#[cfg_attr(docsrs, doc(cfg(feature = "encoding")))]
pub mod streaming {
    pub use fhp_tree::streaming::{
        EarlyStopMatch, EarlyStopOutcome, EarlyStopParser, EarlyStopProgress, MatchCompleteness,
        StreamParser, parse_stream, parse_stream_with_limit,
    };
}

// ---------------------------------------------------------------------------
// Conditional re-exports
// ---------------------------------------------------------------------------

/// CSS selector and XPath engine.
#[cfg(any(feature = "css-selector", feature = "xpath"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "css-selector", feature = "xpath"))))]
pub use fhp_selector::{CompiledSelector, DocumentIndex, Selectable, Selection};

/// XPath types (re-exported from selector crate).
#[cfg(feature = "xpath")]
#[cfg_attr(docsrs, doc(cfg(feature = "xpath")))]
pub mod xpath {
    pub use fhp_selector::xpath::ast::XPathResult;
}

/// Encoding detection and conversion.
#[cfg(feature = "encoding")]
#[cfg_attr(docsrs, doc(cfg(feature = "encoding")))]
pub mod encoding {
    pub use fhp_encoding::{Encoding, decode, decode_or_detect, detect};
}

/// Async parser (requires `async-tokio` feature).
#[cfg(feature = "async-tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "async-tokio")))]
pub mod async_parser {
    pub use fhp_tree::async_parser::{AsyncParser, parse_async};
}

/// Async parser powered by async-std (requires `async-async-std`).
#[cfg(feature = "async-async-std")]
#[cfg_attr(docsrs, doc(cfg(feature = "async-async-std")))]
pub mod async_std_parser {
    pub use fhp_tree::async_std_parser::{AsyncStdParser, parse_async_std};
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience prelude that imports the most commonly used types.
///
/// ```
/// use fast_html_parser::prelude::*;
/// ```
pub mod prelude {
    pub use fhp_tree::node::NodeId;
    pub use fhp_tree::{Document, HtmlError, NodeRef};

    #[cfg(any(feature = "css-selector", feature = "xpath"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "css-selector", feature = "xpath"))))]
    pub use fhp_selector::{CompiledSelector, Selectable, Selection};

    pub use crate::HtmlParser;
}

// ---------------------------------------------------------------------------
// Builder + HtmlParser
// ---------------------------------------------------------------------------

/// Default maximum input size (256 MiB).
const DEFAULT_MAX_INPUT_SIZE: usize = 256 * 1024 * 1024;

/// Configuration builder for the HTML parser.
///
/// # Example
///
/// ```
/// use fast_html_parser::HtmlParser;
///
/// let parser = HtmlParser::builder()
///     .max_input_size(128 * 1024 * 1024)
///     .build();
///
/// let doc = parser.parse_str("<p>fragment</p>").unwrap();
/// assert_eq!(doc.root().text_content(), "fragment");
/// ```
pub struct ParserBuilder {
    max_input_size: usize,
}

impl Default for ParserBuilder {
    fn default() -> Self {
        Self {
            max_input_size: DEFAULT_MAX_INPUT_SIZE,
        }
    }
}

impl ParserBuilder {
    /// Set the maximum input size in bytes.
    ///
    /// Inputs exceeding this limit will return [`HtmlError::InputTooLarge`].
    /// Default: 256 MiB.
    pub fn max_input_size(mut self, size: usize) -> Self {
        self.max_input_size = size;
        self
    }

    /// Consume the builder and create a configured [`HtmlParser`].
    pub fn build(self) -> HtmlParser {
        HtmlParser {
            max_input_size: self.max_input_size,
        }
    }
}

/// A configured HTML parser instance.
///
/// Create via [`HtmlParser::builder()`] for custom configuration, or use the
/// convenience methods [`HtmlParser::parse()`] and [`HtmlParser::parse_bytes()`]
/// for defaults.
///
/// # Example
///
/// ```
/// use fast_html_parser::HtmlParser;
///
/// // One-shot convenience
/// let doc = HtmlParser::parse("<p>Hello</p>").unwrap();
///
/// // Builder pattern
/// let parser = HtmlParser::builder()
///     .max_input_size(1024 * 1024)
///     .build();
/// let doc = parser.parse_str("<p>World</p>").unwrap();
/// ```
pub struct HtmlParser {
    max_input_size: usize,
}

impl HtmlParser {
    /// Create a new [`ParserBuilder`].
    pub fn builder() -> ParserBuilder {
        ParserBuilder::default()
    }

    /// Parse an HTML string with default settings.
    ///
    /// This is a convenience wrapper around `fhp_tree::parse()`.
    ///
    /// # Errors
    ///
    /// Returns [`HtmlError::InputTooLarge`] if the input exceeds 256 MiB.
    ///
    /// # Example
    ///
    /// ```
    /// use fast_html_parser::HtmlParser;
    ///
    /// let doc = HtmlParser::parse("<div><p>Hello</p></div>").unwrap();
    /// assert_eq!(doc.root().text_content(), "Hello");
    /// ```
    pub fn parse(input: &str) -> Result<Document, HtmlError> {
        fhp_tree::parse(input)
    }

    /// Parse an owned `String` with default settings, transferring the allocation.
    ///
    /// Avoids a memcpy of the source bytes when the caller already owns the
    /// input (e.g., from an HTTP response body).
    ///
    /// # Errors
    ///
    /// Returns [`HtmlError::InputTooLarge`] if the input exceeds 256 MiB.
    ///
    /// # Example
    ///
    /// ```
    /// use fast_html_parser::HtmlParser;
    ///
    /// let html = String::from("<div><p>Hello</p></div>");
    /// let doc = HtmlParser::parse_owned(html).unwrap();
    /// assert_eq!(doc.root().text_content(), "Hello");
    /// ```
    pub fn parse_owned(input: String) -> Result<Document, HtmlError> {
        fhp_tree::parse_owned(input)
    }

    /// Parse raw bytes with default settings, auto-detecting encoding.
    ///
    /// # Errors
    ///
    /// Returns [`HtmlError::InputTooLarge`] or [`HtmlError::Encoding`] on
    /// failure.
    ///
    /// # Example
    ///
    /// ```
    /// use fast_html_parser::HtmlParser;
    ///
    /// let doc = HtmlParser::parse_bytes(b"<p>Hello</p>").unwrap();
    /// assert_eq!(doc.root().text_content(), "Hello");
    /// ```
    #[cfg(feature = "encoding")]
    #[cfg_attr(docsrs, doc(cfg(feature = "encoding")))]
    pub fn parse_bytes(input: &[u8]) -> Result<Document, HtmlError> {
        fhp_tree::parse_bytes(input)
    }

    /// Parse an HTML string with the current configuration.
    ///
    /// # Errors
    ///
    /// Returns [`HtmlError::InputTooLarge`] if the input exceeds the
    /// configured limit.
    pub fn parse_str(&self, input: &str) -> Result<Document, HtmlError> {
        fhp_tree::parse_with_limit(input, self.max_input_size)
    }

    /// Parse an owned `String` with the current configuration.
    ///
    /// Avoids a memcpy of the source bytes when the caller already owns the
    /// input (e.g., from an HTTP response body).
    ///
    /// # Errors
    ///
    /// Returns [`HtmlError::InputTooLarge`] if the input exceeds the
    /// configured limit.
    pub fn parse_str_owned(&self, input: String) -> Result<Document, HtmlError> {
        fhp_tree::parse_owned_with_limit(input, self.max_input_size)
    }

    /// Parse raw bytes with the current configuration, auto-detecting encoding.
    ///
    /// # Errors
    ///
    /// Returns [`HtmlError::InputTooLarge`] or [`HtmlError::Encoding`] on
    /// failure.
    #[cfg(feature = "encoding")]
    #[cfg_attr(docsrs, doc(cfg(feature = "encoding")))]
    pub fn parse_raw(&self, input: &[u8]) -> Result<Document, HtmlError> {
        fhp_tree::parse_bytes_with_limit(input, self.max_input_size)
    }
}

/// Parse an HTML string with default settings (convenience alias).
///
/// # Example
///
/// ```
/// let doc = fast_html_parser::parse("<p>Quick</p>").unwrap();
/// assert_eq!(doc.root().text_content(), "Quick");
/// ```
pub fn parse(input: &str) -> Result<Document, HtmlError> {
    HtmlParser::parse(input)
}

/// Parse an owned `String` with default settings, transferring the allocation.
///
/// # Example
///
/// ```
/// let doc = fast_html_parser::parse_owned(String::from("<p>Quick</p>")).unwrap();
/// assert_eq!(doc.root().text_content(), "Quick");
/// ```
pub fn parse_owned(input: String) -> Result<Document, HtmlError> {
    HtmlParser::parse_owned(input)
}

/// Parse raw bytes with default settings, auto-detecting encoding.
///
/// # Example
///
/// ```
/// let doc = fast_html_parser::parse_bytes(b"<p>Quick</p>").unwrap();
/// assert_eq!(doc.root().text_content(), "Quick");
/// ```
#[cfg(feature = "encoding")]
#[cfg_attr(docsrs, doc(cfg(feature = "encoding")))]
pub fn parse_bytes(input: &[u8]) -> Result<Document, HtmlError> {
    HtmlParser::parse_bytes(input)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_convenience() {
        let doc = parse("<div><p>Hello</p></div>").unwrap();
        assert_eq!(doc.root().text_content(), "Hello");
    }

    #[test]
    #[cfg(feature = "encoding")]
    fn parse_bytes_convenience() {
        let doc = parse_bytes(b"<div><p>Hello</p></div>").unwrap();
        assert_eq!(doc.root().text_content(), "Hello");
    }

    #[test]
    fn builder_default() {
        let parser = HtmlParser::builder().build();
        let doc = parser.parse_str("<p>ok</p>").unwrap();
        assert_eq!(doc.root().text_content(), "ok");
    }

    #[test]
    fn builder_max_input_size() {
        let parser = HtmlParser::builder().max_input_size(10).build();
        let result = parser.parse_str("<p>this is too long</p>");
        assert!(matches!(
            result,
            Err(HtmlError::InputTooLarge { size: 23, max: 10 })
        ));
    }

    #[test]
    #[cfg(feature = "encoding")]
    fn builder_parse_raw() {
        let parser = HtmlParser::builder().build();
        let doc = parser.parse_raw(b"<p>bytes</p>").unwrap();
        assert_eq!(doc.root().text_content(), "bytes");
    }

    #[test]
    #[cfg(feature = "encoding")]
    fn builder_parse_raw_too_large() {
        let parser = HtmlParser::builder().max_input_size(5).build();
        let result = parser.parse_raw(b"<p>too large</p>");
        assert!(result.is_err());
    }

    #[test]
    fn static_parse_method() {
        let doc = HtmlParser::parse("<b>bold</b>").unwrap();
        assert_eq!(doc.root().text_content(), "bold");
    }

    #[test]
    #[cfg(feature = "encoding")]
    fn static_parse_bytes_method() {
        let doc = HtmlParser::parse_bytes(b"<i>italic</i>").unwrap();
        assert_eq!(doc.root().text_content(), "italic");
    }

    #[cfg(feature = "css-selector")]
    #[test]
    fn selector_reexport() {
        let doc = HtmlParser::parse("<div><p>Hello</p></div>").unwrap();
        let sel = doc.select("p").unwrap();
        assert_eq!(sel.len(), 1);
    }

    #[test]
    #[cfg(feature = "encoding")]
    fn streaming_reexport() {
        let doc = streaming::parse_stream(b"<p>stream</p>".chunks(4)).unwrap();
        assert_eq!(doc.root().text_content(), "stream");
    }

    #[test]
    fn node_ref_access() {
        let doc = parse("<a href=\"url\">link</a>").unwrap();
        let root = doc.root();
        let a = root.first_child().unwrap();
        assert_eq!(a.tag(), Tag::A);
        assert_eq!(a.attr("href"), Some("url"));
    }

    #[test]
    fn prelude_works() {
        use crate::prelude::*;
        let doc = HtmlParser::parse("<p>prelude</p>").unwrap();
        let _root: NodeRef<'_> = doc.root();
    }
}
