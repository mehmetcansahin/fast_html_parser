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

    // Walk complete tags so a `<meta`-like byte sequence inside a comment or
    // another tag's quoted attribute value cannot be mistaken for markup.
    let mut pos = 0;
    while pos < haystack.len() {
        let Some(lt) = memchr_byte(b'<', &haystack[pos..]) else {
            break;
        };
        let lt = pos + lt;

        if haystack[lt..].starts_with(b"<!--") {
            let comment_body = &haystack[lt + 4..];
            let Some(end) = find_subsequence(comment_body, b"-->") else {
                break;
            };
            pos = lt + 4 + end + 3;
            continue;
        }

        let Some(after_lt) = haystack.get(lt + 1) else {
            break;
        };
        if !after_lt.is_ascii_alphabetic() && !matches!(*after_lt, b'!' | b'/' | b'?') {
            pos = lt + 1;
            continue;
        }

        let Some(gt_offset) = find_tag_end(&haystack[lt + 1..]) else {
            break;
        };
        let gt = lt + 1 + gt_offset;
        let tag = &haystack[lt..=gt];
        pos = gt + 1;

        // Require a real tag-name boundary after "meta". This excludes
        // elements such as `<metadata>` and `<metaverse>`.
        if !starts_with_ci(tag, b"<meta")
            || !tag
                .get(5)
                .is_some_and(|byte| is_html_space(*byte) || matches!(*byte, b'/' | b'>'))
        {
            continue;
        }

        let attributes = parse_meta_attributes(tag);

        // Try <meta charset="...">
        if let Some(enc) = extract_charset_attr(&attributes) {
            return Some(remap_meta_encoding(enc));
        }

        // Try <meta http-equiv="Content-Type" content="...charset=...">
        if let Some(enc) = extract_http_equiv_charset(&attributes) {
            return Some(remap_meta_encoding(enc));
        }
    }
    None
}

#[derive(Default)]
struct MetaAttributes<'a> {
    charset: Option<&'a [u8]>,
    http_equiv: Option<&'a [u8]>,
    content: Option<&'a [u8]>,
}

/// Parse the three attributes relevant to encoding detection using HTML
/// attribute-name and value boundaries.
fn parse_meta_attributes(tag: &[u8]) -> MetaAttributes<'_> {
    let mut attributes = MetaAttributes::default();
    let mut seen_charset = false;
    let mut seen_http_equiv = false;
    let mut seen_content = false;
    let mut pos = b"<meta".len();

    while pos < tag.len() {
        while pos < tag.len() && is_html_space(tag[pos]) {
            pos += 1;
        }
        if pos >= tag.len() || tag[pos] == b'>' {
            break;
        }
        if tag[pos] == b'/' {
            pos += 1;
            continue;
        }

        let name_start = pos;
        while pos < tag.len() && !is_html_space(tag[pos]) && !matches!(tag[pos], b'=' | b'/' | b'>')
        {
            pos += 1;
        }
        let name = &tag[name_start..pos];

        while pos < tag.len() && is_html_space(tag[pos]) {
            pos += 1;
        }
        let value = if pos < tag.len() && tag[pos] == b'=' {
            pos += 1;
            while pos < tag.len() && is_html_space(tag[pos]) {
                pos += 1;
            }
            read_attr_value(tag, &mut pos)
        } else {
            None
        };

        if name.eq_ignore_ascii_case(b"charset") {
            if !seen_charset {
                seen_charset = true;
                attributes.charset = value;
            }
        } else if name.eq_ignore_ascii_case(b"http-equiv") {
            if !seen_http_equiv {
                seen_http_equiv = true;
                attributes.http_equiv = value;
            }
        } else if name.eq_ignore_ascii_case(b"content") && !seen_content {
            seen_content = true;
            attributes.content = value;
        }
    }

    attributes
}

