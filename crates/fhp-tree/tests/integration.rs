//! Comprehensive integration tests for fhp-tree.

use fhp_core::tag::Tag;
use fhp_tree::node::NodeId;
use fhp_tree::parse;

// ---------------------------------------------------------------
// Basic tree structure
// ---------------------------------------------------------------

#[test]
fn simple_document() {
    let doc =
        parse("<html><head><title>Test</title></head><body><p>Hello</p></body></html>").unwrap();
    let root = doc.root();
    assert!(root.has_children());

    let text = root.text_content();
    assert!(text.contains("Test"));
    assert!(text.contains("Hello"));
}

#[test]
fn nested_elements() {
    let doc = parse("<div><ul><li>1</li><li>2</li><li>3</li></ul></div>").unwrap();
    let root = doc.root();
    let div = root.first_child().unwrap();
    assert_eq!(div.tag(), Tag::Div);

    let ul = div.first_child().unwrap();
    assert_eq!(ul.tag(), Tag::Ul);

    let li_count = ul.children().count();
    assert_eq!(li_count, 3);
}

#[test]
fn parent_child_relationships() {
    let doc = parse("<div><span>text</span></div>").unwrap();
    let root = doc.root();
    let div = root.first_child().unwrap();
    let span = div.first_child().unwrap();
    let text_node = span.first_child().unwrap();

    assert_eq!(text_node.parent().unwrap().tag(), Tag::Span);
    assert_eq!(span.parent().unwrap().tag(), Tag::Div);
}

// ---------------------------------------------------------------
// Broken HTML recovery
// ---------------------------------------------------------------

#[test]
fn implicit_close_p_p() {
    let doc = parse("<p>first<p>second").unwrap();
    let root = doc.root();
    let children: Vec<_> = root.children().collect();
    let p_count = children
        .iter()
        .filter(|&c| doc.get(*c).tag() == Tag::P)
        .count();
    assert_eq!(p_count, 2);
}

#[test]
fn implicit_close_li_li() {
    let doc = parse("<ul><li>a<li>b<li>c</ul>").unwrap();
    let root = doc.root();
    let ul = root.first_child().unwrap();
    let li_count = ul
        .children()
        .filter(|&c| doc.get(c).tag() == Tag::Li)
        .count();
    assert_eq!(li_count, 3);
}

#[test]
fn unclosed_div() {
    let doc = parse("<div><span>text").unwrap();
    let root = doc.root();
    assert!(root.has_children());
    assert_eq!(root.text_content(), "text");
}

#[test]
fn wrong_nesting() {
    // <div><span></div></span> — div close should close both.
    let doc = parse("<div><span>hi</div></span>").unwrap();
    let root = doc.root();
    assert!(root.text_content().contains("hi"));
}

#[test]
fn extra_close_tags() {
    let doc = parse("</div></span><p>ok</p></div>").unwrap();
    let root = doc.root();
    assert_eq!(root.text_content(), "ok");
}

// ---------------------------------------------------------------
// Void elements
// ---------------------------------------------------------------

#[test]
fn void_br() {
    let doc = parse("<p>a<br>b</p>").unwrap();
    let root = doc.root();
    let p = root.first_child().unwrap();
    let children: Vec<_> = p.children().collect();
    // text "a", <br>, text "b"
    assert_eq!(children.len(), 3);
    assert_eq!(doc.get(children[1]).tag(), Tag::Br);
    assert!(doc.get(children[1]).is_void());
}

#[test]
fn void_img_with_attrs() {
    let doc = parse("<img src=\"x.png\" alt=\"photo\">").unwrap();
    let root = doc.root();
    let img = root.first_child().unwrap();
    assert_eq!(img.tag(), Tag::Img);
    assert!(img.is_void());
    assert_eq!(img.attr("src"), Some("x.png"));
    assert_eq!(img.attr("alt"), Some("photo"));
}

// ---------------------------------------------------------------
// text_content()
// ---------------------------------------------------------------

#[test]
fn text_content_nested() {
    let doc = parse("<div><span>a</span><span>b</span></div>").unwrap();
    let root = doc.root();
    let div = root.first_child().unwrap();
    assert_eq!(div.text_content(), "ab");
}

#[test]
fn text_content_deep() {
    let doc = parse("<div><div><div><span>deep</span></div></div></div>").unwrap();
    let root = doc.root();
    assert_eq!(root.text_content(), "deep");
}

// ---------------------------------------------------------------
// inner_html() / outer_html()
// ---------------------------------------------------------------

