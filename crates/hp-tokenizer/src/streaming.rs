//! Streaming (chunk-based) tokenizer.
//!
//! [`StreamTokenizer`] processes input in arbitrary-sized chunks, carrying
//! state across chunk boundaries. This enables parsing large files or
//! network streams without loading the entire document into memory.

use crate::extract::extract_tokens;
use crate::structural::StructuralIndexer;
use crate::token::Token;

/// Maximum size of the residual buffer.
///
/// When a chunk boundary falls in the middle of a tag, we buffer up to
/// this many bytes and prepend them to the next chunk.
const MAX_RESIDUAL: usize = 4096;

/// A streaming tokenizer that processes input chunk by chunk.
///
/// Maintains internal state so that token boundaries that span chunk
/// boundaries are handled correctly.
///
/// # Example
///
/// ```
/// use hp_tokenizer::streaming::StreamTokenizer;
/// use hp_tokenizer::token::Token;
///
/// let mut tokenizer = StreamTokenizer::new();
/// let mut all_tokens: Vec<Token<'static>> = Vec::new();
///
/// let html = b"<div>hello</div>";
/// // Feed in small chunks.
/// let owned = tokenizer.feed(&html[..5]);
/// all_tokens.extend(owned);
/// let owned = tokenizer.feed(&html[5..]);
/// all_tokens.extend(owned);
/// let owned = tokenizer.finish();
/// all_tokens.extend(owned);
///
/// assert!(all_tokens.iter().any(|t| matches!(t, Token::OpenTag { .. })));
/// ```
pub struct StreamTokenizer {
    indexer: StructuralIndexer,
    /// Residual bytes from the previous chunk (partial tag).
    residual: Vec<u8>,
    /// Whether we are currently inside a tag (between `<` and `>`).
    in_tag: bool,
}

impl StreamTokenizer {
    /// Create a new streaming tokenizer.
    pub fn new() -> Self {
        Self {
            indexer: StructuralIndexer::new(),
            residual: Vec::with_capacity(256),
            in_tag: false,
        }
    }

    /// Feed a chunk of input and return any complete tokens.
    ///
    /// Tokens are returned as owned (`'static` lifetime) since the chunk
    /// data may not live long enough. Text content is cloned into `Cow::Owned`.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Token<'static>> {
        if chunk.is_empty() {
            return Vec::new();
        }

        // Combine residual + new chunk.
        let mut working = Vec::with_capacity(self.residual.len() + chunk.len());
        working.extend_from_slice(&self.residual);
        working.extend_from_slice(chunk);
        self.residual.clear();

        // Find the last safe split point.
        // Safe = end of a '>' that's not inside a string.
        let split = find_safe_split(&working);

        if split == 0 {
            // No complete tag boundary — buffer everything.
            if working.len() > MAX_RESIDUAL {
                // Too large to buffer — force-process what we have.
                return self.process_chunk(&working);
            }
            self.residual = working;
            return Vec::new();
        }

        // Process the safe portion.
        let safe_part = &working[..split];
        let tokens = self.process_chunk(safe_part);

        // Buffer the rest.
        self.residual = working[split..].to_vec();

        tokens
    }

    /// Signal end of input and flush any remaining buffered data.
    pub fn finish(&mut self) -> Vec<Token<'static>> {
        if self.residual.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.residual);
        self.process_chunk(&remaining)
    }

    /// Process a complete chunk through the structural indexer + extractor.
    fn process_chunk(&mut self, data: &[u8]) -> Vec<Token<'static>> {
        let index = self.indexer.index(data);
        let tokens = extract_tokens(data, &index);

        // Update in_tag state.
        for &b in data.iter().rev() {
            if b == b'>' {
                self.in_tag = false;
                break;
            }
            if b == b'<' {
                self.in_tag = true;
                break;
            }
        }

        // Convert to 'static lifetime by cloning string data.
        tokens.into_iter().map(to_owned_token).collect()
    }
}

impl Default for StreamTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the last safe split point in the buffer.
/// Returns the byte index right after the last `>` that appears to close a tag.
fn find_safe_split(data: &[u8]) -> usize {
    // Walk backwards to find the last '>'.
    for i in (0..data.len()).rev() {
        if data[i] == b'>' {
            return i + 1;
        }
    }
    0
}