/// Extract encoding from `charset="VALUE"` or `charset=VALUE` in a `<meta>` tag.
fn extract_charset_attr(attributes: &MetaAttributes<'_>) -> Option<&'static Encoding> {
    Encoding::for_label(attributes.charset?)
}

/// Extract encoding from `http-equiv="Content-Type" content="...charset=..."`.
fn extract_http_equiv_charset(attributes: &MetaAttributes<'_>) -> Option<&'static Encoding> {
    if !attributes.http_equiv?.eq_ignore_ascii_case(b"content-type") {
        return None;
    }

    extract_charset_from_content(attributes.content?)
}

/// Extract a charset parameter from an HTTP Content-Type attribute value.
fn extract_charset_from_content(content: &[u8]) -> Option<&'static Encoding> {
    for (index, candidate) in content.windows(b"charset".len()).enumerate() {
        if !candidate.eq_ignore_ascii_case(b"charset") {
            continue;
        }
        if index > 0 && !is_html_space(content[index - 1]) && content[index - 1] != b';' {
            continue;
        }

        let mut pos = index + b"charset".len();
        while pos < content.len() && is_html_space(content[pos]) {
            pos += 1;
        }
        if content.get(pos) != Some(&b'=') {
            continue;
        }
        pos += 1;
        while pos < content.len() && is_html_space(content[pos]) {
            pos += 1;
        }

        let value = if matches!(content.get(pos), Some(b'\'' | b'"')) {
            let quote = content[pos];
            pos += 1;
            let end = memchr_byte(quote, &content[pos..])?;
            &content[pos..pos + end]
        } else {
            let end = content[pos..]
                .iter()
                .position(|byte| is_html_space(*byte) || *byte == b';')
                .unwrap_or(content.len() - pos);
            &content[pos..pos + end]
        };

        if let Some(encoding) = Encoding::for_label(value) {
            return Some(encoding);
        }
    }

    None
}

/// Apply the HTML spec's meta-prescan encoding remap.
///
/// A `<meta>`-declared `utf-16` / `utf-16le` / `utf-16be` label is changed to
/// UTF-8: a document whose `<meta>` tag was found by an ASCII byte scan cannot
/// actually be UTF-16 (UTF-16 interleaves NUL bytes that would have prevented
/// the literal ASCII match), so decoding it as UTF-16 would produce mojibake.
fn remap_meta_encoding(enc: &'static Encoding) -> &'static Encoding {
    if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
        encoding_rs::UTF_8
    } else {
        enc
    }
}

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

/// Simple `memchr`-like byte search (no external dep, just for 1KB prescan).
#[inline]
fn memchr_byte(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// Find the first occurrence of `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Find the closing `>` for a tag while respecting quoted attribute values.
fn find_tag_end(input: &[u8]) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in input.iter().copied().enumerate() {
        match quote {
            Some(expected) if byte == expected => quote = None,
            Some(_) => {}
            None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
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

/// HTML's five ASCII whitespace bytes.
fn is_html_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

/// Read a borrowed attribute value and advance `pos` past it.
fn read_attr_value<'a>(tag: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    if *pos >= tag.len() {
        return None;
    }
    let quote = tag[*pos];
    if quote == b'"' || quote == b'\'' {
        *pos += 1;
        let start = *pos;
        let end = memchr_byte(quote, &tag[start..])?;
        *pos = start + end + 1;
        Some(&tag[start..start + end])
    } else {
        let start = *pos;
        while *pos < tag.len() && !is_html_space(tag[*pos]) && tag[*pos] != b'>' {
            *pos += 1;
        }
        if *pos == start {
            return None;
        }
        Some(&tag[start..*pos])
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

    #[test]
    fn meta_charset_utf16_remaps_to_utf8() {
        // A document that was ASCII-prescannable cannot actually be UTF-16;
        // the HTML spec mandates remapping a meta-declared utf-16 label to UTF-8.
        let input = b"<html><head><meta charset=\"utf-16\"></head><body>Hello</body></html>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn meta_charset_utf16le_remaps_to_utf8() {
        let input = b"<meta charset=\"utf-16le\">Hello";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn meta_charset_utf16be_remaps_to_utf8() {
        let input = b"<meta charset=\"utf-16be\">Hello";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn meta_http_equiv_utf16_remaps_to_utf8() {
        let input =
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-16\">Hello";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn meta_inside_comment_is_ignored() {
        let input = b"<!-- <meta charset=windows-1252> --><p>UTF-8</p>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn metadata_element_is_not_a_meta_element() {
        let input = b"<metadata charset=windows-1252></metadata>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn data_charset_is_not_a_charset_attribute() {
        let input = b"<meta data-charset=windows-1252>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn similarly_named_http_equiv_attributes_do_not_match() {
        let input =
            b"<meta data-http-equiv=content-type data-content='text/html; charset=windows-1252'>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn exact_charset_attribute_matches_among_similar_names() {
        let input = b"<meta data-charset=utf-8 charset=windows-1252>";
        assert_eq!(detect(input).name(), "windows-1252");
    }

    #[test]
    fn duplicate_charset_uses_the_first_attribute() {
        let input = b"<meta charset charset=windows-1252>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn meta_like_text_inside_an_attribute_is_ignored() {
        let input = b"<div title='<meta charset=windows-1252>'></div>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    #[test]
    fn less_than_text_does_not_hide_a_later_meta_element() {
        let input = b"<p>1 < 2</p><meta charset=windows-1252>";
        assert_eq!(detect(input).name(), "windows-1252");
    }
}
