//! Curated conformance checks for the deliberately supported HTML5 recovery
//! subset. This is not an html5lib replacement: each fixture below records a
//! behavior that fast-html-parser intentionally promises.

use fast_html_parser::core_types::error::ParseError;
use fast_html_parser::{Document, HtmlError, HtmlParser, NodeRef};

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
        ("plaintext", "<plaintext><b>x&amp;"),
        (
            "duplicate attributes are first-wins",
            "<div ID=first id=second class=a CLASS=b>x</div>",
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

fn assert_nesting_error(html: String) {
    for result in [
        HtmlParser::parse(&html),
        HtmlParser::parse_owned(html.clone()),
    ] {
        match result {
            Err(HtmlError::Parse(ParseError::NestingTooDeep { depth, limit })) => {
                assert_eq!(depth, 513);
                assert_eq!(limit, 512);
            }
            Err(other) => panic!("513-deep input returned the wrong error: {other}"),
            Ok(_) => panic!("513-deep input returned a partial document"),
        }
    }
}

#[test]
fn nesting_limit_accepts_512_and_rejects_513_without_a_partial_document() {
    let mut at_limit = "<div>".repeat(512);
    at_limit.push('x');
    at_limit.push_str(&"</div>".repeat(512));
    assert_oracle_case("512 nested elements", &at_limit);

    assert_nesting_error("<div>".repeat(513));

    let mut void_leaf_over_limit = "<div>".repeat(512);
    void_leaf_over_limit.push_str("<br>");
    assert_nesting_error(void_leaf_over_limit);
}
