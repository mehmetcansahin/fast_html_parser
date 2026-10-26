//! Encoding detection from raw HTML bytes.
//!
//! Implements a simplified version of the HTML spec's encoding sniffing
//! algorithm: BOM → meta prescan → UTF-8 fallback.

use encoding_rs::Encoding;
use fhp_core::attr::{TagScanState, iter_raw_attrs};
use fhp_core::is_html_whitespace;

/// Number of leading bytes [`detect`] prescans for `<meta>` declarations.
///
/// A caller that buffers the start of a stream for [`detect`] gets the same
/// result as for the complete input once it has buffered this many bytes (or
/// the whole input).
pub const PRESCAN_LIMIT: usize = 1024;

/// Detect the character encoding of raw HTML bytes.
///
/// The detection order is:
/// 1. **BOM** — UTF-8 (`EF BB BF`), UTF-16 LE (`FF FE`), UTF-16 BE (`FE FF`)
/// 2. **`<meta>` prescan** — the first `<meta>` element in the first
///    [`PRESCAN_LIMIT`] bytes that declares an encoding, either with
///    `charset="..."` or with `http-equiv="Content-Type"` and
///    `content="...charset=..."`. Elements are considered in document order;
///    within one element, a `charset` attribute takes precedence over
///    `content`.
/// 3. **Fallback** — UTF-8
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
    if let Some((enc, _)) = Encoding::for_bom(input) {
        return enc;
    }

    // 2. Meta prescan.
    if let Some(enc) = prescan_meta(input) {
        return enc;
    }

    // 3. Fallback.
    encoding_rs::UTF_8
}

/// Prescan the first [`PRESCAN_LIMIT`] bytes for `<meta>` encoding declarations.
///
/// Follows the HTML spec's "prescan a byte stream to determine its encoding"
/// algorithm and looks for two patterns:
/// - `<meta charset="ENCODING">`
/// - `<meta http-equiv="Content-Type" content="...charset=ENCODING...">`
///
/// This is a byte-level (ASCII-oriented) scan, so it does not detect a `<meta>`
/// declaration inside a BOM-less UTF-16 document (where bytes are interleaved
/// with NULs). Such documents are expected to carry a BOM or an HTTP charset;
/// matching browser behaviour, a BOM-less UTF-16 body without one falls back to
/// UTF-8 detection.
fn prescan_meta(input: &[u8]) -> Option<&'static Encoding> {
    let haystack = &input[..input.len().min(PRESCAN_LIMIT)];

    // Walk complete markup so a `<meta`-like byte sequence inside a comment or
    // another tag's quoted attribute value cannot be mistaken for a tag.
    let mut pos = 0;
    while let Some(offset) = memchr::memchr(b'<', &haystack[pos..]) {
        let lt = pos + offset;
        let markup = &haystack[lt..];

        if markup.starts_with(b"<!--") {
            // The opener's dashes may also close the comment, so `<!-->` and
            // `<!--->` are complete comments.
            let Some(end) = memchr::memmem::find(&markup[2..], b"-->") else {
                break;
            };
            pos = lt + 2 + end + 3;
            continue;
        }

        let Some(&after_lt) = markup.get(1) else {
            break;
        };
        // `<meta` followed by whitespace or `/` starts a meta element. This
        // excludes elements such as `<metadata>` and `<metaverse>`.
        let is_meta = starts_with_ci(markup, b"<meta")
            && markup
                .get(5)
                .is_some_and(|&byte| is_html_whitespace(byte) || byte == b'/');
        let is_tag = after_lt.is_ascii_alphabetic()
            || (after_lt == b'/' && markup.get(2).is_some_and(u8::is_ascii_alphabetic));
        let gt = if is_meta {
            find_tag_end(markup, b"<meta".len())
        } else if is_tag {
            tag_name_end(markup).and_then(|name_end| find_tag_end(markup, name_end))
        } else if matches!(after_lt, b'!' | b'/' | b'?') {
            // Declarations, bogus comments, and `</` + non-letter end at the
            // first `>`, quotes included.
            memchr::memchr(b'>', markup)
        } else {
            pos = lt + 1;
            continue;
        };
        let Some(gt) = gt else {
            break;
        };
        pos = lt + gt + 1;

        if is_meta {
            if let Some(enc) = meta_encoding(&markup[b"<meta".len()..gt]) {
                return Some(remap_meta_encoding(enc));
            }
        }
    }
    None
}

/// Encoding declared by one `<meta>` element, given the bytes between `<meta`
/// and its closing `>`.
///
/// A `charset` attribute decides alone: when its value is not an encoding
/// label, the element declares nothing, even if it also carries a
/// `http-equiv="Content-Type"` pragma. Without one, the pragma's `content`
/// is used.
fn meta_encoding(region: &[u8]) -> Option<&'static Encoding> {
    // Legacy bytes in an unrelated attribute value are replaced; charset
    // labels are ASCII.
    let region = String::from_utf8_lossy(region);
    let attributes = MetaAttributes::parse(&region);
    match attributes.charset {
        Some(value) => Encoding::for_label(value?.as_bytes()),
        None => extract_http_equiv_charset(&attributes),
    }
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
///
/// Follows the HTML spec's "extracting a character encoding from a meta
/// element" algorithm: `charset` may appear anywhere, and the first one
/// followed by `=` decides, even when its value is not an encoding label.
fn extract_charset_from_content(content: &[u8]) -> Option<&'static Encoding> {
    const CHARSET: &[u8] = b"charset";

    let mut pos = 0;
    loop {
        pos += content[pos..]
            .windows(CHARSET.len())
            .position(|candidate| candidate.eq_ignore_ascii_case(CHARSET))?
            + CHARSET.len();
        pos += skip_whitespace(&content[pos..]);
        if content.get(pos) != Some(&b'=') {
            // Resume the search at the byte that was not `=`.
            continue;
        }
        pos += 1;
        pos += skip_whitespace(&content[pos..]);

        let value = match content.get(pos)? {
            &quote @ (b'"' | b'\'') => {
                let rest = &content[pos + 1..];
                &rest[..memchr::memchr(quote, rest)?]
            }
            _ => {
                let rest = &content[pos..];
                let end = rest
                    .iter()
                    .position(|&byte| is_html_whitespace(byte) || byte == b';')
                    .unwrap_or(rest.len());
                &rest[..end]
            }
        };
        return Encoding::for_label(value);
    }
}

