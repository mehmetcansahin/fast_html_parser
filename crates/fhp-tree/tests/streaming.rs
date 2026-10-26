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
use fhp_tree::{NodeRef, parse, parse_bytes};

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
fn cdata_text_is_literal_on_every_entry_point() {
    let html = "<p><![CDATA[x&amp;y]]></p>";
    let expected = "x&amp;y";

    assert_eq!(parse(html).unwrap().root().text_content(), expected);
    assert_eq!(
        fhp_tree::parse_owned(html.to_owned())
            .unwrap()
            .root()
            .text_content(),
        expected
    );
    assert_eq!(
        parse_bytes(html.as_bytes()).unwrap().root().text_content(),
        expected
    );

    for chunk_size in [1, 5, html.len()] {
        let mut stream = StreamParser::new();
        for chunk in html.as_bytes().chunks(chunk_size) {
            stream.feed(chunk).unwrap();
        }
        assert_eq!(
            stream.finish().unwrap().root().text_content(),
            expected,
            "chunk_size={chunk_size}"
        );

        let mut early = EarlyStopParser::stop_after_element(|node| node.tag() == Tag::P);
        let mut progress = EarlyStopProgress::NeedMore;
        for chunk in html.as_bytes().chunks(chunk_size) {
            progress = early.feed(chunk).unwrap();
            if progress == EarlyStopProgress::Matched {
                break;
            }
        }
        assert_eq!(
            progress,
            EarlyStopProgress::Matched,
            "chunk_size={chunk_size}"
        );
        let EarlyStopOutcome::Matched(found) = early.finish().unwrap() else {
            panic!("the closed paragraph must match");
        };
        assert_eq!(
            found.node().text_content(),
            expected,
            "chunk_size={chunk_size}"
        );
    }
}

#[test]
fn raw_text_of_an_ignored_start_tag_decodes_on_every_entry_point() {
    // `select` ignores these start tags, so their content has no raw-text
    // parent; every entry point decodes it like ordinary text.
    let mut inputs: Vec<String> = ["xmp", "noembed", "noframes", "style", "iframe"]
        .iter()
        .map(|tag| format!("<select><option>a<{tag}>&amp;</{tag}>b</select>"))
        .collect();
    inputs.push("<select><option>a<plaintext>&amp;b".to_owned());
    let expected = if cfg!(feature = "entity-decode") {
        "a&b"
    } else {
        "a&amp;b"
    };

    for html in &inputs {
        assert_eq!(
            parse(html).unwrap().root().text_content(),
            expected,
            "{html}"
        );
        for chunk_size in [1, 7, html.len()] {
            let streamed = parse_stream(html.as_bytes().chunks(chunk_size)).unwrap();
            assert_eq!(
                streamed.root().text_content(),
                expected,
                "{html}, chunk_size={chunk_size}"
            );

            let mut early = EarlyStopParser::stop_after_element(|node| node.tag() == Tag::Select);
            for chunk in html.as_bytes().chunks(chunk_size) {
                if early.feed(chunk).unwrap() == EarlyStopProgress::Matched {
                    break;
                }
            }
            let EarlyStopOutcome::Matched(found) = early.finish().unwrap() else {
                panic!("the select must match: {html}");
            };
            assert_eq!(
                found.node().text_content(),
                expected,
                "{html}, chunk_size={chunk_size}"
            );
        }
    }
}

