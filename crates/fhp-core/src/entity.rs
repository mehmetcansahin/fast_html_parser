mod generated {
    include!("generated_entities.rs");
}

/// Pinned WHATWG source used to generate the named-entity lookup tables.
pub const NAMED_ENTITY_SOURCE_URL: &str = generated::ENTITY_SOURCE_URL;

/// SHA-256 of the vendored WHATWG entity source.
pub const NAMED_ENTITY_SOURCE_SHA256: &str = generated::ENTITY_SOURCE_SHA256;

/// Number of records in the pinned WHATWG entity source.
pub const NAMED_ENTITY_SOURCE_RECORD_COUNT: usize = generated::ENTITY_SOURCE_RECORD_COUNT;

/// A terminal reached while matching a named character reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedEntityMatch {
    /// Replacement Unicode scalar sequence.
    pub value: &'static str,
    /// Number of entity-name bytes consumed, excluding `&` and `;`.
    pub name_len: usize,
    /// Whether WHATWG permits this name without a trailing semicolon.
    pub allows_legacy_omission: bool,
}

#[inline]
fn legacy_trie_transition(node_index: usize, byte: u8) -> Option<usize> {
    if node_index == 0 {
        let next = generated::LEGACY_ENTITY_TRIE_ROOT[byte as usize];
        return (next != generated::NO_ENTITY_NODE).then_some(next as usize);
    }

    let node = generated::LEGACY_ENTITY_TRIE_NODES.get(node_index)?;
    let start = node.first_edge as usize;
    let end = start + node.edge_count as usize;
    let edges = &generated::LEGACY_ENTITY_TRIE_EDGES[start..end];
    match edges {
        [] => None,
        [edge] => (edge.byte == byte).then_some(edge.next as usize),
        [first, second] => {
            if first.byte == byte {
                Some(first.next as usize)
            } else {
                (second.byte == byte).then_some(second.next as usize)
            }
        }
        _ => edges
            .iter()
            .find(|edge| edge.byte == byte)
            .map(|edge| edge.next as usize),
    }
}

/// Find the longest named-reference prefix after an ampersand.
///
/// `input` starts with the first name byte and may contain a trailing
/// semicolon or ordinary text. The returned length excludes the semicolon.
#[inline]
pub fn longest_named_prefix(input: &[u8]) -> Option<NamedEntityMatch> {
    let mut candidate_len = 0usize;
    let mut legacy_node_index = Some(0usize);
    let mut legacy_match = None;

    for (offset, &byte) in input
        .iter()
        .take(generated::MAX_ENTITY_NAME_LEN)
        .enumerate()
    {
        if !byte.is_ascii_alphanumeric() {
            break;
        }
        candidate_len = offset + 1;

        // Exact references are looked up once after the name terminator is
        // known. While scanning that same name, retain the longest legacy
        // terminal for the semicolon-less and exact-miss fallback paths.
        if offset < generated::MAX_LEGACY_ENTITY_NAME_LEN {
            legacy_node_index =
                legacy_node_index.and_then(|node_index| legacy_trie_transition(node_index, byte));
            if let Some(node_index) = legacy_node_index {
                let node = generated::LEGACY_ENTITY_TRIE_NODES[node_index];
                if node.value_index != generated::NO_ENTITY_VALUE {
                    legacy_match = Some(NamedEntityMatch {
                        value: generated::LEGACY_ENTITY_VALUES[node.value_index as usize],
                        name_len: candidate_len,
                        allows_legacy_omission: true,
                    });
                }
            }
        } else {
            legacy_node_index = None;
        }
    }

    if candidate_len != 0 && input.get(candidate_len) == Some(&b';') {
        // The scan above only admits ASCII alphanumerics, so this conversion
        // cannot fail. Keep it checked to avoid introducing unsafe code into
        // the core crate.
        let name = std::str::from_utf8(&input[..candidate_len]).ok()?;
        if let Some(value) = decode_named(name) {
            return Some(NamedEntityMatch {
                value,
                name_len: candidate_len,
                allows_legacy_omission: legacy_match
                    .is_some_and(|matched| matched.name_len == candidate_len),
            });
        }
    }

    legacy_match
}

