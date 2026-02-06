/// Errors that can occur during HTML parsing.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Encountered an unexpected byte during tokenization.
    #[error("unexpected byte 0x{byte:02X} at offset {offset}")]
    UnexpectedByte {
        /// The byte value encountered.
        byte: u8,
        /// The byte offset in the input.
        offset: usize,
    },

    /// The input ended in the middle of a token.
    #[error("unexpected end of input at offset {offset}, state: {state}")]
    UnexpectedEof {
        /// The byte offset where input ended.
        offset: usize,
        /// Description of the tokenizer state when EOF was hit.
        state: &'static str,
    },

    /// A nesting depth limit was exceeded.
    #[error("nesting depth limit exceeded ({depth} > {limit})")]
    NestingTooDeep {
        /// Current depth.
        depth: u32,
        /// Configured limit.
        limit: u32,
    },
}

/// Errors that can occur during CSS selector parsing.
#[derive(Debug, thiserror::Error)]
pub enum SelectorError {
    /// Invalid selector syntax.
    #[error("invalid selector: {reason}")]
    Invalid {
        /// Description of what went wrong.
        reason: String,
    },
}

/// Errors that can occur during XPath expression parsing or evaluation.
#[derive(Debug, thiserror::Error)]
pub enum XPathError {
    /// Invalid XPath syntax.
    #[error("invalid xpath: {reason}")]
    Invalid {
        /// Description of what went wrong.
        reason: String,
    },
}

/// Errors that can occur during encoding detection or conversion.
#[derive(Debug, thiserror::Error)]
pub enum EncodingError {
    /// The decoder encountered bytes that could not be mapped to Unicode.
    #[error("decode error: malformed {encoding} input at byte offset {offset}")]
    MalformedInput {
        /// Name of the encoding being decoded.
        encoding: &'static str,
        /// Approximate byte offset of the first invalid sequence.
        offset: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_display() {
        let err = ParseError::UnexpectedByte {
            byte: 0xFF,
            offset: 42,
        };
        assert_eq!(err.to_string(), "unexpected byte 0xFF at offset 42");
    }

    #[test]
    fn selector_error_display() {
        let err = SelectorError::Invalid {
            reason: "unclosed bracket".to_string(),
        };
        assert_eq!(err.to_string(), "invalid selector: unclosed bracket");
    }

    #[test]
    fn xpath_error_display() {
        let err = XPathError::Invalid {
            reason: "unexpected token".to_string(),
        };
        assert_eq!(err.to_string(), "invalid xpath: unexpected token");
    }

    #[test]
    fn encoding_error_display() {
        let err = EncodingError::MalformedInput {
            encoding: "windows-1252",
            offset: 42,
        };
        assert_eq!(
            err.to_string(),
            "decode error: malformed windows-1252 input at byte offset 42"
        );
    }

    #[test]
    fn nesting_error_display() {
        let err = ParseError::NestingTooDeep {
            depth: 513,
            limit: 512,
        };
        assert_eq!(err.to_string(), "nesting depth limit exceeded (513 > 512)");
    }
}
