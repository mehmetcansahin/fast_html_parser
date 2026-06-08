//! Comprehensive integration tests for fhp-tokenizer.

use std::borrow::Cow;

use fhp_core::tag::Tag;
use fhp_tokenizer::entity::decode_entities;
use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token;
use fhp_tokenizer::tokenize;

fn text_content(tokens: &[Token<'_>]) -> String {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::Text { content } => Some(content.as_ref()),
            _ => None,
        })
        .collect()
}

fn open_tag_names<'a>(tokens: &'a [Token<'_>]) -> Vec<&'a str> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::OpenTag { name, .. } => Some(name.as_ref()),
            _ => None,
        })
        .collect()
}

fn close_tag_names<'a>(tokens: &'a [Token<'_>]) -> Vec<&'a str> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::CloseTag { name, .. } => Some(name.as_ref()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------
// Well-formed HTML — every token type
// ---------------------------------------------------------------

#[test]
fn well_formed_open_tag() {
    let tokens = tokenize("<div>");
    assert_eq!(tokens.len(), 1);
    match &tokens[0] {
        Token::OpenTag {
            tag,
            name,
            attributes,
            ..
        } => {
            assert_eq!(*tag, Tag::Div);
            assert_eq!(name.as_ref(), "div");
            assert!(attributes.is_empty());
        }
        other => panic!("expected OpenTag, got {other:?}"),
    }
}

#[test]
fn well_formed_close_tag() {
    let tokens = tokenize("</div>");
    assert_eq!(tokens.len(), 1);
    match &tokens[0] {
        Token::CloseTag { tag, name } => {
            assert_eq!(*tag, Tag::Div);
            assert_eq!(name.as_ref(), "div");
        }
        other => panic!("expected CloseTag, got {other:?}"),
    }
}

#[test]
fn well_formed_text() {
    let tokens = tokenize("hello world");
    assert_eq!(tokens.len(), 1);
    match &tokens[0] {
        Token::Text { content } => assert_eq!(content.as_ref(), "hello world"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn well_formed_comment() {
    let tokens = tokenize("<!-- comment body -->");
    let comment = tokens.iter().find(|t| matches!(t, Token::Comment { .. }));
    match comment {
        Some(Token::Comment { content }) => {
            assert_eq!(content.trim(), "comment body");
        }
        other => panic!("expected Comment, got {other:?}"),
    }
}

#[test]
fn well_formed_doctype() {
    let tokens = tokenize("<!DOCTYPE html>");
    let doctype = tokens.iter().find(|t| matches!(t, Token::Doctype { .. }));
    match doctype {
        Some(Token::Doctype { content }) => {
            assert_eq!(content.as_ref(), "html");
        }
        other => panic!("expected Doctype, got {other:?}"),
    }
}

#[test]
fn well_formed_attribute() {
    let tokens = tokenize("<a href=\"https://example.com\" class=\"link\">");
    match &tokens[0] {
        Token::OpenTag {
            tag, attributes, ..
        } => {
            assert_eq!(*tag, Tag::A);
            assert_eq!(attributes.len(), 2);
            assert_eq!(attributes[0].name.as_ref(), "href");
            assert_eq!(attributes[0].value.as_deref(), Some("https://example.com"));
            assert_eq!(attributes[1].name.as_ref(), "class");
            assert_eq!(attributes[1].value.as_deref(), Some("link"));
        }
        other => panic!("expected OpenTag, got {other:?}"),
    }
}

#[test]
fn well_formed_self_closing() {
    let tokens = tokenize("<br/>");
    match &tokens[0] {
        Token::OpenTag {
            tag, self_closing, ..
        } => {
            assert_eq!(*tag, Tag::Br);
            assert!(*self_closing);
        }
        other => panic!("expected OpenTag, got {other:?}"),
    }
}

#[test]
fn well_formed_full_document() {
    let html = "<!DOCTYPE html><html><head><title>Test</title></head><body><div class=\"main\"><p>Hello &amp; world</p></div></body></html>";
    let tokens = tokenize(html);

    // Should have doctype.
    assert!(tokens.iter().any(|t| matches!(t, Token::Doctype { .. })));

    // Should have open and close tags.
    let tag_names: Vec<&str> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::OpenTag { name, .. } => Some(name.as_ref()),
            _ => None,
        })
        .collect();
    assert!(tag_names.contains(&"html"));
    assert!(tag_names.contains(&"head"));
    assert!(tag_names.contains(&"body"));
    assert!(tag_names.contains(&"div"));
    assert!(tag_names.contains(&"p"));

    // Should have decoded entity in text.
    let text_tokens: Vec<&str> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::Text { content } => Some(content.as_ref()),
            _ => None,
        })
        .collect();
    assert!(
        text_tokens.iter().any(|t| t.contains("Hello & world")),
        "text: {text_tokens:?}"
    );
}

// ---------------------------------------------------------------
// Broken / malformed HTML
// ---------------------------------------------------------------

#[test]
fn malformed_unclosed_tag() {
    // `<div` with no `>` — should be treated as text.
    let tokens = tokenize("<div");
    // The text should contain the unclosed tag as text.
    assert!(
        !tokens.is_empty(),
        "should produce at least one token for unclosed tag"
    );
}

#[test]
fn malformed_missing_close_quote() {
    // Attribute with missing closing quote.
    let tokens = tokenize("<div class=\"foo>");
    // Should still produce something (may be mangled).
    assert!(!tokens.is_empty());
}

#[test]
fn malformed_script_with_lt() {
    // `<` inside script should not start a new tag.
    let tokens = tokenize("<script>if (x < 10) {}</script>");
    let has_script_open = tokens.iter().any(|t| {
        matches!(
            t,
            Token::OpenTag {
                tag: Tag::Script,
                ..
            }
        )
    });
    let has_script_close = tokens.iter().any(|t| {
        matches!(
            t,
            Token::CloseTag {
                tag: Tag::Script,
                ..
            }
        )
    });
    assert!(has_script_open, "should have script open tag: {tokens:?}");
    assert!(has_script_close, "should have script close tag: {tokens:?}");
}

#[test]
fn script_entity_is_not_decoded_as_text() {
    let tokens = tokenize("<script>const s = \"&amp;\";</script>");
    let script_text = tokens.iter().find_map(|token| match token {
        Token::Text { content } => Some(content.as_ref()),
        _ => None,
    });

    assert_eq!(script_text, Some("const s = \"&amp;\";"));
}

#[test]
fn invalid_close_like_text_is_preserved() {
    let tokens = tokenize("<p>a </ b</p>");
    let text = tokens.iter().find_map(|token| match token {
        Token::Text { content } => Some(content.as_ref()),
        _ => None,
    });

    assert_eq!(text, Some("a </ b"));
}

#[derive(Debug)]
struct TextMarkupCase {
    name: &'static str,
    html: &'static str,
    expected_text: &'static str,
    expected_open_tags: &'static [&'static str],
    expected_close_tags: &'static [&'static str],
}

