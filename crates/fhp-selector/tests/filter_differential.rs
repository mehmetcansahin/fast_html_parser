//! Selection pre-filters candidates (ancestor tags/classes/ids, cached id
//! hashes) before running the matcher. Check on real pages that it never
//! drops a match: every selection must equal a brute-force scan that runs
//! the matcher on every element in document order.

use std::collections::HashSet;

use fhp_selector::Selectable;
use fhp_selector::matcher::match_selector_list;
use fhp_selector::parser::parse_selector;
use fhp_tree::node::{NodeFlags, NodeId};
use fhp_tree::{Document, parse};

const FIXTURES: &[(&str, &str)] = &[
    (
        "hackernews",
        include_str!("../../../testdata/hackernews.html"),
    ),
    (
        "medium_100kb",
        include_str!("../../../testdata/medium_100kb.html"),
    ),
    ("github", include_str!("../../../testdata/github.html")),
    (
        "wikipedia",
        include_str!("../../../testdata/wikipedia.html"),
    ),
];

/// Descendant and child chains over tags, classes, and ids that occur in the
/// fixtures, plus selectors whose required ancestors never occur.
const SELECTORS: &[&str] = &[
    "div p",
    "div > p",
    "div a",
    "ul li a",
    "table td",
    "table table td",
    "tr td a",
    "div div div",
    "li > a",
    "div + p",
    "div ~ p",
    "p a",
    "nav ul li",
    "header a",
    "form input",
    "body > div",
    "* p",
    "svg path",
    "div:not(.x) span",
    "a[href] span",
    ".mw-body p",
    "#content a",
    "#mw-content-text p",
    "span.mw-editsection a",
    "main.mw-body",
    ".athing .titleline > a",
    "td .titleline a",
    ".d-flex a",
    "#nope p",
    ".nope a",
    "nav .nope li",
    "p, div p",
    "li a, td a",
    "#main",
    ".card",
];

fn is_element(document: &Document, node: NodeId) -> bool {
    let n = document.arena().get(node);
    n.depth > 0
        && !n
            .flags
            .has(NodeFlags::IS_TEXT | NodeFlags::IS_COMMENT | NodeFlags::IS_DOCTYPE)
}

/// Elements among `candidates`, in document order, that the matcher accepts.
fn brute_force(document: &Document, css: &str, candidates: &HashSet<NodeId>) -> Vec<NodeId> {
    let list = parse_selector(css).unwrap();
    document
        .root()
        .descendants()
        .filter(|&node| candidates.contains(&node) && is_element(document, node))
        .filter(|&node| match_selector_list(document.arena(), node, &list))
        .collect()
}

#[test]
fn selection_matches_a_brute_force_scan() {
    for (name, html) in FIXTURES {
        let document = parse(html).unwrap();
        let everything: HashSet<NodeId> = document.root().descendants().collect();
        for css in SELECTORS {
            let expected = brute_force(&document, css, &everything);
            let actual: Vec<NodeId> = document
                .select(css)
                .unwrap()
                .iter()
                .map(|node| node.id())
                .collect();
            assert_eq!(actual, expected, "{name}: {css}");
            assert_eq!(
                document.select_first(css).unwrap().map(|node| node.id()),
                expected.first().copied(),
                "{name}: first {css}"
            );
        }
    }
}

#[test]
fn chained_selection_matches_a_brute_force_scan_of_the_roots_subtrees() {
    for (name, html) in FIXTURES {
        let document = parse(html).unwrap();
        for outer in ["div", "section", "li", "table"] {
            let roots = document.select(outer).unwrap();
            let subtrees: HashSet<NodeId> =
                roots.iter().flat_map(|root| root.descendants()).collect();
            for css in SELECTORS {
                let expected = brute_force(&document, css, &subtrees);
                let actual: Vec<NodeId> = roots
                    .select(css)
                    .unwrap()
                    .iter()
                    .map(|node| node.id())
                    .collect();
                assert_eq!(actual, expected, "{name}: {outer} -> {css}");
            }
        }
    }
}
