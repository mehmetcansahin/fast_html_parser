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
    ENTITY_MAP.get(name).copied()
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

/// Compile-time perfect-hash map of the most common HTML named entities.
///
/// This covers the ~250 most-used entities. The full HTML5 spec defines
/// ~2200, but the long tail is almost never seen in practice.
static ENTITY_MAP: phf::Map<&'static str, &'static str> = phf::phf_map! {
    // Most common
    "amp"    => "&",
    "lt"     => "<",
    "gt"     => ">",
    "quot"   => "\"",
    "apos"   => "'",
    "nbsp"   => "\u{00A0}",

    // Latin supplement
    "iexcl"  => "\u{00A1}",
    "cent"   => "\u{00A2}",
    "pound"  => "\u{00A3}",
    "curren" => "\u{00A4}",
    "yen"    => "\u{00A5}",
    "brvbar" => "\u{00A6}",
    "sect"   => "\u{00A7}",
    "uml"    => "\u{00A8}",
    "copy"   => "\u{00A9}",
    "ordf"   => "\u{00AA}",
    "laquo"  => "\u{00AB}",
    "not"    => "\u{00AC}",
    "shy"    => "\u{00AD}",
    "reg"    => "\u{00AE}",
    "macr"   => "\u{00AF}",
    "deg"    => "\u{00B0}",
    "plusmn" => "\u{00B1}",
    "sup2"   => "\u{00B2}",
    "sup3"   => "\u{00B3}",
    "acute"  => "\u{00B4}",
    "micro"  => "\u{00B5}",
    "para"   => "\u{00B6}",
    "middot" => "\u{00B7}",
    "cedil"  => "\u{00B8}",
    "sup1"   => "\u{00B9}",
    "ordm"   => "\u{00BA}",
    "raquo"  => "\u{00BB}",
    "frac14" => "\u{00BC}",
    "frac12" => "\u{00BD}",
    "frac34" => "\u{00BE}",
    "iquest" => "\u{00BF}",

    // Accented Latin
    "Agrave" => "\u{00C0}",
    "Aacute" => "\u{00C1}",
    "Acirc"  => "\u{00C2}",
    "Atilde" => "\u{00C3}",
    "Auml"   => "\u{00C4}",
    "Aring"  => "\u{00C5}",
    "AElig"  => "\u{00C6}",
    "Ccedil" => "\u{00C7}",
    "Egrave" => "\u{00C8}",
    "Eacute" => "\u{00C9}",
    "Ecirc"  => "\u{00CA}",
    "Euml"   => "\u{00CB}",
    "Igrave" => "\u{00CC}",
    "Iacute" => "\u{00CD}",
    "Icirc"  => "\u{00CE}",
    "Iuml"   => "\u{00CF}",
    "ETH"    => "\u{00D0}",
    "Ntilde" => "\u{00D1}",
    "Ograve" => "\u{00D2}",
    "Oacute" => "\u{00D3}",
    "Ocirc"  => "\u{00D4}",
    "Otilde" => "\u{00D5}",
    "Ouml"   => "\u{00D6}",
    "times"  => "\u{00D7}",
    "Oslash" => "\u{00D8}",
    "Ugrave" => "\u{00D9}",
    "Uacute" => "\u{00DA}",
    "Ucirc"  => "\u{00DB}",
    "Uuml"   => "\u{00DC}",
    "Yacute" => "\u{00DD}",
    "THORN"  => "\u{00DE}",
    "szlig"  => "\u{00DF}",
    "agrave" => "\u{00E0}",
    "aacute" => "\u{00E1}",
    "acirc"  => "\u{00E2}",
    "atilde" => "\u{00E3}",
    "auml"   => "\u{00E4}",
    "aring"  => "\u{00E5}",
    "aelig"  => "\u{00E6}",
    "ccedil" => "\u{00E7}",
    "egrave" => "\u{00E8}",
    "eacute" => "\u{00E9}",
    "ecirc"  => "\u{00EA}",
    "euml"   => "\u{00EB}",
    "igrave" => "\u{00EC}",
    "iacute" => "\u{00ED}",
    "icirc"  => "\u{00EE}",
    "iuml"   => "\u{00EF}",
    "eth"    => "\u{00F0}",
    "ntilde" => "\u{00F1}",
    "ograve" => "\u{00F2}",
    "oacute" => "\u{00F3}",
    "ocirc"  => "\u{00F4}",
    "otilde" => "\u{00F5}",
    "ouml"   => "\u{00F6}",
    "divide" => "\u{00F7}",
    "oslash" => "\u{00F8}",
    "ugrave" => "\u{00F9}",
    "uacute" => "\u{00FA}",
    "ucirc"  => "\u{00FB}",
    "uuml"   => "\u{00FC}",
    "yacute" => "\u{00FD}",
    "thorn"  => "\u{00FE}",
    "yuml"   => "\u{00FF}",

    // Greek
    "Alpha"   => "\u{0391}",
    "Beta"    => "\u{0392}",
    "Gamma"   => "\u{0393}",
    "Delta"   => "\u{0394}",
    "Epsilon" => "\u{0395}",
    "Zeta"    => "\u{0396}",
    "Eta"     => "\u{0397}",
    "Theta"   => "\u{0398}",
    "Iota"    => "\u{0399}",
    "Kappa"   => "\u{039A}",
    "Lambda"  => "\u{039B}",
    "Mu"      => "\u{039C}",
    "Nu"      => "\u{039D}",
    "Xi"      => "\u{039E}",
    "Omicron" => "\u{039F}",
    "Pi"      => "\u{03A0}",
    "Rho"     => "\u{03A1}",
    "Sigma"   => "\u{03A3}",
    "Tau"     => "\u{03A4}",
    "Upsilon" => "\u{03A5}",
    "Phi"     => "\u{03A6}",
    "Chi"     => "\u{03A7}",
    "Psi"     => "\u{03A8}",
    "Omega"   => "\u{03A9}",
    "alpha"   => "\u{03B1}",
    "beta"    => "\u{03B2}",
    "gamma"   => "\u{03B3}",
    "delta"   => "\u{03B4}",
    "epsilon" => "\u{03B5}",
    "zeta"    => "\u{03B6}",
    "eta"     => "\u{03B7}",
    "theta"   => "\u{03B8}",
    "iota"    => "\u{03B9}",
    "kappa"   => "\u{03BA}",
    "lambda"  => "\u{03BB}",
    "mu"      => "\u{03BC}",
    "nu"      => "\u{03BD}",
    "xi"      => "\u{03BE}",
    "omicron" => "\u{03BF}",
    "pi"      => "\u{03C0}",
    "rho"     => "\u{03C1}",
    "sigmaf"  => "\u{03C2}",
    "sigma"   => "\u{03C3}",
    "tau"     => "\u{03C4}",
    "upsilon" => "\u{03C5}",
    "phi"     => "\u{03C6}",
    "chi"     => "\u{03C7}",
    "psi"     => "\u{03C8}",
    "omega"   => "\u{03C9}",

    // Math / symbols
    "bull"    => "\u{2022}",
    "hellip"  => "\u{2026}",
    "prime"   => "\u{2032}",
    "Prime"   => "\u{2033}",
    "oline"   => "\u{203E}",
    "frasl"   => "\u{2044}",
    "trade"   => "\u{2122}",
    "larr"    => "\u{2190}",
    "uarr"    => "\u{2191}",
    "rarr"    => "\u{2192}",
    "darr"    => "\u{2193}",
    "harr"    => "\u{2194}",
    "lArr"    => "\u{21D0}",
    "uArr"    => "\u{21D1}",
    "rArr"    => "\u{21D2}",
    "dArr"    => "\u{21D3}",
    "hArr"    => "\u{21D4}",
    "forall"  => "\u{2200}",
    "part"    => "\u{2202}",
    "exist"   => "\u{2203}",
    "empty"   => "\u{2205}",
    "nabla"   => "\u{2207}",
    "isin"    => "\u{2208}",
    "notin"   => "\u{2209}",
    "ni"      => "\u{220B}",
    "prod"    => "\u{220F}",
    "sum"     => "\u{2211}",
    "minus"   => "\u{2212}",
    "lowast"  => "\u{2217}",
    "radic"   => "\u{221A}",
    "prop"    => "\u{221D}",
    "infin"   => "\u{221E}",
    "ang"     => "\u{2220}",
    "and"     => "\u{2227}",
    "or"      => "\u{2228}",
    "cap"     => "\u{2229}",
    "cup"     => "\u{222A}",
    "int"     => "\u{222B}",
    "there4"  => "\u{2234}",
    "sim"     => "\u{223C}",
    "cong"    => "\u{2245}",
    "asymp"   => "\u{2248}",
    "ne"      => "\u{2260}",
    "equiv"   => "\u{2261}",
    "le"      => "\u{2264}",
    "ge"      => "\u{2265}",
    "sub"     => "\u{2282}",
    "sup"     => "\u{2283}",
    "nsub"    => "\u{2284}",
    "sube"    => "\u{2286}",
    "supe"    => "\u{2287}",
    "oplus"   => "\u{2295}",
    "otimes"  => "\u{2297}",
    "perp"    => "\u{22A5}",
    "sdot"    => "\u{22C5}",

    // Punctuation / typography
    "ensp"    => "\u{2002}",
    "emsp"    => "\u{2003}",
    "thinsp"  => "\u{2009}",
    "zwnj"    => "\u{200C}",
    "zwj"     => "\u{200D}",
    "lrm"     => "\u{200E}",
    "rlm"     => "\u{200F}",
    "ndash"   => "\u{2013}",
    "mdash"   => "\u{2014}",
    "lsquo"   => "\u{2018}",
    "rsquo"   => "\u{2019}",
    "sbquo"   => "\u{201A}",
    "ldquo"   => "\u{201C}",
    "rdquo"   => "\u{201D}",
    "bdquo"   => "\u{201E}",
    "dagger"  => "\u{2020}",
    "Dagger"  => "\u{2021}",
    "permil"  => "\u{2030}",
    "lsaquo"  => "\u{2039}",
    "rsaquo"  => "\u{203A}",
    "euro"    => "\u{20AC}",

    // Miscellaneous
    "OElig"   => "\u{0152}",
    "oelig"   => "\u{0153}",
    "Scaron"  => "\u{0160}",
    "scaron"  => "\u{0161}",
    "Yuml"    => "\u{0178}",
    "circ"    => "\u{02C6}",
    "tilde"   => "\u{02DC}",
    "fnof"    => "\u{0192}",

    // Card suits / misc symbols
    "spades"  => "\u{2660}",
    "clubs"   => "\u{2663}",
    "hearts"  => "\u{2665}",
    "diams"   => "\u{2666}",
    "loz"     => "\u{25CA}",
    "lceil"   => "\u{2308}",
    "rceil"   => "\u{2309}",
    "lfloor"  => "\u{230A}",
    "rfloor"  => "\u{230B}",
    "lang"    => "\u{2329}",
    "rang"    => "\u{232A}",
};

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
