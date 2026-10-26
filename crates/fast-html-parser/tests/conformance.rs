//! Curated conformance checks for the deliberately supported HTML5 recovery
//! subset. This is not an html5lib replacement: each fixture below records a
//! behavior that fast-html-parser intentionally promises.

use fast_html_parser::{Document, HtmlParser, NodeRef};

#[derive(Debug, Clone, PartialEq, Eq)]
enum CanonicalEvent {
    Start {
        name: String,
        attrs: Vec<(String, String)>,
    },
    Text(String),
    End(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalDom(Vec<CanonicalEvent>);

fn canonical_attrs<'a>(
    attrs: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Vec<(String, String)> {
    let mut attrs = attrs
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.unwrap_or("").to_owned()))
        .collect::<Vec<_>>();
    attrs.sort_unstable();
    attrs
}

fn push_text(events: &mut Vec<CanonicalEvent>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(CanonicalEvent::Text(previous)) = events.last_mut() {
        previous.push_str(text);
    } else {
        events.push(CanonicalEvent::Text(text.to_owned()));
    }
}

fn encode_fhp_node(node: NodeRef<'_>, doc: &Document, events: &mut Vec<CanonicalEvent>) {
    if node.is_text() {
        push_text(events, node.text());
        return;
    }
    if node.is_comment() || node.is_doctype() {
        return;
    }

    let name = node
        .tag()
        .as_str()
        .or_else(|| doc.arena().unknown_tag_name(node.id()))
        .expect("non-root FHP element must retain its tag name")
        .to_ascii_lowercase();
    let attrs = canonical_attrs(
        node.attrs()
            .iter()
            .map(|attr| (doc.arena().attr_name(attr), doc.arena().attr_value(attr))),
    );
    events.push(CanonicalEvent::Start {
        name: name.clone(),
        attrs,
    });
    for child in node.children() {
        encode_fhp_node(doc.get(child), doc, events);
    }
    events.push(CanonicalEvent::End(name));
}

fn canonical_fhp(doc: &Document) -> CanonicalDom {
    let mut events = Vec::new();
    for child in doc.root().children() {
        encode_fhp_node(doc.get(child), doc, &mut events);
    }
    CanonicalDom(events)
}

fn canonical_scraper(fragment: &str) -> CanonicalDom {
    let doc = scraper::Html::parse_fragment(fragment);
    let mut events = Vec::new();
    let mut open = Vec::new();

    // scraper does not re-export ego-tree's traversal types. Infer them while
    // walking descendants and close elements when their ancestor chain ends.
    for node in doc.tree.root().descendants().skip(1) {
        let mut ancestors = node
            .ancestors()
            .filter(|ancestor| ancestor.value().is_element())
            .map(|ancestor| ancestor.id())
            .collect::<Vec<_>>();
        ancestors.reverse();

        let shared = open
            .iter()
            .zip(&ancestors)
            .take_while(|((left, _), right)| left == *right)
            .count();
        while open.len() > shared {
            let (_, name) = open.pop().expect("open scraper element");
            events.push(CanonicalEvent::End(name));
        }

        match node.value() {
            scraper::Node::Element(element) => {
                let name = element.name().to_ascii_lowercase();
                let attrs = canonical_attrs(
                    element
                        .attrs()
                        .map(|(attr_name, value)| (attr_name, Some(value))),
                );
                events.push(CanonicalEvent::Start {
                    name: name.clone(),
                    attrs,
                });
                open.push((node.id(), name));
            }
            scraper::Node::Text(text) => push_text(&mut events, text),
            _ => {}
        }
    }
    while let Some((_, name)) = open.pop() {
        events.push(CanonicalEvent::End(name));
    }

    // parse_fragment normally exposes children directly below a Fragment
    // root. Keep this adapter explicit so a scraper/html5ever wrapper change
    // cannot create a false mismatch with FHP's synthetic root contract.
    let events = events
        .into_iter()
        .filter(|event| {
            !matches!(
                event,
                CanonicalEvent::Start { name, .. } | CanonicalEvent::End(name)
                    if matches!(name.as_str(), "html" | "head" | "body")
            )
        })
        .fold(Vec::new(), |mut normalized, event| {
            match event {
                CanonicalEvent::Text(text) => push_text(&mut normalized, &text),
                other => normalized.push(other),
            }
            normalized
        });
    CanonicalDom(events)
}

