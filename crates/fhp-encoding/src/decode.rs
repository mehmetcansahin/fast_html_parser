//! Decoding raw bytes to UTF-8 strings.
//!
//! Uses [`encoding_rs`] for the actual byte-to-character conversion.

use std::borrow::Cow;

use encoding_rs::Encoding;

/// Decode raw bytes using a known encoding.
///
/// A BOM matching `encoding` is stripped; any other leading bytes are decoded
/// with `encoding` as given (the BOM is not re-sniffed, so an explicit
/// encoding is always honored). Malformed byte sequences are replaced with
/// U+FFFD REPLACEMENT CHARACTER, so decoding never fails.
///
/// Returns an owned decoded UTF-8 string. Valid UTF-8 is still copied because
/// the return type owns its storage; use [`decode_cow`] to borrow it instead.
///
/// # Example
///
/// ```
/// use fhp_encoding::decode;
/// use encoding_rs::UTF_8;
///
/// assert_eq!(decode(b"Hello, world!", UTF_8), "Hello, world!");
/// assert_eq!(decode(b"caf\xe9", UTF_8), "caf\u{FFFD}");
/// ```
pub fn decode(input: &[u8], encoding: &'static Encoding) -> String {
    decode_cow(input, encoding).into_owned()
}

/// Decode raw bytes using a known encoding, borrowing the input when it is
/// already valid UTF-8 (or ASCII in an ASCII-compatible encoding).
///
/// Handles BOMs and malformed sequences exactly like [`decode`], but only
/// allocates when the decoded text differs from the input bytes.
///
/// # Example
///
/// ```
/// use std::borrow::Cow;
/// use fhp_encoding::decode_cow;
/// use encoding_rs::{UTF_8, WINDOWS_1252};
///
/// assert!(matches!(decode_cow(b"\xEF\xBB\xBFHello", UTF_8), Cow::Borrowed("Hello")));
/// assert!(matches!(decode_cow(b"caf\xe9", WINDOWS_1252), Cow::Owned(_)));
/// ```
pub fn decode_cow<'a>(input: &'a [u8], encoding: &'static Encoding) -> Cow<'a, str> {
    encoding.decode_with_bom_removal(input).0
}

/// Auto-detect encoding and decode in one step.
///
/// Combines [`detect`](crate::detect::detect) and [`decode`] for convenience.
/// Malformed byte sequences are replaced with U+FFFD.
///
/// # Example
///
/// ```
/// use fhp_encoding::decode_or_detect;
///
/// let html = b"<html><body>Hello</body></html>";
/// let (text, encoding) = decode_or_detect(html);
/// assert!(text.contains("Hello"));
/// assert_eq!(encoding.name(), "UTF-8");
/// ```
pub fn decode_or_detect(input: &[u8]) -> (String, &'static Encoding) {
    let encoding = crate::detect::detect(input);
    (decode(input, encoding), encoding)
}