#[test]
fn inner_html_basic() {
    let doc = parse("<div><p>Hello</p></div>").unwrap();
    let div = doc.root().first_child().unwrap();
    let inner = div.inner_html();
    assert_eq!(inner, "<p>Hello</p>");
}

#[test]
fn outer_html_basic() {
    let doc = parse("<div><p>Hello</p></div>").unwrap();
    let div = doc.root().first_child().unwrap();
    let outer = div.outer_html();
    assert_eq!(outer, "<div><p>Hello</p></div>");
}

#[test]
fn outer_html_with_attrs() {
    let doc = parse("<a href=\"url\" class=\"x\">link</a>").unwrap();
    let a = doc.root().first_child().unwrap();
    let outer = a.outer_html();
    assert!(outer.contains("href=\"url\""), "outer: {outer}");
    assert!(outer.contains("class=\"x\""), "outer: {outer}");
    assert!(outer.contains(">link</a>"), "outer: {outer}");
}

#[test]
fn outer_html_void() {
    let doc = parse("<br>").unwrap();
    let br = doc.root().first_child().unwrap();
    let outer = br.outer_html();
    assert!(outer.contains("br"), "outer: {outer}");
}

// ---------------------------------------------------------------
// Traversals
// ---------------------------------------------------------------

#[test]
fn depth_first_traversal() {
    let doc = parse("<div><span>a</span><p>b</p></div>").unwrap();
    let root = doc.root();
    let ids: Vec<NodeId> = root.descendants().collect();
    // root, div, span, text(a), p, text(b)
    assert_eq!(ids.len(), 6);
}

#[test]
fn breadth_first_traversal() {
    let doc = parse("<div><span>a</span><p>b</p></div>").unwrap();
    let root = doc.root();
    let ids: Vec<NodeId> = root.descendants_bfs().collect();
    // root, div, span, p, text(a), text(b)
    assert_eq!(ids.len(), 6);
    // In BFS, div comes before span's children.
    assert_eq!(doc.get(ids[1]).tag(), Tag::Div);
}

#[test]
fn children_count() {
    let doc = parse("<table><tr><td>1</td><td>2</td><td>3</td></tr></table>").unwrap();
    let root = doc.root();
    let table = root.first_child().unwrap();
    let tr = table.first_child().unwrap();
    assert_eq!(tr.children().count(), 3);
}

#[test]
fn ancestors_chain() {
    let doc = parse("<div><span><a>text</a></span></div>").unwrap();
    let root = doc.root();
    let div = root.first_child().unwrap();
    let span = div.first_child().unwrap();
    let a = span.first_child().unwrap();

    let ancestor_tags: Vec<Tag> = a.ancestors().map(|id| doc.get(id).tag()).collect();
    assert_eq!(ancestor_tags.len(), 3);
    assert_eq!(ancestor_tags[0], Tag::Span);
    assert_eq!(ancestor_tags[1], Tag::Div);
}

// ---------------------------------------------------------------
// Node alignment
// ---------------------------------------------------------------

#[test]
fn node_size_64() {
    assert_eq!(std::mem::size_of::<fhp_tree::node::Node>(), 64);
}

#[test]
fn node_align_64() {
    assert_eq!(std::mem::align_of::<fhp_tree::node::Node>(), 64);
}

// ---------------------------------------------------------------
// Large input
// ---------------------------------------------------------------

#[test]
fn large_input_1000_spans() {
    let mut html = String::with_capacity(20000);
    html.push_str("<div>");
    for i in 0..1000 {
        html.push_str(&format!("<span>{i}</span>"));
    }
    html.push_str("</div>");

    let doc = parse(&html).unwrap();
    let root = doc.root();
    let div = root.first_child().unwrap();
    let span_count = div
        .children()
        .filter(|&c| doc.get(c).tag() == Tag::Span)
        .count();
    assert_eq!(span_count, 1000);
}

// ---------------------------------------------------------------
// Entity-decoded text
// ---------------------------------------------------------------

#[test]
fn entity_in_text() {
    let doc = parse("<p>a &amp; b</p>").unwrap();
    let root = doc.root();
    let text = root.text_content();
    assert!(text.contains("a & b"), "text: {text}");
}

#[derive(Debug)]
struct TextMarkupCase {
    name: &'static str,
    html: &'static str,
    expected_text: &'static str,
    expected_inner_html: &'static str,
}

