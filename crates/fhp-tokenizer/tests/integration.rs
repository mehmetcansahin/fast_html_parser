//! Comprehensive integration tests for fhp-tokenizer.

use std::borrow::Cow;

use fhp_core::tag::Tag;
use fhp_tokenizer::entity::decode_entities;
use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token;
use fhp_tokenizer::tokenize;

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