/// Decode a named HTML entity to its character(s).
///
/// Accepts the entity name **without** the leading `&` and trailing `;`.
/// Returns `None` for unknown entity names.
///
/// # Examples
///
/// ```
/// use fhp_core::entity::decode_named;
///
/// assert_eq!(decode_named("amp"), Some("&"));
/// assert_eq!(decode_named("lt"), Some("<"));
/// assert_eq!(decode_named("nonexistent"), None);
/// ```
#[inline]
pub fn decode_named(name: &str) -> Option<&'static str> {
    if name.is_empty() || name.len() > generated::MAX_ENTITY_NAME_LEN {
        return None;
    }

    // These six references dominate real-world HTML and tokenizer hot paths.
    // Keep the complete PHF as the canonical exact table, but avoid SipHash
    // setup for the smallest, most frequent spellings.
    match name {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        "nbsp" => Some("\u{00A0}"),
        _ => generated::EXACT_ENTITIES.get(name).copied(),
    }
}

/// Decode a numeric character reference (`&#123;` or `&#x1F600;`).
///
/// Accepts the digits **without** `&#`, `&#x`, or the trailing `;`.
/// `is_hex` indicates whether the reference uses hexadecimal.
///
/// Applies HTML's numeric-reference corrections: null, surrogate, out-of-range,
/// and overflowing values become U+FFFD, while C1 controls use the Windows-1252
/// replacement table. Returns `None` only when `digits` is empty or contains a
/// digit that is invalid for the selected radix.
///
/// # Examples
///
/// ```
/// use fhp_core::entity::decode_numeric;
///
/// assert_eq!(decode_numeric("60", false), Some('<'));
/// assert_eq!(decode_numeric("3C", true), Some('<'));
/// assert_eq!(decode_numeric("0", false), Some('\u{FFFD}'));
/// ```
pub fn decode_numeric(digits: &str, is_hex: bool) -> Option<char> {
    if digits.is_empty() {
        return None;
    }

    let radix = if is_hex { 16 } else { 10 };
    let mut codepoint = 0u32;
    for byte in digits.bytes() {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' if is_hex => u32::from(byte - b'a') + 10,
            b'A'..=b'F' if is_hex => u32::from(byte - b'A') + 10,
            _ => return None,
        };
        codepoint = match codepoint
            .checked_mul(radix)
            .and_then(|value| value.checked_add(digit))
        {
            Some(value) => value,
            None => return Some('\u{FFFD}'),
        };
    }

    if codepoint == 0 || codepoint > 0x10_FFFF || (0xD800..=0xDFFF).contains(&codepoint) {
        return Some('\u{FFFD}');
    }

    let codepoint = match codepoint {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => codepoint,
    };

    char::from_u32(codepoint)
}

/// Escape HTML text content: `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`.
///
/// Writes the escaped output into `out`. Unescaped segments are flushed in
/// bulk for performance — only special characters cause a pause.
///
/// # Examples
///
/// ```
/// use fhp_core::entity::escape_text;
///
/// let mut buf = String::new();
/// escape_text("1 < 2 & 3 > 0", &mut buf);
/// assert_eq!(buf, "1 &lt; 2 &amp; 3 &gt; 0");
/// ```
#[inline]
pub fn escape_text(input: &str, out: &mut String) {
    escape_impl::<false>(input, out);
}

/// Escape HTML attribute values: `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`, `"` → `&quot;`, `'` → `&#39;`.
///
/// Writes the escaped output into `out`. Like [`escape_text`], unescaped
/// segments are flushed in bulk.
///
/// # Examples
///
/// ```
/// use fhp_core::entity::escape_attr;
///
/// let mut buf = String::new();
/// escape_attr("x&y=\"z\"", &mut buf);
/// assert_eq!(buf, "x&amp;y=&quot;z&quot;");
/// ```
#[inline]
pub fn escape_attr(input: &str, out: &mut String) {
    escape_impl::<true>(input, out);
}