#[test]
fn literal_lt_in_text_is_preserved() {
    let doc = parse("<p>1 < 2 && 3 > 0</p>").unwrap();
    let p = doc.root().first_child().unwrap();

    assert_eq!(p.text_content(), "1 < 2 && 3 > 0");
    assert_eq!(p.inner_html(), "1 &lt; 2 &amp;&amp; 3 &gt; 0");
}

#[test]
fn invalid_close_like_text_is_preserved() {
    let doc = parse("<p>a </ b</p>").unwrap();
    let p = doc.root().first_child().unwrap();

    assert_eq!(p.text_content(), "a </ b");
    assert_eq!(p.inner_html(), "a &lt;/ b");
}

#[test]
fn text_markup_edge_cases_round_trip() {
    let cases = [
        TextMarkupCase {
            name: "lt_before_space",
            html: "<p>a < 2</p>",
            expected_text: "a < 2",
            expected_inner_html: "a &lt; 2",
        },
        TextMarkupCase {
            name: "double_lt",
            html: "<p>a << b</p>",
            expected_text: "a << b",
            expected_inner_html: "a &lt;&lt; b",
        },
        TextMarkupCase {
            name: "lt_before_dot",
            html: "<p>a <.b</p>",
            expected_text: "a <.b",
            expected_inner_html: "a &lt;.b",
        },
        TextMarkupCase {
            name: "lt_before_hash",
            html: "<p>a <#b</p>",
            expected_text: "a <#b",
            expected_inner_html: "a &lt;#b",
        },
        TextMarkupCase {
            name: "lt_before_ampersand",
            html: "<p>a <&b</p>",
            expected_text: "a <&b",
            expected_inner_html: "a &lt;&amp;b",
        },
        TextMarkupCase {
            name: "slash_space",
            html: "<p>a </ b</p>",
            expected_text: "a </ b",
            expected_inner_html: "a &lt;/ b",
        },
        TextMarkupCase {
            name: "slash_digit",
            html: "<p>a </2</p>",
            expected_text: "a </2",
            expected_inner_html: "a &lt;/2",
        },
        TextMarkupCase {
            name: "trailing_lt",
            html: "<p>a <</p>",
            expected_text: "a <",
            expected_inner_html: "a &lt;",
        },
        TextMarkupCase {
            name: "lt_equal",
            html: "<p>a <= b</p>",
            expected_text: "a <= b",
            expected_inner_html: "a &lt;= b",
        },
        TextMarkupCase {
            name: "lt_percent",
            html: "<p>a <% b</p>",
            expected_text: "a <% b",
            expected_inner_html: "a &lt;% b",
        },
        TextMarkupCase {
            name: "invalid_lt_then_real_child",
            html: "<p>a < 2 <span>b &amp; c</span></p>",
            expected_text: "a < 2 b & c",
            expected_inner_html: "a &lt; 2 <span>b &amp; c</span>",
        },
    ];

    for case in cases {
        let doc = parse(case.html).unwrap();
        let p = doc.root().first_child().unwrap();

        assert_eq!(
            p.text_content(),
            case.expected_text,
            "case={}, html={}",
            case.name,
            case.html
        );
        assert_eq!(
            p.inner_html(),
            case.expected_inner_html,
            "case={}, html={}",
            case.name,
            case.html
        );
    }
}

#[test]
fn script_entities_remain_raw_text() {
    let doc = parse("<script>const s = \"&amp;\";</script>").unwrap();
    let script = doc.root().first_child().unwrap();

    assert_eq!(script.text_content(), "const s = \"&amp;\";");
    assert_eq!(script.inner_html(), "const s = \"&amp;\";");
}

#[derive(Debug)]
struct RawTextCase {
    name: &'static str,
    html: &'static str,
    expected_tag: Tag,
    expected_text: &'static str,
}