/// Number of leading HTML whitespace bytes in `bytes`.
fn skip_whitespace(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|&byte| !is_html_whitespace(byte))
        .unwrap_or(bytes.len())
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

/// Index just past the name of the start or end tag beginning at
/// `markup[0] == b'<'`.
///
/// As in the prescan algorithm (and unlike the tokenizer), the name runs to
/// the first whitespace or `>`, so a `/` or a quote is part of the name and
/// cannot open an attribute value.
fn tag_name_end(markup: &[u8]) -> Option<usize> {
    markup
        .iter()
        .position(|&byte| is_html_whitespace(byte) || byte == b'>')
}

/// Index of the `>` that closes a tag whose attributes start at
/// `attrs_start`, following the shared attribute grammar: a `>` inside a
/// quoted attribute value is literal, any other `>` ends the tag.
fn find_tag_end(markup: &[u8], attrs_start: usize) -> Option<usize> {
    let mut state = TagScanState::BeforeAttrName;
    for (index, &byte) in markup.iter().enumerate().skip(attrs_start) {
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

    #[test]
    fn unknown_charset_attribute_does_not_fall_back_to_the_pragma() {
        for charset in ["charset=bogus", "charset", "charset=''"] {
            let input = format!(
                "<meta {charset} http-equiv=Content-Type \
                 content='text/html; charset=windows-1252'>"
            );
            assert_eq!(detect(input.as_bytes()).name(), "UTF-8", "{charset}");
        }
    }

    #[test]
    fn unknown_charset_attribute_does_not_stop_the_prescan() {
        let input = b"<meta charset=bogus><meta charset=windows-1254>";
        assert_eq!(detect(input).name(), "windows-1254");
    }

    #[test]
    fn charset_attribute_wins_over_an_earlier_pragma_in_the_same_element() {
        let input = b"<meta content='text/html; charset=windows-1254' \
                      http-equiv=content-type charset=windows-1252>";
        assert_eq!(detect(input).name(), "windows-1252");
    }

    #[test]
    fn earlier_element_wins_over_a_later_charset_attribute() {
        let input = b"<meta http-equiv=content-type content='text/html; charset=windows-1254'>\
                      <meta charset=windows-1252>";
        assert_eq!(detect(input).name(), "windows-1254");
    }

    #[test]
    fn pragma_without_http_equiv_is_ignored() {
        let input = b"<meta content='text/html; charset=windows-1254'>";
        assert_eq!(detect(input).name(), "UTF-8");
    }

    fn content_charset(content: &str) -> Option<&'static str> {
        extract_charset_from_content(content.as_bytes()).map(Encoding::name)
    }

    #[test]
    fn content_charset_follows_the_spec_algorithm() {
        assert_eq!(
            content_charset("text/html; charset=windows-1254"),
            Some("windows-1254")
        );
        assert_eq!(
            content_charset("text/html;CHARSET = 'windows-1254'"),
            Some("windows-1254")
        );
        assert_eq!(
            content_charset("text/html; charset=\"koi8-r\" x"),
            Some("KOI8-R")
        );
        // `charset` needs no word boundary before it.
        assert_eq!(
            content_charset("text/html,charset=windows-1254"),
            Some("windows-1254")
        );
        assert_eq!(
            content_charset("xcharset=windows-1254"),
            Some("windows-1254")
        );
        // A `charset` not followed by `=` is skipped.
        assert_eq!(
            content_charset("charset windows-1252; charset=windows-1254"),
            Some("windows-1254")
        );
        assert_eq!(
            content_charset("charsetcharset=windows-1254"),
            Some("windows-1254")
        );
        // The first `charset=` decides, even when it is not a label.
        assert_eq!(content_charset("charset=bogus; charset=windows-1254"), None);
        assert_eq!(content_charset("charset='windows-1254"), None);
        assert_eq!(content_charset("charset="), None);
        assert_eq!(content_charset("text/html"), None);
    }

    #[test]
    fn slash_and_quote_in_a_tag_name_do_not_hide_a_later_meta() {
        let input = b"<a/b='><meta charset=windows-1254>";
        assert_eq!(detect(input).name(), "windows-1254");
    }

    #[test]
    fn meta_name_may_end_with_a_slash() {
        assert_eq!(
            detect(b"<meta/charset=windows-1254>").name(),
            "windows-1254"
        );
        assert_eq!(
            detect(b"<META\x0Ccharset=windows-1254>").name(),
            "windows-1254"
        );
    }

    #[test]
    fn quoted_greater_than_inside_meta_is_part_of_the_value() {
        let input = b"<meta name='a>b' charset=windows-1254>";
        assert_eq!(detect(input).name(), "windows-1254");
    }
}