/// Length of the BOM at the start of `input` if it matches `encoding`, else 0.
///
/// This is the prefix [`decode`] strips. A BOM for a different encoding is
/// not counted, because it is decoded as content with `encoding`.
///
/// # Example
///
/// ```
/// use fhp_encoding::bom_length;
/// use encoding_rs::{UTF_8, UTF_16LE};
///
/// assert_eq!(bom_length(b"\xEF\xBB\xBF<p>", UTF_8), 3);
/// assert_eq!(bom_length(b"\xFF\xFE<\x00", UTF_16LE), 2);
/// assert_eq!(bom_length(b"\xFF\xFE<\x00", UTF_8), 0);
/// ```
pub fn bom_length(input: &[u8], encoding: &'static Encoding) -> usize {
    match Encoding::for_bom(input) {
        Some((bom_encoding, length)) if bom_encoding == encoding => length,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_utf8() {
        let text = decode(b"Hello, world!", encoding_rs::UTF_8);
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn decode_utf8_with_bom() {
        let input = b"\xEF\xBB\xBFHello";
        let text = decode(input, encoding_rs::UTF_8);
        assert_eq!(text, "Hello");
    }

    #[test]
    fn decode_latin1() {
        // ISO-8859-1 "café" — encoding_rs maps iso-8859-1 to windows-1252.
        let input = b"caf\xe9";
        let text = decode(input, encoding_rs::WINDOWS_1252);
        assert_eq!(text, "caf\u{00e9}");
    }

    #[test]
    fn decode_windows_1252_turkish() {
        // Windows-1252 Turkish characters: ş=0x9F doesn't exist in 1252.
        // In Windows-1254 (Turkish): ş=0xFE, ğ=0xF0, ı=0xFD, ö=0xF6, ü=0xFC, ç=0xE7
        let input: &[u8] = &[0xF6, 0xFC, 0xE7]; // ö, ü, ç in Windows-1254
        let text = decode(input, encoding_rs::WINDOWS_1254);
        assert_eq!(text, "\u{00f6}\u{00fc}\u{00e7}"); // ö, ü, ç
    }

    #[test]
    fn decode_or_detect_utf8() {
        let input = b"<html>Hello</html>";
        let (text, enc) = decode_or_detect(input);
        assert_eq!(enc.name(), "UTF-8");
        assert!(text.contains("Hello"));
    }

    #[test]
    fn decode_or_detect_with_meta() {
        let input =
            b"<html><head><meta charset=\"windows-1254\"></head><body>\xFE\xF0\xFD</body></html>";
        let (text, enc) = decode_or_detect(input);
        assert_eq!(enc.name(), "windows-1254");
        assert!(text.contains('\u{015F}')); // ş
        assert!(text.contains('\u{011F}')); // ğ
        assert!(text.contains('\u{0131}')); // ı
    }

    #[test]
    fn decode_empty() {
        let text = decode(b"", encoding_rs::UTF_8);
        assert_eq!(text, "");
    }

    #[test]
    fn decode_replaces_each_malformed_sequence() {
        assert_eq!(
            decode(b"a\xFF\xFEb", encoding_rs::UTF_8),
            "a\u{FFFD}\u{FFFD}b"
        );
    }

    #[test]
    fn decode_honors_the_explicit_encoding_over_a_utf16_bom() {
        assert_eq!(
            decode(b"\xFF\xFEa", encoding_rs::UTF_8),
            "\u{FFFD}\u{FFFD}a"
        );
    }

    #[test]
    fn decode_cow_borrows_unchanged_input() {
        assert!(matches!(
            decode_cow(b"<p>caf\xC3\xA9</p>", encoding_rs::UTF_8),
            Cow::Borrowed("<p>caf\u{e9}</p>")
        ));
        assert!(matches!(
            decode_cow(b"\xEF\xBB\xBFa", encoding_rs::UTF_8),
            Cow::Borrowed("a")
        ));
        assert!(matches!(
            decode_cow(b"abc", encoding_rs::WINDOWS_1254),
            Cow::Borrowed("abc")
        ));
        assert_eq!(decode_cow(b"\xFE", encoding_rs::WINDOWS_1254), "\u{15F}");
        assert_eq!(
            decode_cow(b"\xFF\xFEa", encoding_rs::UTF_8),
            decode(b"\xFF\xFEa", encoding_rs::UTF_8)
        );
    }

    #[test]
    fn bom_length_counts_only_a_matching_bom() {
        assert_eq!(bom_length(b"\xEF\xBB\xBFa", encoding_rs::UTF_8), 3);
        assert_eq!(bom_length(b"\xEF\xBB\xBFa", encoding_rs::WINDOWS_1252), 0);
        assert_eq!(bom_length(b"\xFF\xFEa\x00", encoding_rs::UTF_16LE), 2);
        assert_eq!(bom_length(b"\xFF\xFEa\x00", encoding_rs::UTF_16BE), 0);
        assert_eq!(bom_length(b"\xFE\xFF\x00a", encoding_rs::UTF_16BE), 2);
        assert_eq!(bom_length(b"\xEF\xBB", encoding_rs::UTF_8), 0);
        assert_eq!(bom_length(b"", encoding_rs::UTF_8), 0);
    }
}