#[test]
fn unfinished_raw_close_tag_is_dropped_at_eof() {
    // EOF inside the terminated end-tag name drops the end tag (WHATWG
    // "eof-in-tag"); EOF then closes the element with only its text.
    for tag in ["script", "style", "iframe", "title", "textarea"] {
        let html = format!("<{tag}>x</{tag} &amp;");
        let expected_text = "x";
        let one_shot = parse(&html).unwrap();
        assert_eq!(one_shot.root().text_content(), expected_text);
        let expected = canonical_arena(one_shot.arena(), one_shot.root().id());

        for chunk_size in [1, 7, html.len()] {
            let streamed = parse_stream(html.as_bytes().chunks(chunk_size)).unwrap();
            assert_eq!(
                canonical_arena(streamed.arena(), streamed.root().id()),
                expected,
                "tag={tag}, chunk_size={chunk_size}"
            );

            let tag_id = Tag::from_bytes(tag.as_bytes());
            let mut early = EarlyStopParser::stop_after_element(move |node| node.tag() == tag_id);
            for chunk in html.as_bytes().chunks(chunk_size) {
                assert_eq!(early.feed(chunk).unwrap(), EarlyStopProgress::NeedMore);
            }
            let EarlyStopOutcome::Matched(found) = early.finish().unwrap() else {
                panic!("EOF must complete the matched element");
            };
            assert_eq!(found.node().text_content(), expected_text);
        }
    }
}

#[test]
fn depth_limit_is_identical_for_one_shot_owned_sink_and_streaming() {
    // Past 512 open elements a start tag inserts an empty element, raw-text
    // ones included; what follows lands in the deepest open element.
    let html = format!(
        "{}<span>x</span><script>a&amp;b</script><p>y",
        "<div>".repeat(512)
    );
    let one_shot = parse(&html).unwrap();
    let owned = fhp_tree::parse_owned(html.clone()).unwrap();
    let streamed = parse_stream(html.as_bytes().chunks(17)).unwrap();
    let mut builder = TreeBuilder::new();
    fhp_tokenizer::tokenize_into(&html, &mut builder);
    let (sink_arena, sink_root) = builder.finish().unwrap();

    let expected = canonical_arena(one_shot.arena(), one_shot.root().id());
    assert_eq!(canonical_arena(owned.arena(), owned.root().id()), expected);
    assert_eq!(
        canonical_arena(streamed.arena(), streamed.root().id()),
        expected
    );
    assert_eq!(canonical_arena(&sink_arena, sink_root), expected);

    let deepest = (0..512).fold(one_shot.root(), |node, _| node.first_child().unwrap());
    let children: Vec<_> = deepest
        .children()
        .map(|child| one_shot.get(child))
        .collect();
    assert_eq!(children.len(), 6);
    assert_eq!(children[0].tag(), Tag::Span);
    assert_eq!(children[1].text(), "x");
    assert_eq!(children[2].tag(), Tag::Script);
    let script_text = if cfg!(feature = "entity-decode") {
        "a&b"
    } else {
        "a&amp;b"
    };
    assert_eq!(children[3].text(), script_text);
    assert_eq!(children[4].tag(), Tag::P);
    assert_eq!(children[5].text(), "y");
    assert!(children.iter().all(|child| !child.has_children()));
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
            expected_text: "a ok",
            expected_html: "<p>a <!-- b and 1 < 2</p--></p><p>ok</p>",
        },
        StreamingEdgeCase {
            name: "double_lt_and_slash_digit",
            html: b"<p>a << b and a </2</p><span>next</span>",
            expected_text: "a << b and a next",
            expected_html: "<p>a &lt;&lt; b and a <!--2</p--><span>next</span></p>",
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

#[test]
fn early_stop_observes_implied_table_elements() {
    for (tag, html) in [
        (Tag::Tbody, &b"<table><tr><td>x</td></tr></table>"[..]),
        (Tag::Tr, &b"<table><td>x</td></table>"[..]),
    ] {
        let mut parser = EarlyStopParser::stop_on_create(move |node| node.tag() == tag);
        assert_eq!(parser.feed(html).unwrap(), EarlyStopProgress::Matched);
        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("implied {tag:?} was not observed")
        };
        assert_eq!(found.node().tag(), tag);
    }

    // The implied `colgroup` is the only unknown element inside the table.
    let mut parser = EarlyStopParser::stop_on_create(|node| {
        node.tag() == Tag::Unknown
            && node
                .parent()
                .is_some_and(|parent| parent.tag() == Tag::Table)
    });
    assert_eq!(
        parser.feed(b"<table><col></table>").unwrap(),
        EarlyStopProgress::Matched
    );
}