#[test]
fn invalid_lt_sequences_stay_in_text() {
    let cases = [
        TextMarkupCase {
            name: "lt_before_space",
            html: "<p>a < 2</p>",
            expected_text: "a < 2",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "double_lt",
            html: "<p>a << b</p>",
            expected_text: "a << b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "lt_before_dot",
            html: "<p>a <.b</p>",
            expected_text: "a <.b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "lt_before_hash",
            html: "<p>a <#b</p>",
            expected_text: "a <#b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "lt_before_ampersand",
            html: "<p>a <&b</p>",
            expected_text: "a <&b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "slash_space",
            html: "<p>a </ b</p>",
            expected_text: "a </ b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "slash_digit",
            html: "<p>a </2</p>",
            expected_text: "a </2",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "trailing_lt",
            html: "<p>a <</p>",
            expected_text: "a <",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "lt_equal",
            html: "<p>a <= b</p>",
            expected_text: "a <= b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "lt_percent",
            html: "<p>a <% b</p>",
            expected_text: "a <% b",
            expected_open_tags: &["p"],
            expected_close_tags: &["p"],
        },
        TextMarkupCase {
            name: "invalid_lt_then_real_child",
            html: "<p>a < 2 <span>b</span></p>",
            expected_text: "a < 2 b",
            expected_open_tags: &["p", "span"],
            expected_close_tags: &["span", "p"],
        },
    ];

    for case in cases {
        let tokens = tokenize(case.html);

        assert_eq!(
            text_content(&tokens),
            case.expected_text,
            "case={}, html={}, tokens={tokens:?}",
            case.name,
            case.html
        );
        assert_eq!(
            open_tag_names(&tokens),
            case.expected_open_tags,
            "case={}, html={}, tokens={tokens:?}",
            case.name,
            case.html
        );
        assert_eq!(
            close_tag_names(&tokens),
            case.expected_close_tags,
            "case={}, html={}, tokens={tokens:?}",
            case.name,
            case.html
        );
    }
}