fn assert_oracle_case(name: &str, html: &str) {
    let borrowed = HtmlParser::parse(html)
        .unwrap_or_else(|error| panic!("{name}: one-shot parse failed: {error}"));
    let owned = HtmlParser::parse_owned(html.to_owned())
        .unwrap_or_else(|error| panic!("{name}: owned parse failed: {error}"));

    let borrowed_dom = canonical_fhp(&borrowed);
    let owned_dom = canonical_fhp(&owned);
    assert_eq!(
        borrowed_dom, owned_dom,
        "{name}: parse and parse_owned produced different DOMs"
    );

    let oracle = canonical_scraper(html);
    assert_eq!(
        borrowed_dom, oracle,
        "{name}: FHP differs from scraper/html5ever"
    );
}

#[test]
fn supported_recovery_subset_matches_html5ever() {
    let mut cases = vec![
        // Tokenizer: WHATWG attribute grammar, bogus comments, EOF handling,
        // legacy raw-text elements, and script-data escapes.
        (
            "apostrophe in unquoted attribute value",
            "<img alt=it's><p>visible</p><p>it's done</p>",
        ),
        ("quote in attribute name", "<div a\"b>x</div><p>y</p>"),
        (
            "processing instruction is a bogus comment",
            "<?xml version=\"1.0\"?><p>hi</p>",
        ),
        (
            "slash ending an unquoted value",
            "<a href=http://example.com/>x</a>",
        ),
        ("unquoted slash value", "<a b=/>x</a>"),
        ("comment closed by --!>", "<!-- c --!><p>after</p>"),
        (
            "conditional comment markers are bogus comments",
            "<![if !IE]><p>a</p><![endif]><p>b</p>",
        ),
        ("end tag open before space", "<p>a</ b>c</p>"),
        ("empty end tag", "<p>a</>c</p>"),
        ("lt before underscore is text", "<_foo>x</_foo>"),
        ("unterminated comment at EOF", "<p>x</p><!-- unterminated"),
        ("unterminated tag at EOF", "<p>x</p><div class=\"a"),
        ("xmp is raw text", "<xmp><b>x</b></xmp>"),
        ("noembed is raw text", "<noembed><b>x</b></noembed>"),
        (
            "escaped script end tag",
            "<script><!--<script>x</script>y--></script><p>z</p>",
        ),
        ("non-void trailing slash", "<div/>after"),
        ("paragraph implicit close", "<p>a<div>b</div>c"),
        ("list item implicit close", "<ul><li>a<li>b</ul>"),
        ("definition item implicit close", "<dl><dt>a<dd>b<dt>c</dl>"),
        ("heading implicit close", "<h1>a<h2>b</h2>"),
        (
            "option implicit close",
            "<select><option>a<option>b</select>",
        ),
        ("implied table body and row", "<table><td>x</td></table>"),
        (
            "table foster parenting",
            "<table>before<tr><td>x</td></tr>after</table>",
        ),
        (
            "table foster parenting after an element sibling",
            "<p>before</p><table>foster<tr><td>x</td></tr></table>",
        ),
        (
            "select invalid-element filtering",
            "<select><div>x<option>a<option>b</select>",
        ),
        (
            "nested select start closes and is ignored",
            "<select><option>x<select><input>",
        ),
        (
            "select input start breaks out and is reprocessed",
            "<select><option>x<input>tail",
        ),
        (
            "select textarea start breaks out and is reprocessed",
            "<select>x<textarea>tail</textarea>",
        ),
        ("formatting adoption repair", "<b><i>x</b>y</i>"),
        ("code formatting adoption repair", "<code><i>x</code>y</i>"),
        ("tt formatting adoption repair", "<tt><i>x</tt>y</i>"),
        ("nested anchor start repair", "<a>one<a>two</a>"),
        (
            "formatting end tag moves the furthest block out",
            "<b><div>x</b>y</div>",
        ),
        (
            "nested anchor keeps the enclosing block open",
            "<a href=1><div>t<a href=2>x</a><p>d</p></div></a>",
        ),
        (
            "adoption clones the three nearest formatting elements",
            "<a><b><i><u><s><div>x</a>y",
        ),
        (
            "adoption drops ordinary elements from the stack",
            "<b><span><em><div><p>x</p>y</b>z",
        ),
        (
            "Noah's Ark limits reopened formatting",
            "<a><b><b><b><b>x</a>y",
        ),
        (
            "named block starts close a paragraph",
            "<p>a<blockquote>b</blockquote>c<p>d<dl><dt>e</dl><p>f<figure>g</figure>",
        ),
        (
            "object bounds the paragraph scope",
            "<p>a<object><p>b</object>c</p>",
        ),
        (
            "body end tag closes a column group",
            "<table><colgroup><col></body><col></table>",
        ),
        ("plaintext", "<plaintext><b>x&amp;"),
        (
            "duplicate attributes are first-wins",
            "<div ID=first id=second class=a CLASS=b>x</div>",
        ),
        (
            "nested list in list item",
            "<ul><li>a<ul><li>b</li></ul></li></ul>",
        ),
        (
            "list item start stops at nested list",
            "<ul><li>a<ol><li>b<li>c</ol><li>d</ul>",
        ),
        (
            "definition item start stops at nested dl",
            "<dl><dt>a<dd><dl><dt>b</dl></dl>",
        ),
        (
            "list item start stops at table cell",
            "<ul><li>a<table><tr><td><li>b</td></tr></table></li></ul>",
        ),
        (
            "nested table in cell",
            "<table><tr><td><table><tr><td>inner</td></tr></table></td><td>x</td></tr></table>",
        ),
        (
            "nested table inside div in cell",
            "<table><tr><td><div><table><tr><td>i</td></tr></table></div></td></tr></table>",
        ),
        (
            "table caption",
            "<table><caption>cap</caption><tr><td>x</td></tr></table>",
        ),
        (
            "table colgroup",
            "<table><colgroup><col><col></colgroup><tr><td>x</td></tr></table>",
        ),
        (
            "table col implies colgroup",
            "<table><col><tr><td>x</td></tr></table>",
        ),
        (
            "script inside table",
            "<table><script>var a=1;</script><tr><td>x</td></tr></table>",
        ),
        (
            "style inside table",
            "<table><style>td{}</style><tr><td>x</td></tr></table>",
        ),
        (
            "form inside table",
            "<table><form><tr><td>x</td></tr></form></table>",
        ),
        (
            "hidden input inside table",
            "<table><input type=hidden name=a><tr><td>x</td></tr></table>",
        ),
        (
            "text input fostered out of table",
            "<table><input type=text><tr><td>x</td></tr></table>",
        ),
        (
            "fostered element keeps its content",
            "<table><div>hello<span>w</span></div><tr><td>x</td></tr></table>",
        ),
        (
            "fostered paragraph",
            "<table><p>para</p><tr><td>x</td></tr></table>",
        ),
        (
            "table row start closes fostered element",
            "<table><div>x<tr><td>y</td></tr></table>",
        ),
        (
            "div end tag ignored inside cell",
            "<div><table><tr><td>cell</div>after</td></tr></table>",
        ),
        (
            "anchor inside cell is not repaired across the cell",
            "<a href=o><table><tr><td><a href=i>x</a></td><td>y</td></tr></table></a>",
        ),
        (
            "cell end tag closes select in table",
            "<table><tr><td><select><option>a</td><td>b</td></tr></table>",
        ),
        (
            "table start closes select in table",
            "<table><tr><td><select><option>a<table><tr><td>b</td></tr></table></td></tr></table>",
        ),
        (
            "heading start does not pop a non-heading",
            "<h1>a<span>b<h2>c</h2></span></h1>",
        ),
        (
            "option start only closes a current option",
            "<option>a<div>b<option>c",
        ),
        ("list item start closes paragraph", "<p>a<li>b"),
        (
            "paragraph end tag without open paragraph",
            "<div>a</p>b</div>",
        ),
        ("br end tag is a br start tag", "<p>a</br>b</p>"),
        ("orphan cell is ignored", "<td>x</td>"),
        (
            "cell after table is ignored",
            "<table><tr><td>a</td></tr></table><td>b</td>",
        ),
        (
            "script inside select",
            "<select><option>a</option><script>s</script></select>",
        ),
        (
            "leading newline after pre and textarea",
            "<pre>\ncode</pre><textarea>\ntext</textarea>",
        ),
        (
            "leading CRLF after pre and textarea",
            "<pre>\r\ncode</pre><textarea>\r\ntext</textarea>",
        ),
        (
            "end tag name with equals and quote",
            "<div>a</div=\"x>b<p>visible</p>",
        ),
        (
            "end tag name with quote before content",
            "<p>a</p=\"><b>bold</b> rest",
        ),
        (
            "end tag attribute name starting with equals",
            "<div>a</div ='x>b<p>visible</p>",
        ),
        ("dt end tag does not close dd", "<dd>a</dt>b"),
        ("dd end tag does not close dt", "<dt>a</dd>b"),
        (
            "select in table ignores caption end tag outside scope",
            "<table><tr><td><select><option>a</caption>b</td></tr></table>",
        ),
        (
            "select in table ignores th end tag in td",
            "<table><tr><td><select><option>a</th>b</td></tr></table>",
        ),
        (
            "reopened formatting element is fostered from table",
            "<table><b>x<i>y</b>z<tr><td>c</td></tr></table>",
        ),
        (
            "reopened formatting element is fostered from tbody",
            "<table><tbody><b>x<i>y</b>z<tr><td>c</td></tr></table>",
        ),
        (
            "reopened formatting element is fostered from tr",
            "<table><tr><b>x<i>y</b>z<td>c</td></tr></table>",
        ),
        (
            "end tag in column group closes it",
            "<table><colgroup></p><col></colgroup></table>",
        ),
    ];

    if cfg!(feature = "entity-decode") {
        cases.push((
            "full, multi-codepoint, and legacy entities",
            "<p title='&copy=test' data-ok='&copy test'>&CounterClockwiseContourIntegral;|&NotEqualTilde;|&copy test</p>",
        ));
    }

    for (name, html) in cases {
        assert_oracle_case(name, html);
    }
}