#[test]
fn early_stop_observes_adoption_agency_clones() {
    type Predicate = fn(NodeRef<'_>) -> bool;
    fn child_of(node: NodeRef<'_>, tag: Tag, parent: Tag) -> bool {
        node.tag() == tag
            && node
                .parent()
                .is_some_and(|parent_node| parent_node.tag() == parent)
    }

    // Each predicate matches only a clone: the formatting element's
    // replacement inside the furthest block, a clone wrapping the moved
    // block, and a formatting element reopened after a misnested end tag.
    let cases: [(&str, Predicate, &str); 3] = [
        (
            "<b><p>one</b>two</p>",
            |node| child_of(node, Tag::B, Tag::P),
            "<b>one</b>",
        ),
        (
            "<a><b><div>x</a>y</b>",
            |node| node.tag() == Tag::B && node.depth() == 1,
            "<b></b>",
        ),
        (
            "<div><i><b>x</i>y</b></div>",
            |node| child_of(node, Tag::B, Tag::Div),
            "<b>y</b>",
        ),
    ];
    for (html, predicate, complete) in cases {
        let document = parse(html).unwrap();
        assert!(
            document
                .root()
                .descendants()
                .any(|id| predicate(document.get(id))),
            "{html}: parse finds no match"
        );

        let mut parser = EarlyStopParser::stop_on_create(predicate);
        assert_eq!(
            parser.feed(html.as_bytes()).unwrap(),
            EarlyStopProgress::Matched
        );
        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("{html}: no match on create");
        };
        assert!(predicate(found.node()), "{html}");

        let mut parser = EarlyStopParser::stop_after_element(predicate);
        parser.feed(html.as_bytes()).unwrap();
        let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
            panic!("{html}: no match after the element");
        };
        assert_eq!(found.node().outer_html(), complete, "{html}");
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
    async fn async_parser_applies_the_same_depth_limit() {
        let html = format!("{}x<p>y", "<div>".repeat(513));
        let expected = fhp_tree::parse(&html).unwrap().to_html();
        let doc = AsyncParser::new(html.as_bytes())
            .with_buf_size(19)
            .parse()
            .await
            .unwrap();
        assert_eq!(doc.to_html(), expected);
    }
}

#[cfg(feature = "async-async-std")]
mod async_std_tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use fhp_tree::async_std_parser::AsyncStdParser;

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
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
    fn async_std_applies_the_same_depth_limit() {
        block_on(async {
            let html = format!("{}x<p>y", "<div>".repeat(513));
            let expected = fhp_tree::parse(&html).unwrap().to_html();
            let actual = AsyncStdParser::new(html.as_bytes())
                .with_buf_size(23)
                .parse()
                .await
                .unwrap();
            assert_eq!(actual.to_html(), expected);
        });
    }
}

