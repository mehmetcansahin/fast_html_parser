//! Decoding raw bytes to UTF-8 strings.
//!
//! Uses [`encoding_rs`] for the actual byte-to-character conversion.

use encoding_rs::Encoding;
use fhp_core::error::EncodingError;

/// Decode raw bytes using a known encoding.
///
/// Returns the decoded UTF-8 string. If the input is already valid UTF-8 and
/// `encoding` is UTF-8, this avoids copying via [`Cow`](std::borrow::Cow).
///
/// # Errors
///
/// Returns [`EncodingError::MalformedInput`] if the decoder encounters
/// bytes that cannot be mapped to Unicode and the encoding does not use
/// a replacement character (in practice, `encoding_rs` always replaces
/// unmappable bytes, so this is defensive).
///
/// # Example
///
/// ```
/// use fhp_encoding::decode;
/// use encoding_rs::UTF_8;
///
/// let text = decode(b"Hello, world!", UTF_8).unwrap();
/// assert_eq!(text, "Hello, world!");
/// ```
pub fn decode(input: &[u8], encoding: &'static Encoding) -> Result<String, EncodingError> {
    // Strip BOM if present.
    let input = strip_bom(input, encoding);

    let (cow, _actual_encoding, had_errors) = encoding.decode(input);

    if had_errors {
        // Find approximate offset of first malformed byte.
        let offset = find_first_error_offset(input, encoding);
        return Err(EncodingError::MalformedInput {
            encoding: encoding.name(),
            offset,
        });
    }

    Ok(cow.into_owned())
}

/// Auto-detect encoding and decode in one step.
///
/// Combines [`detect`](crate::detect::detect) and [`decode`] for convenience.
///
/// # Errors
///
/// Returns [`EncodingError::MalformedInput`] on decode failure.
///
/// # Example
///
/// ```
/// use fhp_encoding::decode_or_detect;
///
/// let html = b"<html><body>Hello</body></html>";
/// let (text, encoding) = decode_or_detect(html).unwrap();
/// assert!(text.contains("Hello"));
/// assert_eq!(encoding.name(), "UTF-8");
/// ```
pub fn decode_or_detect(input: &[u8]) -> Result<(String, &'static Encoding), EncodingError> {
    let encoding = crate::detect::detect(input);
    let text = decode(input, encoding)?;
    Ok((text, encoding))
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

/// Find the approximate byte offset of the first decoding error.
///
/// Uses a binary-search-like approach: decode prefixes to narrow down the
/// first problematic byte. Falls back to 0 if the error is in the first byte.
fn find_first_error_offset(input: &[u8], encoding: &'static Encoding) -> usize {
    // Simple linear scan with small chunks for accuracy.
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut output = vec![0u8; 1024];
    // Track the byte position of the current chunk explicitly. The decoder's
    // consumed-byte count can lag the chunk position (e.g. an internally
    // buffered partial sequence), so it must not be used to derive `is_last`
    // or the reported offset.
    let mut pos = 0usize;

    for chunk in input.chunks(256) {
        let is_last = pos + chunk.len() >= input.len();
        let (result, read, _) =
            decoder.decode_to_utf8_without_replacement(chunk, &mut output, is_last);
        if let encoding_rs::DecoderResult::Malformed(_, _) = result {
            return pos + read;
        }
        pos += chunk.len();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_utf8() {
        let text = decode(b"Hello, world!", encoding_rs::UTF_8).unwrap();
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn decode_utf8_with_bom() {
        let input = b"\xEF\xBB\xBFHello";
        let text = decode(input, encoding_rs::UTF_8).unwrap();
        assert_eq!(text, "Hello");
    }

    #[test]
    fn decode_latin1() {
        // ISO-8859-1 "café" — encoding_rs maps iso-8859-1 to windows-1252.
        let input = b"caf\xe9";
        let text = decode(input, encoding_rs::WINDOWS_1252).unwrap();
        assert_eq!(text, "caf\u{00e9}");
    }

    #[test]
    fn decode_windows_1252_turkish() {
        // Windows-1252 Turkish characters: ş=0x9F doesn't exist in 1252.
        // In Windows-1254 (Turkish): ş=0xFE, ğ=0xF0, ı=0xFD, ö=0xF6, ü=0xFC, ç=0xE7
        let input: &[u8] = &[0xF6, 0xFC, 0xE7]; // ö, ü, ç in Windows-1254
        let text = decode(input, encoding_rs::WINDOWS_1254).unwrap();
        assert_eq!(text, "\u{00f6}\u{00fc}\u{00e7}"); // ö, ü, ç
    }

    #[test]
    fn decode_or_detect_utf8() {
        let input = b"<html>Hello</html>";
        let (text, enc) = decode_or_detect(input).unwrap();
        assert_eq!(enc.name(), "UTF-8");
        assert!(text.contains("Hello"));
    }

    #[test]
    fn decode_or_detect_with_meta() {
        let input =
            b"<html><head><meta charset=\"windows-1254\"></head><body>\xFE\xF0\xFD</body></html>";
        let (text, enc) = decode_or_detect(input).unwrap();
        assert_eq!(enc.name(), "windows-1254");
        assert!(text.contains('\u{015F}')); // ş
        assert!(text.contains('\u{011F}')); // ğ
        assert!(text.contains('\u{0131}')); // ı
    }

    #[test]
    fn decode_empty() {
        let text = decode(b"", encoding_rs::UTF_8).unwrap();
        assert_eq!(text, "");
    }
}
