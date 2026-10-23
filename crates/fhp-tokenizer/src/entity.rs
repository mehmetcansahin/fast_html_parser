//! WHATWG character-reference decoding with a zero-allocation fast path.

use std::borrow::Cow;

use fhp_core::entity::{decode_numeric, longest_named_prefix};
use memchr::memchr;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntityContext {
    Text,
    Attribute,
}

#[derive(Debug, PartialEq)]
enum Replacement {
    Named(&'static str),
    Numeric(char),
}

impl Replacement {
    #[inline]
    fn push_into(self, output: &mut String) {
        match self {
            Self::Named(value) => output.push_str(value),
            Self::Numeric(value) => output.push(value),
        }
    }
}

/// Decode HTML character references in text content.
///
/// Named references use the complete pinned WHATWG data set, including
/// multi-codepoint replacements and the legacy names for which the trailing
/// semicolon may be omitted. If no reference is decoded, the input is returned
/// as a borrowed string.
#[inline]
pub fn decode_entities(input: &str) -> Cow<'_, str> {
    decode_entities_in(input, EntityContext::Text)
}

/// Decode HTML character references in an attribute value.
///
/// This applies the ambiguous-ampersand rule: a legacy named reference without
/// a semicolon is not decoded when followed by an ASCII alphanumeric byte or
/// `=`.
#[inline]
pub fn decode_attribute_entities(input: &str) -> Cow<'_, str> {
    decode_entities_in(input, EntityContext::Attribute)
}

fn decode_entities_in(input: &str, context: EntityContext) -> Cow<'_, str> {
    let bytes = input.as_bytes();
    let Some(mut amp) = memchr(b'&', bytes) else {
        return Cow::Borrowed(input);
    };

    let mut output = None;
    let mut copied_until = 0usize;

    loop {
        let Some((replacement, consumed)) = decode_reference(input, amp + 1, context) else {
            let search_from = amp + 1;
            let Some(relative_amp) = memchr(b'&', &bytes[search_from..]) else {
                break;
            };
            amp = search_from + relative_amp;
            continue;
        };

        let output = output.get_or_insert_with(|| String::with_capacity(input.len()));
        output.push_str(&input[copied_until..amp]);
        replacement.push_into(output);
        copied_until = amp + 1 + consumed;

        let Some(relative_amp) = memchr(b'&', &bytes[copied_until..]) else {
            break;
        };
        amp = copied_until + relative_amp;
    }

    let Some(mut output) = output else {
        return Cow::Borrowed(input);
    };

    output.push_str(&input[copied_until..]);
    Cow::Owned(output)
}

/// Decode the reference whose body begins at `start` (immediately after `&`).
/// Returns the replacement and bytes consumed after the ampersand.
fn decode_reference(
    input: &str,
    start: usize,
    context: EntityContext,
) -> Option<(Replacement, usize)> {
    common_reference(&input.as_bytes()[start..])
        .or_else(|| decode_any_reference(input, start, context))
}

/// The general matcher behind [`decode_reference`]'s common-name fast path.
fn decode_any_reference(
    input: &str,
    start: usize,
    context: EntityContext,
) -> Option<(Replacement, usize)> {
    let bytes = input.as_bytes();
    let first = *bytes.get(start)?;

    if first == b'#' {
        return decode_numeric_reference(input, start);
    }

    // Scan the name once. The core matcher records the longest legacy terminal
    // while locating the complete name, then prefers an exact PHF hit when a
    // semicolon terminates it. Exact misses and semicolon-less references reuse
    // the recorded legacy candidate instead of rescanning the same bytes.
    let matched = longest_named_prefix(&bytes[start..])?;
    let name_end = start + matched.name_len;
    let has_semicolon = bytes.get(name_end) == Some(&b';');
    if !has_semicolon && !matched.allows_legacy_omission {
        return None;
    }

    if context == EntityContext::Attribute
        && !has_semicolon
        && matches!(bytes.get(name_end), Some(byte) if byte.is_ascii_alphanumeric() || *byte == b'=')
    {
        return None;
    }

    Some((
        Replacement::Named(matched.value),
        matched.name_len + usize::from(has_semicolon),
    ))
}

/// Match the semicolon-terminated references that dominate real-world HTML
/// without walking the general matcher. `body` starts after the `&`.
///
/// Each spelling ends in `;`, which stops the general name scan, so the
/// result is identical to the slow path in both text and attribute context.
#[inline]
fn common_reference(body: &[u8]) -> Option<(Replacement, usize)> {
    match body {
        [b'a', b'm', b'p', b';', ..] => Some((Replacement::Named("&"), 4)),
        [b'l', b't', b';', ..] => Some((Replacement::Named("<"), 3)),
        [b'g', b't', b';', ..] => Some((Replacement::Named(">"), 3)),
        [b'q', b'u', b'o', b't', b';', ..] => Some((Replacement::Named("\""), 5)),
        [b'n', b'b', b's', b'p', b';', ..] => Some((Replacement::Named("\u{00A0}"), 5)),
        [b'a', b'p', b'o', b's', b';', ..] => Some((Replacement::Named("'"), 5)),
        [b'#', b'3', b'9', b';', ..] => Some((Replacement::Numeric('\''), 4)),
        _ => None,
    }
}

