//! Encoding detection from raw HTML bytes.
//!
//! Implements a simplified version of the HTML spec's encoding sniffing
//! algorithm: BOM → meta prescan → UTF-8 fallback.

use encoding_rs::Encoding;
use fhp_core::attr::{TagScanState, iter_raw_attrs, tag_name_offset};
use fhp_core::is_html_whitespace;

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

    // Walk complete markup so a `<meta`-like byte sequence inside a comment or
    // another tag's quoted attribute value cannot be mistaken for a tag.
    let mut pos = 0;
    while pos < haystack.len() {
        let Some(lt) = memchr_byte(b'<', &haystack[pos..]) else {
            break;
        };
        let lt = pos + lt;
        let markup = &haystack[lt..];

        if markup.starts_with(b"<!--") {
            // The opener's dashes may also close the comment, so `<!-->` and
            // `<!--->` are complete comments.
            let Some(end) = find_subsequence(&markup[2..], b"-->") else {
                break;
            };
            pos = lt + 2 + end + 3;
            continue;
        }

        let Some(&after_lt) = markup.get(1) else {
            break;
        };
        let is_tag = after_lt.is_ascii_alphabetic()
            || (after_lt == b'/' && markup.get(2).is_some_and(u8::is_ascii_alphabetic));
        let gt = if is_tag {
            find_tag_end(markup)
        } else if matches!(after_lt, b'!' | b'/' | b'?') {
            // Declarations, bogus comments, and `</` + non-letter end at the
            // first `>`, quotes included.
            memchr_byte(b'>', markup)
        } else {
            pos = lt + 1;
            continue;
        };
        let Some(gt) = gt else {
            break;
        };
        let tag = &markup[..=gt];
        pos = lt + gt + 1;

        // Require a real tag-name boundary after "meta". This excludes
        // elements such as `<metadata>` and `<metaverse>`.
        if !starts_with_ci(tag, b"<meta")
            || !tag
                .get(5)
                .is_some_and(|byte| is_html_whitespace(*byte) || matches!(*byte, b'/' | b'>'))
        {
            continue;
        }

        if let Some(enc) = meta_encoding(tag) {
            return Some(remap_meta_encoding(enc));
        }
    }
    None
}

/// Encoding declared by one complete `<meta ...>` tag.
///
/// Tries `<meta charset="...">` first, then
/// `<meta http-equiv="Content-Type" content="...charset=...">`.
fn meta_encoding(tag: &[u8]) -> Option<&'static Encoding> {
    // The bytes strictly between `<meta` and the closing `>`. Legacy bytes in
    // an unrelated attribute value are replaced; charset labels are ASCII.
    let region = String::from_utf8_lossy(&tag[b"<meta".len()..tag.len() - 1]);
    let attributes = MetaAttributes::parse(&region);
    extract_charset_attr(&attributes).or_else(|| extract_http_equiv_charset(&attributes))
}

/// The first occurrence of each attribute relevant to encoding detection.
/// The outer `Option` records presence; the inner one the (possibly absent)
/// value, so a valueless first occurrence still shadows later duplicates.
#[derive(Default)]
struct MetaAttributes<'a> {
    charset: Option<Option<&'a str>>,
    http_equiv: Option<Option<&'a str>>,
    content: Option<Option<&'a str>>,
}

impl<'a> MetaAttributes<'a> {
    /// Parse a `<meta>` attribute region with the shared HTML attribute
    /// grammar; the first occurrence of each name wins.
    fn parse(region: &'a str) -> Self {
        let mut attributes = Self::default();
        for attr in iter_raw_attrs(region) {
            let slot = if attr.name.eq_ignore_ascii_case("charset") {
                &mut attributes.charset
            } else if attr.name.eq_ignore_ascii_case("http-equiv") {
                &mut attributes.http_equiv
            } else if attr.name.eq_ignore_ascii_case("content") {
                &mut attributes.content
            } else {
                continue;
            };
            slot.get_or_insert(attr.value);
        }
        attributes
    }
}

/// Extract encoding from `charset="VALUE"` or `charset=VALUE` in a `<meta>` tag.
fn extract_charset_attr(attributes: &MetaAttributes<'_>) -> Option<&'static Encoding> {
    Encoding::for_label(attributes.charset.flatten()?.as_bytes())
}

/// Extract encoding from `http-equiv="Content-Type" content="...charset=..."`.
fn extract_http_equiv_charset(attributes: &MetaAttributes<'_>) -> Option<&'static Encoding> {
    if !attributes
        .http_equiv
        .flatten()?
        .eq_ignore_ascii_case("content-type")
    {
        return None;
    }

    extract_charset_from_content(attributes.content.flatten()?.as_bytes())
}

/// Extract a charset parameter from an HTTP Content-Type attribute value.
fn extract_charset_from_content(content: &[u8]) -> Option<&'static Encoding> {
    for (index, candidate) in content.windows(b"charset".len()).enumerate() {
        if !candidate.eq_ignore_ascii_case(b"charset") {
            continue;
        }
        if index > 0 && !is_html_whitespace(content[index - 1]) && content[index - 1] != b';' {
            continue;
        }

        let mut pos = index + b"charset".len();
        while pos < content.len() && is_html_whitespace(content[pos]) {
            pos += 1;
        }
        if content.get(pos) != Some(&b'=') {
            continue;
        }
        pos += 1;
        while pos < content.len() && is_html_whitespace(content[pos]) {
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
                .position(|byte| is_html_whitespace(*byte) || *byte == b';')
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
/// A `<meta>`-declared `x-user-defined` label is changed to windows-1252.
fn remap_meta_encoding(enc: &'static Encoding) -> &'static Encoding {
    if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
        encoding_rs::UTF_8
    } else if enc == encoding_rs::X_USER_DEFINED {
        encoding_rs::WINDOWS_1252
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

/// Index of the `>` that closes the start or end tag beginning at
/// `markup[0] == b'<'`, following the shared tag grammar: a `>` inside a
/// quoted attribute value is literal, any other `>` ends the tag.
fn find_tag_end(markup: &[u8]) -> Option<usize> {
    let mut state = TagScanState::START;
    for (index, &byte) in markup.iter().enumerate().skip(tag_name_offset(markup)) {
        if byte == b'>' && !state.gt_is_literal() {
            return Some(index);
        }
        state = state.step(byte);
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
