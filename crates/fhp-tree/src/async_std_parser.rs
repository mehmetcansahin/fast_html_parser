//! Async HTML parser powered by async-std.
//!
//! Reads from an [`async_std::io::Read`] source in 64 KiB chunks and builds
//! the DOM incrementally using [`StreamParser`](crate::streaming::StreamParser).
//!
//! Requires the `async-async-std` feature flag.

use async_std::io::{Read, ReadExt};

use crate::streaming::StreamParser;
use crate::{Document, HtmlError, MAX_INPUT_SIZE};

/// Default read buffer size (64 KiB).
const BUF_SIZE: usize = 64 * 1024;

/// An async HTML parser that reads from an async-std [`Read`] source.
///
/// # Example
///
/// ```no_run
/// use fhp_tree::async_std_parser::AsyncStdParser;
///
/// # async fn example() -> Result<(), fhp_tree::HtmlError> {
/// let html = b"<div><p>Hello</p></div>";
/// let doc = AsyncStdParser::new(&html[..]).parse().await?;
/// assert_eq!(doc.root().text_content(), "Hello");
/// # Ok(())
/// # }
/// ```
pub struct AsyncStdParser<R> {
    reader: R,
    buf_size: usize,
    max_input_size: usize,
}

impl<R: Read + Unpin> AsyncStdParser<R> {
    /// Create a new async-std parser reading from `reader`.
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
            let read = self.reader.read(&mut buf).await?;
            if read == 0 {
                break;
            }
            // A terminal parser error stops reads from the source immediately.
            parser.feed(&buf[..read])?;
        }

        parser.finish()
    }
}

/// Parse HTML asynchronously from an async-std [`Read`] source.
pub async fn parse_async_std<R: Read + Unpin>(reader: R) -> Result<Document, HtmlError> {
    AsyncStdParser::new(reader).parse().await
}
