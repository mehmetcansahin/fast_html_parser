//! Streaming (chunk-based) tokenizer.
//!
//! [`StreamTokenizer`](crate::streaming::StreamTokenizer) processes input in arbitrary-sized chunks, carrying
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
/// use fhp_tokenizer::streaming::StreamTokenizer;
/// use fhp_tokenizer::token::Token;
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
    /// Reusable working buffer to avoid per-feed allocation.
    working: Vec<u8>,
}

impl StreamTokenizer {
    /// Create a new streaming tokenizer.
    pub fn new() -> Self {
        Self {
            indexer: StructuralIndexer::new(),
            residual: Vec::with_capacity(256),
            working: Vec::with_capacity(4096),
        }
    }

    /// Feed a chunk of UTF-8 input and return any complete tokens.
    ///
    /// Tokens are returned as owned (`'static` lifetime) since the chunk
    /// data may not live long enough. Text content is cloned into `Cow::Owned`.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Token<'static>> {
        if chunk.is_empty() {
            return Vec::new();
        }

        // Combine residual + new chunk into reusable working buffer.
        // Take ownership to avoid borrow conflicts with process_chunk.
        let mut working = std::mem::take(&mut self.working);
        working.clear();
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
                let tokens = self.process_chunk(&working);
                self.working = working;
                return tokens;
            }
            // Swap: working becomes the residual, residual (now empty) becomes the
            // reusable working buffer — no new allocation.
            std::mem::swap(&mut self.residual, &mut working);
            self.working = working;
            return Vec::new();
        }

        // Process the safe portion.
        let tokens = self.process_chunk(&working[..split]);

        // Buffer the rest.
        self.residual.extend_from_slice(&working[split..]);

        // Return the working buffer for reuse.
        self.working = working;

        tokens
    }

    /// Feed a UTF-8 chunk and process complete tokens via callback without cloning.
    ///
    /// This path is intended for internal high-throughput consumers (e.g. tree
    /// building) that can consume tokens immediately.
    pub fn feed_str_with(&mut self, chunk: &str, mut on_token: impl FnMut(&Token<'_>)) {
        if chunk.is_empty() {
            return;
        }

        // Combine residual + new chunk into reusable working buffer.
        let mut working = std::mem::take(&mut self.working);
        working.clear();
        working.extend_from_slice(&self.residual);
        working.extend_from_slice(chunk.as_bytes());
        self.residual.clear();

        let split = find_safe_split(&working);

        if split == 0 {
            if working.len() > MAX_RESIDUAL {
                // Too large to buffer — force-process what we have.
                match std::str::from_utf8(&working) {
                    Ok(text) => {
                        let tokens = self.process_chunk_borrowed(text);
                        for token in &tokens {
                            on_token(token);
                        }
                    }
                    Err(_) => {
                        let text = String::from_utf8_lossy(&working).into_owned();
                        let tokens = self.process_chunk_borrowed(&text);
                        for token in &tokens {
                            on_token(token);
                        }
                    }
                }
                self.working = working;
                return;
            }
            std::mem::swap(&mut self.residual, &mut working);
            self.working = working;
            return;
        }

        match std::str::from_utf8(&working[..split]) {
            Ok(text) => {
                let tokens = self.process_chunk_borrowed(text);
                for token in &tokens {
                    on_token(token);
                }
            }
            Err(_) => {
                let text = String::from_utf8_lossy(&working[..split]).into_owned();
                let tokens = self.process_chunk_borrowed(&text);
                for token in &tokens {
                    on_token(token);
                }
            }
        }

        self.residual.extend_from_slice(&working[split..]);
        self.working = working;
    }

    /// Signal end of input and flush any remaining buffered data.
    pub fn finish(&mut self) -> Vec<Token<'static>> {
        if self.residual.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.residual);
        self.process_chunk(&remaining)
    }

    /// Signal end of input and flush buffered tokens via callback without cloning.
    pub fn finish_with(&mut self, mut on_token: impl FnMut(&Token<'_>)) {
        if self.residual.is_empty() {
            return;
        }
        let remaining = std::mem::take(&mut self.residual);
        match std::str::from_utf8(&remaining) {
            Ok(text) => {
                let tokens = self.process_chunk_borrowed(text);
                for token in &tokens {
                    on_token(token);
                }
            }
            Err(_) => {
                let text = String::from_utf8_lossy(&remaining).into_owned();
                let tokens = self.process_chunk_borrowed(&text);
                for token in &tokens {
                    on_token(token);
                }
            }
        }
    }

    /// Process a complete chunk through the structural indexer + extractor.
    fn process_chunk(&mut self, data: &[u8]) -> Vec<Token<'static>> {
        match std::str::from_utf8(data) {
            Ok(text) => {
                let index = self.indexer.index(text.as_bytes());
                let tokens = extract_tokens(text, &index);
                tokens.into_iter().map(to_owned_token).collect()
            }
            Err(_) => {
                let text = String::from_utf8_lossy(data).into_owned();
                let index = self.indexer.index(text.as_bytes());
                let tokens = extract_tokens(&text, &index);
                tokens.into_iter().map(to_owned_token).collect()
            }
        }
    }

    /// Process a complete UTF-8 chunk and return borrowed tokens.
    fn process_chunk_borrowed<'a>(&mut self, data: &'a str) -> Vec<Token<'a>> {
        let index = self.indexer.index(data.as_bytes());
        extract_tokens(data, &index)
    }
}

