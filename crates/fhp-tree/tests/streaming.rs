//! Integration tests for streaming, early-stop, and async parsing.

#![cfg(feature = "encoding")]

use fhp_core::tag::Tag;
use fhp_tree::streaming::{EarlyStopParser, ParseStatus, StreamParser, parse_stream};
use fhp_tree::{parse, parse_bytes};

// ---------------------------------------------------------------------------
// Streaming vs one-shot equivalence
// ---------------------------------------------------------------------------

/// Helper: parse HTML both one-shot and streaming, assert same text content.
fn assert_stream_equiv(html: &[u8], chunk_size: usize) {
    let one_shot = parse_bytes(html).unwrap();
    let streamed = parse_stream(html.chunks(chunk_size)).unwrap();

    let one_text = one_shot.root().text_content();
    let stream_text = streamed.root().text_content();
    assert_eq!(
        one_text, stream_text,
        "chunk_size={chunk_size}: one-shot text != streaming text"
    );

    assert_eq!(
        one_shot.node_count(),
        streamed.node_count(),
        "chunk_size={chunk_size}: node count mismatch"
    );
}

const SAMPLE_HTML: &[u8] =
    b"<html><head><title>Test</title></head><body><div class=\"main\"><p>Hello <b>world</b></p><ul><li>one</li><li>two</li><li>three</li></ul></div></body></html>";

#[test]
fn stream_equiv_chunk_1() {
    assert_stream_equiv(SAMPLE_HTML, 1);
}

#[test]
fn stream_equiv_chunk_7() {
    assert_stream_equiv(SAMPLE_HTML, 7);
}

#[test]
fn stream_equiv_chunk_64() {
    assert_stream_equiv(SAMPLE_HTML, 64);
}

#[test]
fn stream_equiv_chunk_1024() {
    assert_stream_equiv(SAMPLE_HTML, 1024);
}

#[test]
fn stream_equiv_chunk_65536() {
    assert_stream_equiv(SAMPLE_HTML, 65536);
}

// ---------------------------------------------------------------------------
// StreamParser API tests
// ---------------------------------------------------------------------------

#[test]
fn stream_parser_basic() {
    let mut parser = StreamParser::new();
    parser.feed(b"<div>");
    parser.feed(b"<p>Hello</p>");
    parser.feed(b"</div>");
    let doc = parser.finish().unwrap();
    assert_eq!(doc.root().text_content(), "Hello");
}

#[test]
fn stream_parser_complex_html() {
    let html = b"<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Page</title></head><body><h1>Header</h1><p>Paragraph with <a href=\"#\">link</a></p></body></html>";
    let doc = parse_stream(html.chunks(13)).unwrap();
    let text = doc.root().text_content();
    assert!(text.contains("Header"), "text: {text}");
    assert!(text.contains("Paragraph"), "text: {text}");
    assert!(text.contains("link"), "text: {text}");
}

#[test]
fn stream_parser_empty_input() {
    let parser = StreamParser::new();
    let doc = parser.finish().unwrap();
    assert!(!doc.root().has_children());
}

#[test]
fn stream_parser_attributes_preserved() {
    let html = b"<a href=\"https://example.com\" class=\"link\">click</a>";
    let doc = parse_stream(html.chunks(10)).unwrap();
    let root = doc.root();
    let a = root.first_child().unwrap();
    assert_eq!(a.tag(), Tag::A);
    assert_eq!(a.attr("href"), Some("https://example.com"));
    assert!(a.has_class("link"));
}

#[test]
fn stream_parser_preserves_raw_text_across_chunk_boundaries() {
    let prefix = vec![b'x'; 1024];
    let suffix = b"<script>if(a<b){x()}</script><p>ok</p>";

    let mut html = prefix.clone();
    html.extend_from_slice(suffix);

    let one_shot = parse_bytes(&html).unwrap();

    let mut parser = StreamParser::new();
    parser.feed(&prefix);
    parser.feed(b"<script>");
    parser.feed(b"if(a<b){x()}");
    parser.feed(b"</script><p>ok</p>");
    let streamed = parser.finish().unwrap();

    assert_eq!(
        streamed.root().text_content(),
        one_shot.root().text_content()
    );
    assert_eq!(streamed.node_count(), one_shot.node_count());
    assert!(streamed.to_html().contains("<script>if(a<b){x()}</script>"));
}

#[derive(Debug)]
struct StreamingEdgeCase {
    name: &'static str,
    html: &'static [u8],
    expected_text: &'static str,
    expected_html: &'static str,
}

