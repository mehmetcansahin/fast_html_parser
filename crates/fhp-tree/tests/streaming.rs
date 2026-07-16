//! Integration tests for streaming, early-stop, and async parsing.

#![cfg(feature = "encoding")]

use std::fmt::Write as _;

use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;
use fhp_tree::builder::TreeBuilder;
use fhp_tree::node::{NodeFlags, NodeId};
use fhp_tree::streaming::{
    EarlyStopOutcome, EarlyStopParser, EarlyStopProgress, StreamParser, parse_stream,
    parse_stream_with_limit,
};
use fhp_tree::{parse, parse_bytes};

fn canonical_arena(arena: &Arena, root: NodeId) -> String {
    fn push_field(output: &mut String, kind: char, value: &str) {
        write!(output, "{kind}{}:{value}", value.len()).unwrap();
    }

    fn walk(arena: &Arena, id: NodeId, output: &mut String) {
        let node = arena.get(id);
        if node.flags.has(NodeFlags::IS_TEXT) {
            push_field(output, 'T', arena.text(id));
            return;
        }
        if node.flags.has(NodeFlags::IS_COMMENT) {
            push_field(output, 'C', arena.text(id));
            return;
        }
        if node.flags.has(NodeFlags::IS_DOCTYPE) {
            push_field(output, 'D', arena.text(id));
            return;
        }

        let name = node
            .tag
            .as_str()
            .or_else(|| arena.unknown_tag_name(id))
            .unwrap_or("#root");
        push_field(output, 'E', name);
        let mut attributes: Vec<_> = arena
            .attrs(id)
            .iter()
            .map(|attribute| {
                (
                    arena.attr_name(attribute).to_ascii_lowercase(),
                    arena.attr_value(attribute).unwrap_or(""),
                    arena.attr_value(attribute).is_some(),
                )
            })
            .collect();
        attributes.sort_unstable();
        for (name, value, has_value) in attributes {
            push_field(output, 'A', &name);
            output.push(if has_value { '=' } else { '?' });
            push_field(output, 'V', value);
        }
        output.push('[');
        let mut child = node.first_child;
        while !child.is_null() {
            walk(arena, child, output);
            child = arena.get(child).next_sibling;
        }
        output.push(']');
    }

    let mut output = String::new();
    walk(arena, root, &mut output);
    output
}

// ---------------------------------------------------------------------------
// Streaming vs one-shot equivalence
// ---------------------------------------------------------------------------

/// Parse HTML through every synchronous entry point and require the same DOM.
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
        one_shot.to_html(),
        streamed.to_html(),
        "chunk_size={chunk_size}: canonical DOM mismatch"
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
    parser.feed(b"<div>").unwrap();
    parser.feed(b"<p>Hello</p>").unwrap();
    parser.feed(b"</div>").unwrap();
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
fn one_shot_owned_sink_and_streaming_build_the_same_dom() {
    let html = "<table><tr><td>A</td></tr>outside</table><p id='first' ID='later'>x<p>y";
    let one_shot = parse(html).unwrap();
    let owned = fhp_tree::parse_owned(html.to_owned()).unwrap();
    let streamed = parse_stream(html.as_bytes().chunks(3)).unwrap();

    let mut builder = TreeBuilder::new();
    builder.set_source(html);
    fhp_tokenizer::tokenize_into(html, &mut builder);
    let (sink_arena, sink_root) = builder.finish().unwrap();

    let expected = canonical_arena(one_shot.arena(), one_shot.root().id());
    assert_eq!(canonical_arena(owned.arena(), owned.root().id()), expected);
    assert_eq!(
        canonical_arena(streamed.arena(), streamed.root().id()),
        expected
    );
    assert_eq!(canonical_arena(&sink_arena, sink_root), expected);
}

