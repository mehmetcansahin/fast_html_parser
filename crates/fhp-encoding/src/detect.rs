//! Encoding detection from raw HTML bytes.
//!
//! Implements a simplified version of the HTML spec's encoding sniffing
//! algorithm: BOM → meta prescan → UTF-8 fallback.

use encoding_rs::Encoding;

/// Maximum number of bytes to prescan for `<meta>` tags.
const PRESCAN_LIMIT: usize = 1024;

/// Detect the character encoding of raw HTML bytes.
///
/// The detection order is:
/// 1. **BOM** — UTF-8 (`EF BB BF`), UTF-16 LE (`FF FE`), UTF-16 BE (`FE FF`)
/// 2. **`<meta charset="...">`** — first occurrence in the first 1 KB
/// 3. **`<meta http-equiv="Content-Type" content="...charset=...">`**
/// 4. **Fallback** — UTF-8
///
/// # Example
///
/// ```
/// use fhp_encoding::detect;
///
/// let html = b"\xEF\xBB\xBF<html>UTF-8 with BOM</html>";
/// assert_eq!(detect(html).name(), "UTF-8");
/// ```
pub fn detect(input: &[u8]) -> &'static Encoding {
    // 1. BOM detection.
    if let Some(enc) = detect_bom(input) {
        return enc;
    }

    // 2–3. Meta prescan.
    if let Some(enc) = prescan_meta(input) {
        return enc;
    }

    // 4. Fallback.
    encoding_rs::UTF_8
}

/// Check for a Byte Order Mark at the start of the input.
fn detect_bom(input: &[u8]) -> Option<&'static Encoding> {
    if input.len() >= 3 && input[0] == 0xEF && input[1] == 0xBB && input[2] == 0xBF {
        return Some(encoding_rs::UTF_8);
    }
    if input.len() >= 2 {
        if input[0] == 0xFF && input[1] == 0xFE {
            return Some(encoding_rs::UTF_16LE);
        }
        if input[0] == 0xFE && input[1] == 0xFF {
            return Some(encoding_rs::UTF_16BE);
        }
    }
    None
}

/// Prescan the first [`PRESCAN_LIMIT`] bytes for `<meta>` encoding declarations.
///
/// Looks for two patterns:
/// - `<meta charset="ENCODING">`
/// - `<meta http-equiv="Content-Type" content="...charset=ENCODING...">`
///
/// This is a byte-level (ASCII-oriented) scan, so it does not detect a `<meta>`
/// declaration inside a BOM-less UTF-16 document (where bytes are interleaved
/// with NULs). Such documents are expected to carry a BOM or an HTTP charset;
/// matching browser behaviour, a BOM-less UTF-16 body without one falls back to
/// UTF-8 detection.
fn prescan_meta(input: &[u8]) -> Option<&'static Encoding> {
    let limit = input.len().min(PRESCAN_LIMIT);
    let haystack = &input[..limit];

    // Find each '<' and check if it starts a <meta tag.
    let mut pos = 0;
    while pos < haystack.len() {
        // Find next '<'.
        let Some(lt) = memchr_byte(b'<', &haystack[pos..]) else {
            break;
        };
        let lt = pos + lt;
        pos = lt + 1;

        // Check for "<meta" (case-insensitive).
        if !starts_with_ci(&haystack[lt..], b"<meta") {
            continue;
        }

        // Extract everything up to the closing '>'.
        let tag_start = lt;
        let Some(gt_offset) = memchr_byte(b'>', &haystack[tag_start..]) else {
            break;
        };
        let tag_bytes = &haystack[tag_start..tag_start + gt_offset + 1];
        pos = tag_start + gt_offset + 1;

        // Try <meta charset="...">
        if let Some(enc) = extract_charset_attr(tag_bytes) {
            return Some(enc);
        }

        // Try <meta http-equiv="Content-Type" content="...charset=...">
        if let Some(enc) = extract_http_equiv_charset(tag_bytes) {
            return Some(enc);
        }
    }
    None
}

/// Extract encoding from `charset="VALUE"` or `charset=VALUE` in a `<meta>` tag.
fn extract_charset_attr(tag: &[u8]) -> Option<&'static Encoding> {
    let charset_needle = b"charset";

    let idx = find_subsequence_ci(tag, charset_needle)?;
    let rest = &tag[idx + charset_needle.len()..];

    // Skip whitespace and '='.
    let rest = skip_ws(rest);
    if rest.first() != Some(&b'=') {
        return None;
    }
    let rest = skip_ws(&rest[1..]);

    // Read the value (quoted or unquoted).
    let value = read_attr_value(rest)?;
    Encoding::for_label(value.as_bytes())
}