#[test]
fn raw_text_edge_cases_round_trip() {
    let cases = [
        RawTextCase {
            name: "script_invalid_close_like_expression",
            html: "<script>if (a </ b) { x = \"&amp;\"; }</script>",
            expected_tag: Tag::Script,
            expected_text: "if (a </ b) { x = \"&amp;\"; }",
        },
        RawTextCase {
            name: "script_fake_other_close_tag",
            html: "<script>const s = \"</not-script>\";</script>",
            expected_tag: Tag::Script,
            expected_text: "const s = \"</not-script>\";",
        },
        RawTextCase {
            name: "script_prefix_close_name_is_text",
            html: "<script>if (a </scripted) {}</script>",
            expected_tag: Tag::Script,
            expected_text: "if (a </scripted) {}",
        },
        RawTextCase {
            name: "script_uppercase_close",
            html: "<script>const ok = true;</SCRIPT>",
            expected_tag: Tag::Script,
            expected_text: "const ok = true;",
        },
        RawTextCase {
            name: "style_lt_comparison",
            html: "<style>.x { width: calc(100% < 2px); }</style>",
            expected_tag: Tag::Style,
            expected_text: ".x { width: calc(100% < 2px); }",
        },
        RawTextCase {
            name: "style_entity_stays_raw",
            html: "<style>.x::before { content: \"&lt;\"; }</style>",
            expected_tag: Tag::Style,
            expected_text: ".x::before { content: \"&lt;\"; }",
        },
    ];

    for case in cases {
        let doc = parse(case.html).unwrap();
        let node = doc.root().first_child().unwrap();

        assert_eq!(
            node.tag(),
            case.expected_tag,
            "case={}, html={}",
            case.name,
            case.html
        );
        assert_eq!(
            node.text_content(),
            case.expected_text,
            "case={}, html={}",
            case.name,
            case.html
        );
        assert_eq!(
            node.inner_html(),
            case.expected_text,
            "case={}, html={}",
            case.name,
            case.html
        );
    }
}

#[test]
fn empty_attribute_value_is_not_boolean_attribute() {
    let doc = parse("<input value=\"\" disabled data-empty=''>").unwrap();
    let input = doc.root().first_child().unwrap();

    assert_eq!(input.attr("value"), Some(""));
    assert_eq!(input.attr("disabled"), None);
    assert_eq!(input.attr("data-empty"), Some(""));
    assert_eq!(
        input.outer_html(),
        "<input value=\"\" disabled data-empty=\"\">"
    );
}

#[test]
fn long_attribute_value_is_not_truncated() {
    for len in [65_535, 65_536, 70_000] {
        let value = "a".repeat(len);
        let html = format!("<div data-x=\"{value}\"></div>");
        let doc = parse(&html).unwrap();
        let div = doc.root().first_child().unwrap();

        assert_eq!(div.attr("data-x").map(str::len), Some(value.len()));
        assert_eq!(div.attr("data-x"), Some(value.as_str()));
    }
}

#[test]
fn long_attribute_name_is_not_truncated() {
    let name = format!("data-{}", "x".repeat(70_000));
    let html = format!("<div {name}=\"ok\"></div>");
    let doc = parse(&html).unwrap();
    let div = doc.root().first_child().unwrap();

    assert_eq!(div.attr(&name), Some("ok"));
}

// ---------------------------------------------------------------
// Comment and doctype
// ---------------------------------------------------------------

#[test]
fn full_document_with_doctype() {
    let html = "<!DOCTYPE html><html><head><title>T</title></head><body><p>X</p></body></html>";
    let doc = parse(html).unwrap();
    let root = doc.root();

    let mut has_doctype = false;
    for child_id in root.children() {
        if doc.get(child_id).is_doctype() {
            has_doctype = true;
        }
    }
    assert!(has_doctype);
    assert!(root.text_content().contains("X"));
}

#[test]
fn has_class() {
    let doc = parse("<div class=\"foo bar baz\">x</div>").unwrap();
    let div = doc.root().first_child().unwrap();
    assert!(div.has_class("foo"));
    assert!(div.has_class("bar"));
    assert!(div.has_class("baz"));
    assert!(!div.has_class("qux"));
}

#[test]
fn malformed_cdata_prefix_produces_valid_utf8() {
    // Regression for a memory-safety bug: `<![` followed by arbitrary content
    // (not the literal `<![CDATA[`) with a multibyte char straddling the
    // assumed 9-byte offset used to fabricate an invalid-UTF-8 &str through the
    // sink parse path (UB in release, panic in debug). The public parse() path
    // must yield valid UTF-8 output for any input.
    let doc = parse("<div><![ABCDE\u{20ac}]]></div>").unwrap();
    let html = doc.to_html();
    assert!(std::str::from_utf8(html.as_bytes()).is_ok());
    let text = doc.root().text_content();
    assert!(std::str::from_utf8(text.as_bytes()).is_ok());
}

#[test]
fn wellformed_cdata_multibyte_roundtrips() {
    let doc = parse("<div><![CDATA[a\u{20ac}b]]></div>").unwrap();
    assert!(doc.root().text_content().contains("a\u{20ac}b"));
}