#[test]
fn valid_close_tags_still_close_after_lt_filtering() {
    let tokens = tokenize("<p>a </p><p>b</p>");
    let close_tags: Vec<_> = tokens
        .iter()
        .filter_map(|token| match token {
            Token::CloseTag { name, .. } => Some(name.as_ref()),
            _ => None,
        })
        .collect();
    let text: String = tokens
        .iter()
        .filter_map(|token| match token {
            Token::Text { content } => Some(content.as_ref()),
            _ => None,
        })
        .collect();

    assert_eq!(close_tags, ["p", "p"]);
    assert_eq!(text, "a b");
}

#[derive(Debug)]
struct RawTextCase {
    name: &'static str,
    html: &'static str,
    expected_text: &'static str,
    expected_open_tag: &'static str,
    expected_close_tag: &'static str,
    expected_tag: Tag,
}

#[test]
fn raw_text_close_like_sequences_are_preserved() {
    let cases = [
        RawTextCase {
            name: "script_invalid_close_like_expression",
            html: "<script>if (a </ b) { x = \"&amp;\"; }</script>",
            expected_text: "if (a </ b) { x = \"&amp;\"; }",
            expected_open_tag: "script",
            expected_close_tag: "script",
            expected_tag: Tag::Script,
        },
        RawTextCase {
            name: "script_fake_other_close_tag",
            html: "<script>const s = \"</not-script>\";</script>",
            expected_text: "const s = \"</not-script>\";",
            expected_open_tag: "script",
            expected_close_tag: "script",
            expected_tag: Tag::Script,
        },
        RawTextCase {
            name: "script_prefix_close_name_is_text",
            html: "<script>if (a </scripted) {}</script>",
            expected_text: "if (a </scripted) {}",
            expected_open_tag: "script",
            expected_close_tag: "script",
            expected_tag: Tag::Script,
        },
        RawTextCase {
            name: "script_uppercase_close",
            html: "<script>const ok = true;</SCRIPT>",
            expected_text: "const ok = true;",
            expected_open_tag: "script",
            expected_close_tag: "SCRIPT",
            expected_tag: Tag::Script,
        },
        RawTextCase {
            name: "style_lt_comparison",
            html: "<style>.x { width: calc(100% < 2px); }</style>",
            expected_text: ".x { width: calc(100% < 2px); }",
            expected_open_tag: "style",
            expected_close_tag: "style",
            expected_tag: Tag::Style,
        },
        RawTextCase {
            name: "style_entity_stays_raw",
            html: "<style>.x::before { content: \"&lt;\"; }</style>",
            expected_text: ".x::before { content: \"&lt;\"; }",
            expected_open_tag: "style",
            expected_close_tag: "style",
            expected_tag: Tag::Style,
        },
    ];

    for case in cases {
        let tokens = tokenize(case.html);
        let has_close = tokens.iter().any(|token| {
            matches!(
                token,
                Token::CloseTag {
                    tag: close_tag,
                    ..
                } if *close_tag == case.expected_tag
            )
        });

        assert_eq!(
            text_content(&tokens),
            case.expected_text,
            "case={}, html={}, tokens={tokens:?}",
            case.name,
            case.html
        );
        assert_eq!(
            open_tag_names(&tokens),
            [case.expected_open_tag],
            "case={}, html={}, tokens={tokens:?}",
            case.name,
            case.html
        );
        assert_eq!(
            close_tag_names(&tokens),
            [case.expected_close_tag],
            "case={}, html={}, tokens={tokens:?}",
            case.name,
            case.html
        );
        assert!(
            has_close,
            "case={}, html={}, tokens={tokens:?}",
            case.name, case.html
        );
    }
}