#[test]
fn depth_error_is_identical_for_one_shot_owned_sink_and_streaming() {
    let html = format!("{}x", "<div>".repeat(513));
    let is_depth_error = |result: Result<fhp_tree::Document, fhp_tree::HtmlError>| {
        matches!(
            result,
            Err(fhp_tree::HtmlError::Parse(
                fhp_core::error::ParseError::NestingTooDeep {
                    depth: 513,
                    limit: 512
                }
            ))
        )
    };

    assert!(is_depth_error(parse(&html)));
    assert!(is_depth_error(fhp_tree::parse_owned(html.clone())));
    assert!(is_depth_error(parse_stream(html.as_bytes().chunks(17))));

    let mut builder = TreeBuilder::new();
    builder.set_source(&html);
    fhp_tokenizer::tokenize_into(&html, &mut builder);
    assert!(matches!(
        builder.finish(),
        Err(fhp_core::error::ParseError::NestingTooDeep {
            depth: 513,
            limit: 512
        })
    ));
}

#[test]
fn early_stop_does_not_tokenize_later_internal_blocks() {
    let mut html = b"<a href='/match'>match</a>".to_vec();
    html.resize(64 * 1024, b'x');
    html.extend_from_slice("<div>".repeat(513).as_bytes());

    let mut parser = EarlyStopParser::stop_on_create(|node| node.tag() == Tag::A);
    assert_eq!(parser.feed(&html).unwrap(), EarlyStopProgress::Matched);
    let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
        panic!("expected early match")
    };
    assert_eq!(found.node().attr("href"), Some("/match"));
}

#[test]
fn iterator_stops_reading_after_first_terminal_error() {
    use std::cell::Cell;
    use std::rc::Rc;

    struct CountingChunks<'a> {
        chunks: std::vec::IntoIter<&'a [u8]>,
        reads: Rc<Cell<usize>>,
    }

    impl<'a> Iterator for CountingChunks<'a> {
        type Item = &'a [u8];

        fn next(&mut self) -> Option<Self::Item> {
            let next = self.chunks.next();
            if next.is_some() {
                self.reads.set(self.reads.get() + 1);
            }
            next
        }
    }

    let reads = Rc::new(Cell::new(0));
    let chunks: Vec<&[u8]> = vec![b"1234", b"5678", b"unread"];
    let result = parse_stream_with_limit(
        CountingChunks {
            chunks: chunks.into_iter(),
            reads: Rc::clone(&reads),
        },
        4,
    );
    assert!(matches!(
        result,
        Err(fhp_tree::HtmlError::InputTooLarge { size: 8, max: 4 })
    ));
    assert_eq!(reads.get(), 2);
}

#[test]
fn stream_parser_preserves_raw_text_across_chunk_boundaries() {
    let prefix = vec![b'x'; 1024];
    let suffix = b"<script>if(a<b){x()}</script><p>ok</p>";

    let mut html = prefix.clone();
    html.extend_from_slice(suffix);

    let one_shot = parse_bytes(&html).unwrap();

    let mut parser = StreamParser::new();
    parser.feed(&prefix).unwrap();
    parser.feed(b"<script>").unwrap();
    parser.feed(b"if(a<b){x()}").unwrap();
    parser.feed(b"</script><p>ok</p>").unwrap();
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
    let mut parser = EarlyStopParser::stop_on_create(|node| node.tag() == Tag::A);
    assert_eq!(parser.feed(html).unwrap(), EarlyStopProgress::Matched);
    let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
        panic!("expected a match")
    };
    assert_eq!(found.node().tag(), Tag::A);
}

#[test]
fn early_stop_multi_chunk() {
    let mut parser = EarlyStopParser::stop_on_create(|node| node.tag() == Tag::A);

    // First chunk has no <a>.
    let status = parser
        .feed(b"<div><p>text</p><ul><li>item</li></ul>")
        .unwrap();
    assert_eq!(status, EarlyStopProgress::NeedMore);

    // Second chunk introduces <a>.
    let status = parser.feed(b"<a href=\"#\">link</a></div>").unwrap();
    assert_eq!(status, EarlyStopProgress::Matched);
}