/// Tokenizer review fixtures (WHATWG tag grammar, bogus comments, EOF
/// handling, legacy raw text, script escapes) and tree-builder conformance
/// fixtures: streaming must build the one-shot DOM at every chunk size.
#[test]
fn stream_equiv_review_fixtures() {
    const FIXTURES: &[&str] = &[
        // Tokenizer.
        "<img alt=it's><p>visible</p>",
        "<div a\"b>x</div>",
        "<a b=x'y c='z>w'>",
        "<a href=http://example.com/>",
        "<a b=/>",
        "<a b=\"x\"/>",
        "<br/>",
        "<?xml version=\"1.0\"?>",
        "<!x y>",
        "<![if !IE]>",
        "<p>a</ b>c</p>",
        "<p>a</>c</p>",
        "<_foo>",
        "<!-- c --!><p>",
        "<!DOCTYPE html \"a>b\">",
        "<!-- x",
        "<!-- x --",
        "<![CDATA[x",
        "<!DOCTYPE html",
        "<div class=\"a",
        "<?php",
        "<xmp><b>x</b></xmp>",
        "<noembed><b>x</b></noembed>",
        "<noframes><b>x</b></noframes>",
        "<script><!--<script>x</script>y--></script><p>",
        // Tree builder.
        "<ul><li>a<ul><li>b</li></ul></li></ul>",
        "<ul><li>a<ol><li>b<li>c</ol><li>d</ul>",
        "<dl><dt>a<dd><dl><dt>b</dl></dl>",
        "<ul><li>a<table><tr><td><li>b</td></tr></table></li></ul>",
        "<table><tr><td><table><tr><td>inner</td></tr></table></td><td>x</td></tr></table>",
        "<table><tr><td><div><table><tr><td>i</td></tr></table></div></td></tr></table>",
        "<table><caption>cap</caption><tr><td>x</td></tr></table>",
        "<table><colgroup><col><col></colgroup><tr><td>x</td></tr></table>",
        "<table><col><tr><td>x</td></tr></table>",
        "<table><script>var a=1;</script><tr><td>x</td></tr></table>",
        "<table><style>td{}</style><tr><td>x</td></tr></table>",
        "<table><form><tr><td>x</td></tr></form></table>",
        "<table><input type=hidden name=a><tr><td>x</td></tr></table>",
        "<table><input type=text><tr><td>x</td></tr></table>",
        "<table><div>hello<span>w</span></div><tr><td>x</td></tr></table>",
        "<table><p>para</p><tr><td>x</td></tr></table>",
        "<table><div>x<tr><td>y</td></tr></table>",
        "<div><table><tr><td>cell</div>after</td></tr></table>",
        "<a href=o><table><tr><td><a href=i>x</a></td><td>y</td></tr></table></a>",
        "<table><tr><td><select><option>a</td><td>b</td></tr></table>",
        "<table><tr><td><select><option>a<table><tr><td>b</td></tr></table></td></tr></table>",
        "<h1>a<span>b<h2>c</h2></span></h1>",
        "<option>a<div>b<option>c",
        "<p>a<li>b",
        "<div>a</p>b</div>",
        "<p>a</br>b</p>",
        "<td>x</td>",
        "<table><tr><td>a</td></tr></table><td>b</td>",
        "<select><option>a</option><script>s</script></select>",
        "<pre>\ncode</pre><textarea>\ntext</textarea>",
        "<pre>\r\ncode</pre><textarea>\r\ntext</textarea><pre>\rx</pre>",
        "<b><div>x</b>y</div>",
        "<a href=1><div>t<a href=2>x</a><p>d</p></div></a>",
        "<a><b><i><u><s><div>x</a>y",
        "<a><b><b><b><b>x</a>y",
        "<p>a<blockquote>b</blockquote>c",
        "<p>a<object><p>b</object>c",
        "<html><body>x</body>\n<!--a--></html><!--b--><p>y",
        "<body><select><option>a</body><!--c--></select><table><colgroup><col></html><col>",
        "<img alt=it's><p>visible</p><p>it's done</p>",
        "<div a\"b>x</div><p>y</p>",
        "<?xml version=\"1.0\"?><p>hi</p>",
        "<a href=http://example.com/>x</a>",
        "<a b=/>x</a>",
        "<!-- c --!><p>after</p>",
        "<![if !IE]><p>a</p><![endif]><p>b</p>",
        "<p>a</ b>c</p>",
        "<p>a</>c</p>",
        "<_foo>x</_foo>",
        "<p>x</p><!-- unterminated",
        "<p>x</p><div class=\"a",
        "<xmp><b>x</b></xmp>",
        "<noembed><b>x</b></noembed>",
        "<script><!--<script>x</script>y--></script><p>z</p>",
    ];

    for html in FIXTURES {
        for chunk_size in (1..=9).chain([17]) {
            assert_stream_equiv(html.as_bytes(), chunk_size);
        }
    }
}