#[test]
fn attribute_value_presence_cases() {
    let tokens = tokenize("<input value=\"\" disabled data-empty=''>");
    let attrs = match &tokens[0] {
        Token::OpenTag { attributes, .. } => attributes,
        other => panic!("expected OpenTag, got {other:?}"),
    };
    let values: Vec<_> = attrs
        .iter()
        .map(|attr| (attr.name.as_ref(), attr.value.as_deref()))
        .collect();

    assert_eq!(
        values,
        [
            ("value", Some("")),
            ("disabled", None),
            ("data-empty", Some(""))
        ]
    );
}

#[test]
fn malformed_extra_close_tag() {
    // Extra close tag — should produce a CloseTag token.
    let tokens = tokenize("</div></div>");
    let close_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::CloseTag { .. }))
        .count();
    assert_eq!(close_count, 2);
}

// ---------------------------------------------------------------
// Entity decoding
// ---------------------------------------------------------------

#[test]
fn entity_amp() {
    assert_eq!(decode_entities("&amp;"), "&");
}

#[test]
fn entity_lt() {
    assert_eq!(decode_entities("&lt;"), "<");
}

#[test]
fn entity_numeric_decimal() {
    assert_eq!(decode_entities("&#60;"), "<");
}

#[test]
fn entity_numeric_hex() {
    assert_eq!(decode_entities("&#x3C;"), "<");
}

#[test]
fn entity_unknown_passthrough() {
    assert_eq!(decode_entities("&foobar;"), "&foobar;");
}

#[test]
fn entity_no_entities_borrowed() {
    let result = decode_entities("no entities here");
    assert!(matches!(result, Cow::Borrowed(_)));
}

// ---------------------------------------------------------------
// Streaming — same HTML with different chunk sizes
// ---------------------------------------------------------------

fn collect_streaming(html: &[u8], chunk_size: usize) -> Vec<Token<'static>> {
    let mut tok = StreamTokenizer::new();
    let mut all = Vec::new();
    for chunk in html.chunks(chunk_size) {
        all.extend(tok.feed(chunk));
    }
    all.extend(tok.finish());
    all
}

fn count_token_type(tokens: &[Token<'_>], pred: fn(&Token<'_>) -> bool) -> usize {
    tokens.iter().filter(|t| pred(t)).count()
}

#[test]
fn streaming_1_byte_chunks() {
    let html = b"<div class=\"test\">hello</div>";
    let tokens = collect_streaming(html, 1);
    let open = count_token_type(&tokens, |t| matches!(t, Token::OpenTag { .. }));
    let close = count_token_type(&tokens, |t| matches!(t, Token::CloseTag { .. }));
    assert!(open >= 1, "open tags: {open}, tokens: {tokens:?}");
    assert!(close >= 1, "close tags: {close}, tokens: {tokens:?}");
}

#[test]
fn streaming_7_byte_chunks() {
    let html = b"<div class=\"test\">hello &amp; world</div>";
    let tokens = collect_streaming(html, 7);
    let open = count_token_type(&tokens, |t| matches!(t, Token::OpenTag { .. }));
    let close = count_token_type(&tokens, |t| matches!(t, Token::CloseTag { .. }));
    assert!(open >= 1, "open tags: {open}");
    assert!(close >= 1, "close tags: {close}");
}

#[test]
fn streaming_64_byte_chunks() {
    let html = b"<html><head><title>Test</title></head><body><p>Hello</p></body></html>";
    let tokens = collect_streaming(html, 64);
    let open = count_token_type(&tokens, |t| matches!(t, Token::OpenTag { .. }));
    assert!(open >= 4, "open tags: {open}");
}

// ---------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------

#[test]
fn empty_input() {
    let tokens = tokenize("");
    assert!(tokens.is_empty());
}