impl Default for StreamTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the last safe split point in the buffer.
/// Returns the byte index right after the last completed markup construct.
fn find_safe_split(data: &[u8]) -> usize {
    #[derive(Clone, Copy)]
    enum Mode {
        Data,
        Tag { quote: Option<u8> },
        Doctype { quote: Option<u8> },
        Comment,
        CData,
    }

    let mut mode = Mode::Data;
    let mut i = 0usize;
    let mut last_safe = 0usize;

    while i < data.len() {
        match mode {
            Mode::Data => {
                if data[i] == b'<' {
                    // <!-- ... -->
                    if i + 3 < data.len() && &data[i..i + 4] == b"<!--" {
                        mode = Mode::Comment;
                        i += 4;
                        continue;
                    }

                    // <![CDATA[ ... ]]>
                    if i + 8 < data.len() && &data[i..i + 9] == b"<![CDATA[" {
                        mode = Mode::CData;
                        i += 9;
                        continue;
                    }

                    if i + 1 < data.len() {
                        let next = data[i + 1];
                        // <!DOCTYPE ...> or other <! ... >
                        if next == b'!' {
                            mode = Mode::Doctype { quote: None };
                            i += 2;
                            continue;
                        }
                        // Normal open/close tags. Ignore stray '<' in text.
                        if next == b'/'
                            || next.is_ascii_alphabetic()
                            || next == b'_'
                            || next == b'?'
                        {
                            mode = Mode::Tag { quote: None };
                            i += 1;
                            continue;
                        }
                    }
                }
                i += 1;
            }
            Mode::Tag { mut quote } => {
                if let Some(q) = quote {
                    if data[i] == q {
                        quote = None;
                    }
                    mode = Mode::Tag { quote };
                    i += 1;
                    continue;
                }
                match data[i] {
                    b'"' | b'\'' => {
                        mode = Mode::Tag {
                            quote: Some(data[i]),
                        };
                        i += 1;
                    }
                    b'>' => {
                        last_safe = i + 1;
                        mode = Mode::Data;
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            Mode::Doctype { mut quote } => {
                if let Some(q) = quote {
                    if data[i] == q {
                        quote = None;
                    }
                    mode = Mode::Doctype { quote };
                    i += 1;
                    continue;
                }
                match data[i] {
                    b'"' | b'\'' => {
                        mode = Mode::Doctype {
                            quote: Some(data[i]),
                        };
                        i += 1;
                    }
                    b'>' => {
                        last_safe = i + 1;
                        mode = Mode::Data;
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            Mode::Comment => {
                if i + 2 < data.len()
                    && data[i] == b'-'
                    && data[i + 1] == b'-'
                    && data[i + 2] == b'>'
                {
                    last_safe = i + 3;
                    mode = Mode::Data;
                    i += 3;
                } else {
                    i += 1;
                }
            }
            Mode::CData => {
                if i + 2 < data.len()
                    && data[i] == b']'
                    && data[i + 1] == b']'
                    && data[i + 2] == b'>'
                {
                    last_safe = i + 3;
                    mode = Mode::Data;
                    i += 3;
                } else {
                    i += 1;
                }
            }
        }
    }

    last_safe
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
            name: std::borrow::Cow::Owned(name.into_owned()),
            attributes: attributes.into_iter().map(to_owned_attr).collect(),
            self_closing,
        },
        Token::CloseTag { tag, name } => Token::CloseTag {
            tag,
            name: std::borrow::Cow::Owned(name.into_owned()),
        },
        Token::Text { content } => Token::Text {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::Comment { content } => Token::Comment {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::Doctype { content } => Token::Doctype {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
        Token::CData { content } => Token::CData {
            content: std::borrow::Cow::Owned(content.into_owned()),
        },
    }
}

/// Convert a borrowed attribute to owned.
fn to_owned_attr(attr: crate::token::Attribute<'_>) -> crate::token::Attribute<'static> {
    crate::token::Attribute {
        name: std::borrow::Cow::Owned(attr.name.into_owned()),
        value: attr.value.map(|v| std::borrow::Cow::Owned(v.into_owned())),
    }
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
