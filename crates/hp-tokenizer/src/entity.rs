//! Entity decoding with SIMD fast-path.
//!
//! If the input contains no `&` characters, returns `Cow::Borrowed` (zero
//! allocation). Otherwise, decodes named (`&amp;`), decimal (`&#60;`), and
//! hex (`&#x3C;`) character references using `hp_core::entity`.

use std::borrow::Cow;

/// Decode HTML entities in a string.
///
/// Fast path: if no `&` is present, returns `Cow::Borrowed` with zero
/// allocation. Otherwise, decodes entities and returns `Cow::Owned`.
///
/// # Examples
///
/// ```
/// use hp_tokenizer::entity::decode_entities;
///
/// assert_eq!(decode_entities("hello"), "hello");
/// assert_eq!(decode_entities("a &amp; b"), "a & b");
/// assert_eq!(decode_entities("&#60;div&#62;"), "<div>");
/// assert_eq!(decode_entities("&#x3C;br&#x3E;"), "<br>");
/// ```
pub fn decode_entities<'a>(input: &'a str) -> Cow<'a, str> {
    // Fast path: no ampersand → no entities to decode.
    if !input.as_bytes().contains(&b'&') {
        return Cow::Borrowed(input);
    }

    decode_entities_slow(input)
}

/// Slow path: actually decode entities.
fn decode_entities_slow(input: &str) -> Cow<'_, str> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if bytes[i] == b'&' {
            // Look for the closing ';'.
            if let Some(semi_offset) = bytes[i + 1..].iter().position(|&b| b == b';') {
                let semi = i + 1 + semi_offset;
                let entity_body = &input[i + 1..semi];

                if let Some(decoded) = try_decode_entity(entity_body) {
                    result.push_str(&decoded);
                    i = semi + 1;
                    continue;
                }
            }
            // Unrecognized entity — pass through literally.
            result.push('&');
            i += 1;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    Cow::Owned(result)
}

/// Try to decode a single entity body (between `&` and `;`).
fn try_decode_entity(body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }

    if let Some(digits) = body.strip_prefix('#') {
        // Numeric entity.
        if digits.starts_with('x') || digits.starts_with('X') {
            // Hex: &#xHH;
            hp_core::entity::decode_numeric(&digits[1..], true).map(|c| c.to_string())
        } else {
            // Decimal: &#DD;
            hp_core::entity::decode_numeric(digits, false).map(|c| c.to_string())
        }
    } else {
        // Named entity.
        hp_core::entity::decode_named(body).map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_entities_borrowed() {
        let result = decode_entities("hello world");
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "hello world");
    }

    #[test]
    fn named_entities() {
        assert_eq!(decode_entities("&amp;"), "&");
        assert_eq!(decode_entities("&lt;"), "<");
        assert_eq!(decode_entities("&gt;"), ">");
        assert_eq!(decode_entities("&quot;"), "\"");
        assert_eq!(decode_entities("&apos;"), "'");
    }

    #[test]
    fn numeric_decimal() {
        assert_eq!(decode_entities("&#60;"), "<");
        assert_eq!(decode_entities("&#62;"), ">");
        assert_eq!(decode_entities("&#38;"), "&");
    }

    #[test]
    fn numeric_hex() {
        assert_eq!(decode_entities("&#x3C;"), "<");
        assert_eq!(decode_entities("&#x3E;"), ">");
        assert_eq!(decode_entities("&#X3c;"), "<");
    }

    #[test]
    fn mixed_entities() {
        assert_eq!(decode_entities("a &amp; b &lt; c &#62; d"), "a & b < c > d");
    }

    #[test]
    fn unknown_entity_passthrough() {
        assert_eq!(decode_entities("&unknown;"), "&unknown;");
    }

    #[test]
    fn ampersand_without_semicolon() {
        assert_eq!(decode_entities("a & b"), "a & b");
    }

    #[test]
    fn empty_input() {
        let result = decode_entities("");
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "");
    }

    #[test]
    fn entity_at_end() {
        assert_eq!(decode_entities("hello&amp;"), "hello&");
    }

    #[test]
    fn consecutive_entities() {
        assert_eq!(decode_entities("&lt;&gt;&amp;"), "<>&");
    }
}