#[test]
fn whitespace_only() {
    let tokens = tokenize("   \t\n\r  ");
    assert_eq!(tokens.len(), 1);
    match &tokens[0] {
        Token::Text { content } => {
            assert!(content.trim().is_empty());
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn entity_only() {
    let tokens = tokenize("&amp;&lt;&gt;");
    assert_eq!(tokens.len(), 1);
    match &tokens[0] {
        Token::Text { content } => {
            assert_eq!(content.as_ref(), "&<>");
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn void_elements_without_slash() {
    let tokens = tokenize("<br><hr><img src=\"x.png\">");
    let tags: Vec<(Tag, bool)> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::OpenTag {
                tag, self_closing, ..
            } => Some((*tag, *self_closing)),
            _ => None,
        })
        .collect();

    assert_eq!(tags.len(), 3);
    assert!(
        tags.iter().all(|(_, sc)| *sc),
        "void elements should be self-closing"
    );
}

#[test]
fn single_quoted_attribute() {
    let tokens = tokenize("<div class='foo'>");
    match &tokens[0] {
        Token::OpenTag { attributes, .. } => {
            assert_eq!(attributes[0].value.as_deref(), Some("foo"));
        }
        other => panic!("expected OpenTag, got {other:?}"),
    }
}

#[test]
fn unquoted_attribute() {
    let tokens = tokenize("<div class=foo>");
    match &tokens[0] {
        Token::OpenTag { attributes, .. } => {
            assert_eq!(attributes[0].value.as_deref(), Some("foo"));
        }
        other => panic!("expected OpenTag, got {other:?}"),
    }
}

#[test]
fn multiple_text_segments() {
    let tokens = tokenize("<p>a</p><p>b</p>");
    let texts: Vec<&str> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::Text { content } => Some(content.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, &["a", "b"]);
}

#[test]
fn deeply_nested() {
    let html = "<div><div><div><div><div><span>deep</span></div></div></div></div></div>";
    let tokens = tokenize(html);
    let open_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::OpenTag { .. }))
        .count();
    let close_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::CloseTag { .. }))
        .count();
    assert_eq!(open_count, 6);
    assert_eq!(close_count, 6);
}

#[test]
fn case_insensitive_tags() {
    let tokens = tokenize("<DIV></DIV>");
    match &tokens[0] {
        Token::OpenTag { tag, name, .. } => {
            assert_eq!(*tag, Tag::Div);
            assert_eq!(name.as_ref(), "DIV");
        }
        other => panic!("expected OpenTag, got {other:?}"),
    }
}

#[test]
fn long_input_over_1000_bytes() {
    let mut html = String::with_capacity(2000);
    html.push_str("<div>");
    for i in 0..100 {
        html.push_str(&format!("<span>{i}</span>"));
    }
    html.push_str("</div>");
    assert!(html.len() > 1000);

    let tokens = tokenize(&html);
    let span_opens = tokens
        .iter()
        .filter(|t| matches!(t, Token::OpenTag { tag: Tag::Span, .. }))
        .count();
    assert_eq!(span_opens, 100);
}

// ---------------------------------------------------------------
// CDATA section handling (UB regression)
// ---------------------------------------------------------------

#[test]
fn cdata_wellformed_extracts_content() {
    let tokens = tokenize("<![CDATA[hello]]>");
    let found = tokens
        .iter()
        .find_map(|t| match t {
            Token::CData { content } => Some(content.as_ref()),
            _ => None,
        });
    assert_eq!(found, Some("hello"));
}

#[test]
fn cdata_wellformed_multibyte_content() {
    // A multibyte char inside a real CDATA section must round-trip intact.
    let tokens = tokenize("<![CDATA[a\u{20ac}b]]>");
    let found = tokens
        .iter()
        .find_map(|t| match t {
            Token::CData { content } => Some(content.as_ref()),
            _ => None,
        });
    assert_eq!(found, Some("a\u{20ac}b"));
}

#[test]
fn cdata_malformed_prefix_is_not_treated_as_cdata() {
    // `<![` not followed by the literal `CDATA[` must not enter CDATA mode.
    // With a multibyte char straddling the assumed 9-byte `<![CDATA[` offset
    // this previously fabricated an invalid-UTF-8 &str: UB in release, a
    // debug_assert panic in debug. The parser must not produce a CData token.
    let tokens = tokenize("<![ABCDE\u{20ac}]]>");
    assert!(!tokens.iter().any(|t| matches!(t, Token::CData { .. })));
}
