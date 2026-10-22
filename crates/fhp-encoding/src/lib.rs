//! Encoding detection and conversion for the HTML parser.
//!
//! Detects the character encoding of raw HTML bytes and converts them to
//! UTF-8. The detection pipeline follows the HTML specification's
//! [encoding sniffing algorithm](https://html.spec.whatwg.org/multipage/parsing.html#encoding-sniffing-algorithm):
//!
//! 1. BOM (Byte Order Mark) detection
//! 2. `<meta>` prescan of the first 1 KB: the first element that declares an
//!    encoding with `charset="..."` or with `http-equiv="Content-Type"` and
//!    `content="...charset=..."` wins
//! 3. Fallback to UTF-8
//!
//! The actual decoding is delegated to [`encoding_rs`], which is
//! SIMD-optimized by Mozilla/Servo. Decoding never fails: malformed byte
//! sequences are replaced with U+FFFD REPLACEMENT CHARACTER, as browsers do.
//!
//! # Quick Start
//!
//! ```
//! use fhp_encoding::{detect, decode_or_detect};
//!
//! let html = b"<html><head><meta charset=\"utf-8\"></head><body>Hello</body></html>";
//! let encoding = detect(html);
//! assert_eq!(encoding.name(), "UTF-8");
//!
//! let (text, _enc) = decode_or_detect(html);
//! assert!(text.contains("Hello"));
//! ```

/// Decoding raw bytes to UTF-8 strings.
pub mod decode;
/// Encoding detection from raw bytes.
pub mod detect;
/// Streaming decoder for chunk-based processing.
pub mod stream;

pub use decode::{bom_length, decode, decode_cow, decode_or_detect};
pub use detect::{PRESCAN_LIMIT, detect};
pub use encoding_rs::Encoding;
pub use stream::DecodingReader;
