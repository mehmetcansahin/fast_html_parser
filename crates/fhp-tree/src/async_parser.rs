//! Async HTML parser powered by Tokio.
//!
//! Reads from a [`tokio::io::AsyncRead`] source in 64 KB chunks and builds
//! the DOM tree incrementally using [`StreamParser`](crate::streaming::StreamParser).
//!
//! Requires the `async-tokio` feature flag.

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::streaming::StreamParser;
use crate::{Document, HtmlError, MAX_INPUT_SIZE};

/// Default read buffer size (64 KB, page-aligned).
const BUF_SIZE: usize = 64 * 1024;

/// An async HTML parser that reads from a [`tokio::io::AsyncRead`] source.
///
/// # Example
///
/// ```no_run
/// use fhp_tree::async_parser::AsyncParser;
///
/// # async fn example() -> Result<(), fhp_tree::HtmlError> {
/// let html = b"<div><p>Hello</p></div>";
/// let reader = &html[..];
/// let doc = AsyncParser::new(reader).parse().await?;
/// assert_eq!(doc.root().text_content(), "Hello");
/// # Ok(())
/// # }
/// ```
pub struct AsyncParser<R> {
    reader: R,
    buf_size: usize,
    max_input_size: usize,
}

impl<R: AsyncRead + Unpin> AsyncParser<R> {
    /// Create a new async parser reading from `reader`.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf_size: BUF_SIZE,
            max_input_size: MAX_INPUT_SIZE,
        }
    }

    /// Set a custom buffer size for chunk reads.
    pub fn with_buf_size(mut self, size: usize) -> Self {
        self.buf_size = size.max(1);
        self
    }

    /// Set the maximum raw and decoded input size.
    pub fn with_max_input_size(mut self, max_input_size: usize) -> Self {
        self.max_input_size = max_input_size;
        self
    }

    /// Parse the input asynchronously and return the completed document.
    pub async fn parse(mut self) -> Result<Document, HtmlError> {
        let mut parser = StreamParser::with_max_input_size(self.max_input_size);
        let mut buf = vec![0u8; self.buf_size];

        loop {
            let n = self.reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            // A terminal parser error stops reads from the source immediately.
            parser.feed(&buf[..n])?;
        }

        parser.finish()
    }
}

/// Parse HTML asynchronously from an [`AsyncRead`] source.
///
/// Convenience function that creates an [`AsyncParser`] with default settings.
///
/// # Example
///
/// ```no_run
/// use fhp_tree::async_parser::parse_async;
///
/// # async fn example() -> Result<(), fhp_tree::HtmlError> {
/// let html = b"<div><p>Hello</p></div>";
/// let doc = parse_async(&html[..]).await?;
/// assert_eq!(doc.root().text_content(), "Hello");
/// # Ok(())
/// # }
/// ```
pub async fn parse_async<R: AsyncRead + Unpin>(reader: R) -> Result<Document, HtmlError> {
    AsyncParser::new(reader).parse().await
}