/// Shared escape implementation. When `ESCAPE_QUOTES` is true, `"` and `'`
/// are also escaped (for attribute values).
#[inline(always)]
fn escape_impl<const ESCAPE_QUOTES: bool>(input: &str, out: &mut String) {
    out.reserve(input.len());

    let bytes = input.as_bytes();
    let mut last = 0;

    for (i, &b) in bytes.iter().enumerate() {
        let replacement = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' if ESCAPE_QUOTES => "&quot;",
            b'\'' if ESCAPE_QUOTES => "&#39;",
            _ => continue,
        };

        out.push_str(&input[last..i]);
        out.push_str(replacement);
        last = i + 1;
    }

    out.push_str(&input[last..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_named_entities() {
        assert_eq!(decode_named("amp"), Some("&"));
        assert_eq!(decode_named("lt"), Some("<"));
        assert_eq!(decode_named("gt"), Some(">"));
        assert_eq!(decode_named("quot"), Some("\""));
        assert_eq!(decode_named("apos"), Some("'"));
        assert_eq!(decode_named("nbsp"), Some("\u{00A0}"));
    }

    #[test]
    fn unknown_entity() {
        assert_eq!(decode_named("nonexistent"), None);
        assert_eq!(decode_named(""), None);
    }

    #[test]
    fn complete_whatwg_entity_data_is_available() {
        assert_eq!(NAMED_ENTITY_SOURCE_RECORD_COUNT, 2231);
        assert_eq!(decode_named("CounterClockwiseContourIntegral"), Some("∳"));
        assert_eq!(decode_named("NotEqualTilde"), Some("≂̸"));
        assert_eq!(decode_named("Afr"), Some("𝔄"));
    }

    #[test]
    fn every_generated_exact_entity_decodes() {
        assert_eq!(
            generated::EXACT_ENTITIES.len(),
            generated::EXACT_ENTITY_COUNT
        );
        assert_eq!(
            generated::EXACT_ENTITY_COUNT + generated::LEGACY_ENTITY_COUNT,
            NAMED_ENTITY_SOURCE_RECORD_COUNT
        );

        for (name, expected) in generated::EXACT_ENTITIES.entries() {
            assert_eq!(decode_named(name), Some(*expected), "exact entity {name}");

            let input = format!("{name};");
            let matched = longest_named_prefix(input.as_bytes()).unwrap();
            assert_eq!(matched.value, *expected, "exact entity {name}");
            assert_eq!(matched.name_len, name.len(), "exact entity {name}");
            assert_eq!(
                matched.allows_legacy_omission,
                generated::LEGACY_ENTITY_NAMES.binary_search(name).is_ok(),
                "exact entity {name}"
            );
        }
    }

    #[test]
    fn every_generated_legacy_entity_matches_without_semicolon() {
        assert_eq!(
            generated::LEGACY_ENTITY_NAMES.len(),
            generated::LEGACY_ENTITY_COUNT
        );

        for name in generated::LEGACY_ENTITY_NAMES {
            let expected = decode_named(name).unwrap();
            let input = format!("{name}!");
            let matched = longest_named_prefix(input.as_bytes()).unwrap();
            assert_eq!(matched.value, expected, "legacy entity {name}");
            assert_eq!(matched.name_len, name.len(), "legacy entity {name}");
            assert!(matched.allows_legacy_omission, "legacy entity {name}");
        }
    }

    #[test]
    fn longest_prefix_reports_legacy_omission() {
        let matched = longest_named_prefix(b"notin;").unwrap();
        assert_eq!(matched.value, "∉");
        assert_eq!(matched.name_len, 5);
        assert!(!matched.allows_legacy_omission);

        let matched = longest_named_prefix(b"notit;").unwrap();
        assert_eq!(matched.value, "¬");
        assert_eq!(matched.name_len, 3);
        assert!(matched.allows_legacy_omission);

        let matched = longest_named_prefix(b"notin").unwrap();
        assert_eq!(matched.value, "¬");
        assert_eq!(matched.name_len, 3);
        assert!(matched.allows_legacy_omission);

        let matched = longest_named_prefix(b"NotEqualTilde;").unwrap();
        assert_eq!(matched.value, "≂̸");
        assert!(!matched.allows_legacy_omission);
    }

    #[test]
    fn numeric_decimal() {
        assert_eq!(decode_numeric("60", false), Some('<'));
        assert_eq!(decode_numeric("62", false), Some('>'));
        assert_eq!(decode_numeric("38", false), Some('&'));
        assert_eq!(decode_numeric("128512", false), Some('\u{1F600}'));
    }

    #[test]
    fn numeric_hex() {
        assert_eq!(decode_numeric("3C", true), Some('<'));
        assert_eq!(decode_numeric("3e", true), Some('>'));
        assert_eq!(decode_numeric("1F600", true), Some('\u{1F600}'));
    }

    #[test]
    fn numeric_null_replaced() {
        assert_eq!(decode_numeric("0", false), Some('\u{FFFD}'));
        assert_eq!(decode_numeric("0", true), Some('\u{FFFD}'));
    }

    #[test]
    fn numeric_invalid() {
        assert_eq!(decode_numeric("FFFFFF", true), Some('\u{FFFD}'));
        assert_eq!(decode_numeric("D800", true), Some('\u{FFFD}'));
        assert_eq!(
            decode_numeric("999999999999999999999999", false),
            Some('\u{FFFD}')
        );
        assert_eq!(decode_numeric("abc", false), None); // not decimal
        assert_eq!(decode_numeric("", false), None);
    }

    #[test]
    fn numeric_c1_controls_use_html_replacements() {
        assert_eq!(decode_numeric("128", false), Some('\u{20AC}'));
        assert_eq!(decode_numeric("82", true), Some('\u{201A}'));
        assert_eq!(decode_numeric("9F", true), Some('\u{0178}'));
        assert_eq!(decode_numeric("81", true), Some('\u{0081}'));
    }

    #[test]
    fn numeric_noncharacters_are_preserved() {
        assert_eq!(decode_numeric("FDD0", true), Some('\u{FDD0}'));
    }

    #[test]
    fn greek_entities() {
        assert_eq!(decode_named("alpha"), Some("\u{03B1}"));
        assert_eq!(decode_named("omega"), Some("\u{03C9}"));
        assert_eq!(decode_named("Sigma"), Some("\u{03A3}"));
    }

    #[test]
    fn typography_entities() {
        assert_eq!(decode_named("mdash"), Some("\u{2014}"));
        assert_eq!(decode_named("euro"), Some("\u{20AC}"));
        assert_eq!(decode_named("trade"), Some("\u{2122}"));
    }

    // ---- escape_text tests ----

    #[test]
    fn escape_text_special_chars() {
        let mut buf = String::new();
        escape_text("&", &mut buf);
        assert_eq!(buf, "&amp;");

        buf.clear();
        escape_text("<", &mut buf);
        assert_eq!(buf, "&lt;");

        buf.clear();
        escape_text(">", &mut buf);
        assert_eq!(buf, "&gt;");
    }

    #[test]
    fn escape_text_mixed() {
        let mut buf = String::new();
        escape_text("1 < 2 & 3 > 0", &mut buf);
        assert_eq!(buf, "1 &lt; 2 &amp; 3 &gt; 0");
    }

    #[test]
    fn escape_text_plain() {
        let mut buf = String::new();
        escape_text("hello world", &mut buf);
        assert_eq!(buf, "hello world");
    }

    #[test]
    fn escape_text_empty() {
        let mut buf = String::new();
        escape_text("", &mut buf);
        assert_eq!(buf, "");
    }

    #[test]
    fn escape_text_all_special() {
        let mut buf = String::new();
        escape_text("&<>", &mut buf);
        assert_eq!(buf, "&amp;&lt;&gt;");
    }

    // ---- escape_attr tests ----

    #[test]
    fn escape_attr_quote() {
        let mut buf = String::new();
        escape_attr("say \"hello\"", &mut buf);
        assert_eq!(buf, "say &quot;hello&quot;");
    }

    #[test]
    fn escape_attr_mixed() {
        let mut buf = String::new();
        escape_attr("x&y=\"z\"", &mut buf);
        assert_eq!(buf, "x&amp;y=&quot;z&quot;");
    }

    #[test]
    fn escape_attr_plain() {
        let mut buf = String::new();
        escape_attr("plain", &mut buf);
        assert_eq!(buf, "plain");
    }

    #[test]
    fn escape_attr_single_quote() {
        let mut buf = String::new();
        escape_attr("it's", &mut buf);
        assert_eq!(buf, "it&#39;s");
    }

    #[test]
    fn escape_attr_empty() {
        let mut buf = String::new();
        escape_attr("", &mut buf);
        assert_eq!(buf, "");
    }

    #[test]
    fn escape_text_does_not_escape_quotes() {
        let mut buf = String::new();
        escape_text("say \"hello\" it's", &mut buf);
        assert_eq!(buf, "say \"hello\" it's");
    }
}