fn decode_numeric_reference(input: &str, start: usize) -> Option<(Replacement, usize)> {
    let bytes = input.as_bytes();
    let mut cursor = start + 1; // skip '#'
    let is_hex = matches!(bytes.get(cursor), Some(b'x' | b'X'));
    if is_hex {
        cursor += 1;
    }
    let digits_start = cursor;

    while let Some(&byte) = bytes.get(cursor) {
        let valid = if is_hex {
            byte.is_ascii_hexdigit()
        } else {
            byte.is_ascii_digit()
        };
        if !valid {
            break;
        }
        cursor += 1;
    }

    if cursor == digits_start {
        return None;
    }

    let value = decode_numeric(&input[digits_start..cursor], is_hex)?;
    let has_semicolon = bytes.get(cursor) == Some(&b';');
    let consumed = cursor - start + usize::from(has_semicolon);
    Some((Replacement::Numeric(value), consumed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_reference_is_borrowed() {
        assert!(matches!(decode_entities("hello world"), Cow::Borrowed(_)));
        assert!(matches!(decode_entities("a & b"), Cow::Borrowed(_)));
    }

    #[test]
    fn decodes_full_whatwg_and_multi_codepoint_entities() {
        assert_eq!(decode_entities("&CounterClockwiseContourIntegral;"), "∳");
        assert_eq!(decode_entities("&NotEqualTilde;"), "≂̸");
        assert_eq!(decode_entities("&Afr;"), "𝔄");
    }

    #[test]
    fn applies_legacy_semicolon_omission_rules() {
        assert_eq!(decode_entities("&copy test"), "© test");
        assert_eq!(decode_entities("&notin;"), "∉");
        assert_eq!(decode_entities("&notin"), "¬in");
        assert_eq!(decode_entities("&notit;"), "¬it;");
        assert_eq!(
            decode_entities("&NotEqualTilde test"),
            "&NotEqualTilde test"
        );
    }

    #[test]
    fn attribute_context_rejects_ambiguous_ampersands() {
        assert_eq!(decode_attribute_entities("&copy test"), "© test");
        assert_eq!(decode_attribute_entities("&copy=test"), "&copy=test");
        assert_eq!(decode_attribute_entities("&notin;"), "∉");
        assert_eq!(decode_attribute_entities("&notin"), "&notin");
        assert_eq!(decode_attribute_entities("&notit;"), "&notit;");
        assert_eq!(decode_attribute_entities("&copy;=test"), "©=test");
    }

    #[test]
    fn numeric_references_allow_missing_semicolons() {
        assert_eq!(decode_entities("&#60div"), "<div");
        assert_eq!(decode_entities("&#x3Cdiv"), "ύiv");
        assert_eq!(decode_entities("&#x3C;div"), "<div");
        assert_eq!(decode_entities("&#128; &#x82;"), "€ ‚");
    }

    #[test]
    fn malformed_references_are_preserved() {
        assert_eq!(decode_entities("&&&&"), "&&&&");
        assert_eq!(decode_entities("&unknown;"), "&unknown;");
        assert_eq!(decode_entities("&#; &#x;"), "&#; &#x;");
    }

    #[test]
    fn common_references_match_the_general_matcher() {
        for body in ["amp;", "lt;", "gt;", "quot;", "nbsp;", "apos;", "#39;"] {
            for suffix in ["", "x", "=", ";", " ", "&amp;"] {
                let input = format!("&{body}{suffix}");
                let fast = common_reference(&input.as_bytes()[1..]);
                assert!(fast.is_some(), "{input}");
                for context in [EntityContext::Text, EntityContext::Attribute] {
                    assert_eq!(fast, decode_any_reference(&input, 1, context), "{input}");
                }
            }
        }
        for body in ["amp", "lt", "ampx;", "#39", "#390;", "quo;", ""] {
            assert_eq!(common_reference(body.as_bytes()), None, "{body}");
        }
    }

    #[test]
    fn many_unterminated_ampersands_remain_linear_and_borrowed() {
        let input = "&".repeat(100_000);
        let decoded = decode_entities(&input);
        assert!(matches!(decoded, Cow::Borrowed(_)));
        assert_eq!(decoded, input);
    }
}