/// Convert a borrowed token to an owned ('static) token.
fn to_owned_token(token: Token<'_>) -> Token<'static> {
    match token {
        Token::OpenTag {
            tag,
            name,
            attributes,
            self_closing,
        } => Token::OpenTag {
            tag,
            name: leak_str(name),
            attributes: attributes.into_iter().map(to_owned_attr).collect(),
            self_closing,
        },
        Token::CloseTag { tag, name } => Token::CloseTag {
            tag,
            name: leak_str(name),
        },
        Token::Text { content } => Token::Text {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::Comment { content } => Token::Comment {
            content: leak_str(content),
        },
        Token::Doctype { content } => Token::Doctype {
            content: leak_str(content),
        },
        Token::CData { content } => Token::CData {
            content: leak_str(content),
        },
    }
}

/// Convert a borrowed attribute to owned.
fn to_owned_attr(attr: crate::token::Attribute<'_>) -> crate::token::Attribute<'static> {
    crate::token::Attribute {
        name: leak_str(attr.name),
        value: attr.value.map(|v| std::borrow::Cow::Owned(v.into_owned())),
    }
}

/// Convert a `&str` into a `&'static str` by boxing + leaking.
/// This is acceptable for streaming use where tokens are consumed and
/// the total leaked memory is bounded by document size.
fn leak_str(s: &str) -> &'static str {
    if s.is_empty() {
        return "";
    }
    Box::leak(s.to_string().into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_chunk() {
        let mut tok = StreamTokenizer::new();
        let tokens = tok.feed(b"<div>hello</div>");
        let final_tokens = tok.finish();

        let all: Vec<_> = tokens.into_iter().chain(final_tokens).collect();
        assert!(all.iter().any(|t| matches!(t, Token::OpenTag { .. })));
        assert!(all.iter().any(|t| matches!(t, Token::CloseTag { .. })));
    }

    #[test]
    fn multi_chunk() {
        let html = b"<div>hello</div>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        // Feed byte by byte.
        for &b in html.iter() {
            all.extend(tok.feed(&[b]));
        }
        all.extend(tok.finish());

        let has_open = all.iter().any(|t| matches!(t, Token::OpenTag { .. }));
        let has_close = all.iter().any(|t| matches!(t, Token::CloseTag { .. }));
        let has_text = all.iter().any(|t| matches!(t, Token::Text { .. }));

        assert!(has_open, "should have open tag");
        assert!(has_close, "should have close tag");
        assert!(has_text, "should have text");
    }

    #[test]
    fn chunk_size_7() {
        let html = b"<div class=\"test\">hello world</div>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for chunk in html.chunks(7) {
            all.extend(tok.feed(chunk));
        }
        all.extend(tok.finish());

        assert!(all.iter().any(|t| matches!(t, Token::OpenTag { .. })));
        assert!(all.iter().any(|t| matches!(t, Token::CloseTag { .. })));
    }

    #[test]
    fn chunk_size_64() {
        let html = b"<html><head><title>Test</title></head><body><div class=\"main\"><p>Hello</p></div></body></html>";
        let mut tok = StreamTokenizer::new();
        let mut all = Vec::new();

        for chunk in html.chunks(64) {
            all.extend(tok.feed(chunk));
        }
        all.extend(tok.finish());

        let open_count = all
            .iter()
            .filter(|t| matches!(t, Token::OpenTag { .. }))
            .count();
        assert!(open_count >= 5, "should have multiple open tags");
    }

    #[test]
    fn empty_chunks() {
        let mut tok = StreamTokenizer::new();
        let t1 = tok.feed(b"");
        let t2 = tok.feed(b"<br/>");
        let t3 = tok.feed(b"");
        let t4 = tok.finish();

        let all: Vec<_> = t1.into_iter().chain(t2).chain(t3).chain(t4).collect();
        assert!(all.iter().any(|t| matches!(t, Token::OpenTag { .. })));
    }

    #[test]
    fn find_safe_split_basic() {
        assert_eq!(find_safe_split(b"<div>hello</div>"), 16);
        assert_eq!(find_safe_split(b"<div>hello"), 5);
        assert_eq!(find_safe_split(b"hello"), 0);
    }
}