#[test]
fn stream_parser_preserves_lt_edge_cases_across_chunk_boundaries() {
    let cases = [
        StreamingEdgeCase {
            name: "invalid_close_like_and_lt_space",
            html: b"<p>a </ b and 1 < 2</p><p>ok</p>",
            expected_text: "a </ b and 1 < 2ok",
            expected_html: "<p>a &lt;/ b and 1 &lt; 2</p><p>ok</p>",
        },
        StreamingEdgeCase {
            name: "double_lt_and_slash_digit",
            html: b"<p>a << b and a </2</p><span>next</span>",
            expected_text: "a << b and a </2next",
            expected_html: "<p>a &lt;&lt; b and a &lt;/2</p><span>next</span>",
        },
        StreamingEdgeCase {
            name: "script_raw_text",
            html: b"<script>if (a </ b && c < 3) { s = \"&amp;\"; }</script><p>ok</p>",
            expected_text: "if (a </ b && c < 3) { s = \"&amp;\"; }ok",
            expected_html: "<script>if (a </ b && c < 3) { s = \"&amp;\"; }</script><p>ok</p>",
        },
        StreamingEdgeCase {
            name: "style_raw_text",
            html: b"<style>.x::before { content: \"&lt;\"; }</style><p>ok</p>",
            expected_text: ".x::before { content: \"&lt;\"; }ok",
            expected_html: "<style>.x::before { content: \"&lt;\"; }</style><p>ok</p>",
        },
    ];
    let chunk_sizes = [1, 2, 3, 5, 8, 13, 64];

    for case in cases {
        let one_shot = parse_bytes(case.html).unwrap();
        assert_eq!(
            one_shot.root().text_content(),
            case.expected_text,
            "case={}, html={}",
            case.name,
            String::from_utf8_lossy(case.html)
        );
        assert_eq!(
            one_shot.to_html(),
            case.expected_html,
            "case={}, html={}",
            case.name,
            String::from_utf8_lossy(case.html)
        );

        for chunk_size in chunk_sizes {
            let streamed = parse_stream(case.html.chunks(chunk_size)).unwrap();

            assert_eq!(
                streamed.root().text_content(),
                case.expected_text,
                "case={}, chunk_size={chunk_size}, html={}",
                case.name,
                String::from_utf8_lossy(case.html)
            );
            assert_eq!(
                streamed.to_html(),
                case.expected_html,
                "case={}, chunk_size={chunk_size}, html={}",
                case.name,
                String::from_utf8_lossy(case.html)
            );
            assert_eq!(
                streamed.to_html(),
                one_shot.to_html(),
                "case={}, chunk_size={chunk_size}, html={}",
                case.name,
                String::from_utf8_lossy(case.html)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Early termination
// ---------------------------------------------------------------------------

#[test]
fn early_stop_finds_first_a_tag() {
    let html = b"<div><p>paragraph</p><a href=\"/page\">link</a><span>after</span></div>";
    let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
    match parser.feed(html) {
        ParseStatus::Found(_id) => {
            // Successfully found the <a> tag.
        }
        other => panic!("expected Found, got {other:?}"),
    }
}

#[test]
fn early_stop_multi_chunk() {
    let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);

    // First chunk has no <a>.
    let status = parser.feed(b"<div><p>text</p><ul><li>item</li></ul>");
    assert!(matches!(status, ParseStatus::NeedMore));

    // Second chunk introduces <a>.
    let status = parser.feed(b"<a href=\"#\">link</a></div>");
    assert!(matches!(status, ParseStatus::Found(_)));
}

#[test]
fn early_stop_no_match_returns_done() {
    let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
    let html = b"<div><p>no links</p><span>at all</span></div>";
    parser.feed(html);
    match parser.finish() {
        ParseStatus::Done(doc) => {
            let text = doc.root().text_content();
            assert!(text.contains("no links"), "text: {text}");
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn early_stop_subsequent_feed_after_found() {
    let mut parser = EarlyStopParser::stop_when(|node| node.tag == Tag::A);
    let html = b"<a>link</a>";
    assert!(matches!(parser.feed(html), ParseStatus::Found(_)));
    // Feeding more data after Found should still return Found.
    assert!(matches!(parser.feed(b"<p>more</p>"), ParseStatus::Found(_)));
}

#[test]
fn early_stop_by_text_flag() {
    use fhp_tree::node::NodeFlags;
    let mut parser = EarlyStopParser::stop_when(|node| node.flags.has(NodeFlags::IS_TEXT));
    let html = b"<div>some text</div>";
    match parser.feed(html) {
        ParseStatus::Found(_) => {}
        other => panic!("expected Found for text node, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Encoding + streaming
// ---------------------------------------------------------------------------

#[test]
fn stream_utf8_with_bom() {
    let html = b"\xEF\xBB\xBF<div><p>BOM test</p></div>";
    let doc = parse_stream(html.chunks(10)).unwrap();
    let text = doc.root().text_content();
    assert!(text.contains("BOM test"), "text: {text}");
    // BOM should not appear as text content.
    assert!(!text.contains('\u{FEFF}'), "BOM character in text: {text}");
}

#[test]
fn stream_windows_1254_meta() {
    // Turkish ş=0xFE, ğ=0xF0 in Windows-1254.
    let mut html = b"<meta charset=\"windows-1254\"><body>".to_vec();
    html.extend_from_slice(&[0xFE, 0xF0]); // ş, ğ
    html.extend_from_slice(b"</body>");

    let doc = parse_stream(html.chunks(15)).unwrap();
    let text = doc.root().text_content();
    assert!(text.contains('ş'), "text: {text}");
    assert!(text.contains('ğ'), "text: {text}");
}

#[test]
fn stream_utf16le_chunks() {
    // UTF-16 LE BOM + "<p>Hi</p>" in UTF-16 LE.
    let mut bytes = vec![0xFF, 0xFE]; // BOM
    for &ch in b"<p>Hi</p>" {
        bytes.push(ch);
        bytes.push(0x00);
    }

    let doc = parse_stream(bytes.chunks(6)).unwrap();
    let text = doc.root().text_content();
    assert!(text.contains("Hi"), "text: {text}");
}

#[test]
fn stream_utf16be_chunks() {
    // UTF-16 BE BOM + "<p>Hi</p>" in UTF-16 BE.
    let mut bytes = vec![0xFE, 0xFF]; // BOM
    for &ch in b"<p>Hi</p>" {
        bytes.push(0x00);
        bytes.push(ch);
    }

    let doc = parse_stream(bytes.chunks(8)).unwrap();
    let text = doc.root().text_content();
    assert!(text.contains("Hi"), "text: {text}");
}

// ---------------------------------------------------------------------------
// parse_stream convenience
// ---------------------------------------------------------------------------

#[test]
fn parse_stream_vs_parse() {
    let html = "<html><body><div><p>Hello</p></div></body></html>";
    let one_shot = parse(html).unwrap();
    let streamed = parse_stream(html.as_bytes().chunks(11)).unwrap();
    assert_eq!(
        one_shot.root().text_content(),
        streamed.root().text_content()
    );
    assert_eq!(one_shot.node_count(), streamed.node_count());
}

// ---------------------------------------------------------------------------
// Async tests (behind async-tokio feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "async-tokio")]
mod async_tests {
    use fhp_tree::async_parser::{AsyncParser, parse_async};

    #[tokio::test]
    async fn async_parse_basic() {
        let html = b"<div><p>Hello async</p></div>";
        let doc = parse_async(&html[..]).await.unwrap();
        assert_eq!(doc.root().text_content(), "Hello async");
    }

    #[tokio::test]
    async fn async_parse_complex() {
        let html = b"<html><head><title>Async</title></head><body><p>World</p></body></html>";
        let doc = AsyncParser::new(&html[..])
            .with_buf_size(8)
            .parse()
            .await
            .unwrap();
        let text = doc.root().text_content();
        assert!(text.contains("Async"), "text: {text}");
        assert!(text.contains("World"), "text: {text}");
    }

    #[tokio::test]
    async fn async_parse_empty() {
        let html: &[u8] = b"";
        let doc = parse_async(html).await.unwrap();
        assert!(!doc.root().has_children());
    }

    #[tokio::test]
    async fn async_parse_encoding() {
        // UTF-8 BOM + HTML.
        let html = b"\xEF\xBB\xBF<div>BOM async</div>";
        let doc = AsyncParser::new(&html[..]).parse().await.unwrap();
        let text = doc.root().text_content();
        assert!(text.contains("BOM async"), "text: {text}");
    }

    #[tokio::test]
    async fn async_parse_utf16le() {
        let mut bytes = vec![0xFF, 0xFE]; // BOM
        for &ch in b"<p>Async16</p>" {
            bytes.push(ch);
            bytes.push(0x00);
        }

        let doc = AsyncParser::new(bytes.as_slice())
            .with_buf_size(4)
            .parse()
            .await
            .unwrap();
        let text = doc.root().text_content();
        assert!(text.contains("Async16"), "text: {text}");
    }
}