/// Extract encoding from `http-equiv="Content-Type" content="...charset=..."`.
fn extract_http_equiv_charset(tag: &[u8]) -> Option<&'static Encoding> {
    // Must have http-equiv="content-type".
    if !contains_subsequence_ci(tag, b"http-equiv") {
        return None;
    }
    if !contains_subsequence_ci(tag, b"content-type") {
        return None;
    }

    // Find the `content` attribute — skip occurrences inside "content-type".
    // We look for "content" followed by optional whitespace then "=".
    let content_needle = b"content";
    let mut search_start = 0;
    let content_value = loop {
        let idx = find_subsequence_ci(&tag[search_start..], content_needle)?;
        let abs_idx = search_start + idx;
        let after = &tag[abs_idx + content_needle.len()..];
        let after = skip_ws(after);
        if after.first() == Some(&b'=') {
            let rest = skip_ws(&after[1..]);
            break read_attr_value(rest)?;
        }
        // Not followed by '=', skip past and try again.
        search_start = abs_idx + content_needle.len();
    };

    // Find charset= inside the content value.
    let cv_lower: String = content_value.to_ascii_lowercase();
    let charset_pos = cv_lower.find("charset=")?;
    let enc_str = &cv_lower[charset_pos + 8..];
    // Trim trailing ';' or whitespace.
    let enc_str = enc_str.split(';').next().unwrap_or("").trim();

    Encoding::for_label(enc_str.as_bytes())
}

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

/// Simple `memchr`-like byte search (no external dep, just for 1KB prescan).
#[inline]
fn memchr_byte(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// Case-insensitive prefix check.
fn starts_with_ci(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    haystack[..needle.len()]
        .iter()
        .zip(needle)
        .all(|(&a, &b)| a.eq_ignore_ascii_case(&b))
}

/// Find the first occurrence of `needle` in `haystack` (case-insensitive).
fn find_subsequence_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

/// Check if `haystack` contains `needle` (case-insensitive).
fn contains_subsequence_ci(haystack: &[u8], needle: &[u8]) -> bool {
    find_subsequence_ci(haystack, needle).is_some()
}

/// Skip leading ASCII whitespace.
fn skip_ws(input: &[u8]) -> &[u8] {
    let start = input
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(input.len());
    &input[start..]
}

/// Read an attribute value — either quoted (`"..."` / `'...'`) or bare token.
fn read_attr_value(input: &[u8]) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    let quote = input[0];
    if quote == b'"' || quote == b'\'' {
        let end = memchr_byte(quote, &input[1..])?;
        let value = &input[1..1 + end];
        Some(String::from_utf8_lossy(value).into_owned())
    } else {
        // Bare token: ends at whitespace, '>', '/', or ';'.
        let end = input
            .iter()
            .position(|&b| b.is_ascii_whitespace() || b == b'>' || b == b'/' || b == b';')
            .unwrap_or(input.len());
        if end == 0 {
            return None;
        }
        Some(String::from_utf8_lossy(&input[..end]).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_utf8() {
        let input = b"\xEF\xBB\xBF<html></html>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn bom_utf16le() {
        let input = b"\xFF\xFE<\x00h\x00t\x00m\x00l\x00";
        assert_eq!(detect(input).name(), "UTF-16LE");
    }

    #[test]
    fn bom_utf16be() {
        let input = b"\xFE\xFF\x00<\x00h\x00t\x00m\x00l";
        assert_eq!(detect(input).name(), "UTF-16BE");
    }

    #[test]
    fn meta_charset_double_quote() {
        let input = b"<html><head><meta charset=\"windows-1252\"></head></html>";
        assert_eq!(detect(input).name(), "windows-1252");
    }

    #[test]
    fn meta_charset_single_quote() {
        let input = b"<html><head><meta charset='iso-8859-1'></head></html>";
        assert_eq!(detect(input).name(), "windows-1252"); // encoding_rs maps iso-8859-1 → windows-1252
    }

    #[test]
    fn meta_charset_case_insensitive() {
        let input = b"<HTML><HEAD><META CHARSET=\"UTF-8\"></HEAD></HTML>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn meta_http_equiv() {
        let input = b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1254\"></head></html>";
        assert_eq!(detect(input).name(), "windows-1254");
    }

    #[test]
    fn fallback_utf8() {
        let input = b"<html><head></head><body>Hello</body></html>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn empty_input() {
        assert_eq!(detect(b"").name(), "UTF-8");
    }

    #[test]
    fn no_meta_in_first_1kb() {
        // Put meta after 1KB — should not be detected.
        let mut input = vec![b' '; 1100];
        let meta = b"<meta charset=\"iso-8859-1\">";
        input.extend_from_slice(meta);
        assert_eq!(detect(&input).name(), "UTF-8"); // fallback
    }

    #[test]
    fn meta_charset_bare_value() {
        let input = b"<meta charset=utf-8>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn bom_takes_priority_over_meta() {
        // UTF-8 BOM but meta says windows-1252. BOM wins.
        let input = b"\xEF\xBB\xBF<html><head><meta charset=\"windows-1252\"></head></html>";
        assert_eq!(detect(input).name(), "UTF-8");
    }
}