#[test]
fn early_stop_no_match_returns_done() {
    let mut parser = EarlyStopParser::stop_on_create(|node| node.tag() == Tag::A);
    let html = b"<div><p>no links</p><span>at all</span></div>";
    parser.feed(html).unwrap();
    match parser.finish().unwrap() {
        EarlyStopOutcome::Done(doc) => {
            let text = doc.root().text_content();
            assert!(text.contains("no links"), "text: {text}");
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn early_stop_subsequent_feed_after_found() {
    let mut parser = EarlyStopParser::stop_on_create(|node| node.tag() == Tag::A);
    let html = b"<a>link</a>";
    assert_eq!(parser.feed(html).unwrap(), EarlyStopProgress::Matched);
    // Feeding more data after Found should still return Found.
    assert_eq!(
        parser.feed(b"<p>more</p>").unwrap(),
        EarlyStopProgress::Matched
    );
}

#[test]
fn early_stop_by_text_flag() {
    let mut parser = EarlyStopParser::stop_on_create(|node| node.is_text());
    let html = b"<div>some text</div>";
    assert_eq!(parser.feed(html).unwrap(), EarlyStopProgress::Matched);
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
    use std::collections::VecDeque;
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use fhp_tree::HtmlError;
    use fhp_tree::async_parser::{AsyncParser, parse_async};
    use tokio::io::{AsyncRead, ReadBuf};

    struct CountingReader {
        chunks: VecDeque<&'static [u8]>,
        reads: Arc<AtomicUsize>,
    }

    impl AsyncRead for CountingReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(chunk) = self.chunks.pop_front() {
                buf.put_slice(chunk);
            }
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn async_parse_basic() {
        let html = b"<div><p>Hello async</p></div>";
        let doc = parse_async(&html[..]).await.unwrap();
        assert_eq!(doc.root().text_content(), "Hello async");
    }

    #[tokio::test]
    async fn async_parse_complex() {
        let html = b"<html><head><title>Async</title></head><body><p>World</p></body></html>";
        let expected = fhp_tree::parse_bytes(html).unwrap().to_html();
        let doc = AsyncParser::new(&html[..])
            .with_buf_size(8)
            .parse()
            .await
            .unwrap();
        assert_eq!(doc.to_html(), expected);
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

    #[tokio::test]
    async fn async_parser_stops_reading_after_size_error() {
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = CountingReader {
            chunks: VecDeque::from([&b"1234"[..], &b"5678"[..], &b"ignored"[..]]),
            reads: Arc::clone(&reads),
        };

        let result = AsyncParser::new(reader)
            .with_buf_size(8)
            .with_max_input_size(4)
            .parse()
            .await;
        assert!(matches!(
            result,
            Err(HtmlError::InputTooLarge { size: 8, max: 4 })
        ));
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn async_parser_returns_the_same_typed_depth_error() {
        let html = format!("{}x", "<div>".repeat(513));
        let result = AsyncParser::new(html.as_bytes())
            .with_buf_size(19)
            .parse()
            .await;
        assert!(matches!(
            result,
            Err(HtmlError::Parse(
                fhp_core::error::ParseError::NestingTooDeep {
                    depth: 513,
                    limit: 512
                }
            ))
        ));
    }
}

#[cfg(feature = "async-async-std")]
mod async_std_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    use fhp_tree::HtmlError;
    use fhp_tree::async_std_parser::AsyncStdParser;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    #[test]
    fn async_std_matches_one_shot_dom() {
        block_on(async {
            let html = b"<table><tr><td>A</td><td>B</table><p>x<p>y";
            let expected = fhp_tree::parse_bytes(html).unwrap().to_html();
            let actual = AsyncStdParser::new(&html[..])
                .with_buf_size(5)
                .parse()
                .await
                .unwrap();
            assert_eq!(actual.to_html(), expected);
        });
    }

    #[test]
    fn async_std_returns_the_same_typed_depth_error() {
        block_on(async {
            let html = format!("{}x", "<div>".repeat(513));
            let result = AsyncStdParser::new(html.as_bytes())
                .with_buf_size(23)
                .parse()
                .await;
            assert!(matches!(
                result,
                Err(HtmlError::Parse(
                    fhp_core::error::ParseError::NestingTooDeep {
                        depth: 513,
                        limit: 512
                    }
                ))
            ));
        });
    }
}
