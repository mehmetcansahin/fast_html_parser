//! Decoding raw bytes to UTF-8 strings.
//!
//! Uses [`encoding_rs`] for the actual byte-to-character conversion.

use encoding_rs::Encoding;

/// Decode raw bytes using a known encoding.
///
/// A BOM matching `encoding` is stripped; any other leading bytes are decoded
/// with `encoding` as given (the BOM is not re-sniffed, so an explicit
/// encoding is always honored). Malformed byte sequences are replaced with
/// U+FFFD REPLACEMENT CHARACTER, so decoding never fails.
///
/// Returns an owned decoded UTF-8 string. Valid UTF-8 is still copied because
/// the return type owns its storage.
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
    let input = strip_bom(input, encoding);
    encoding.decode_without_bom_handling(input).0.into_owned()
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

/// Strip the BOM prefix if it matches the given encoding.
fn strip_bom<'a>(input: &'a [u8], encoding: &'static Encoding) -> &'a [u8] {
    if encoding == encoding_rs::UTF_8
        && input.len() >= 3
        && input[0] == 0xEF
        && input[1] == 0xBB
        && input[2] == 0xBF
    {
        return &input[3..];
    }
    if encoding == encoding_rs::UTF_16LE && input.len() >= 2 && input[0] == 0xFF && input[1] == 0xFE
    {
        return &input[2..];
    }
    if encoding == encoding_rs::UTF_16BE && input.len() >= 2 && input[0] == 0xFE && input[1] == 0xFF
    {
        return &input[2..];
    }
    input
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
}