fn empty_element(name: &str) -> [CanonicalEvent; 2] {
    [
        CanonicalEvent::Start {
            name: name.to_owned(),
            attrs: Vec::new(),
        },
        CanonicalEvent::End(name.to_owned()),
    ]
}

/// Canonical DOM of `depth` nested `div` elements around `inner`.
fn nested_divs(depth: usize, inner: Vec<CanonicalEvent>) -> CanonicalDom {
    let [start, end] = empty_element("div");
    let mut events = vec![start; depth];
    events.extend(inner);
    events.extend(std::iter::repeat_n(end, depth));
    CanonicalDom(events)
}

fn assert_parses_to(html: &str, expected: &CanonicalDom) {
    for result in [
        HtmlParser::parse(html),
        HtmlParser::parse_owned(html.to_owned()),
    ] {
        let document = result.unwrap_or_else(|error| panic!("deep input failed: {error}"));
        assert_eq!(&canonical_fhp(&document), expected);
    }
}

#[test]
fn elements_past_512_open_elements_are_inserted_empty() {
    let mut at_limit = "<div>".repeat(512);
    at_limit.push('x');
    at_limit.push_str(&"</div>".repeat(512));
    assert_oracle_case("512 nested elements", &at_limit);

    // The 513th element stays empty, and the text after it goes to the 512th.
    let mut over_limit = "<div>".repeat(513);
    over_limit.push('x');
    let mut inner = empty_element("div").to_vec();
    inner.push(CanonicalEvent::Text("x".to_owned()));
    assert_parses_to(&over_limit, &nested_divs(512, inner));

    let mut void_leaf = "<div>".repeat(512);
    void_leaf.push_str("<br>");
    assert_parses_to(&void_leaf, &nested_divs(512, empty_element("br").to_vec()));
}
