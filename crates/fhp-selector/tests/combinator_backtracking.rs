//! Regression tests for right-to-left combinator matching with backtracking.
//!
//! The greedy walk committed to the first matching ancestor/sibling and never
//! reconsidered, so `A > B C` style selectors dropped valid matches when the
//! nearest B was not a child of A but an outer B was.

use fhp_selector::Selectable;
use fhp_tree::parse;

#[test]
fn descendant_then_child_backtracks() {
    // span descends from div_outer, and div_outer is a child of section.
    // Greedy matching picked div_inner (nearest) whose parent is not section.
    let doc = parse("<section><div><div><span>x</span></div></div></section>").unwrap();
    assert_eq!(doc.select("section > div span").unwrap().len(), 1);
}

#[test]
fn descendant_then_child_no_false_match() {
    // No <aside> ancestor: must NOT match.
    let doc = parse("<section><div><div><span>x</span></div></div></section>").unwrap();
    assert_eq!(doc.select("aside > div span").unwrap().len(), 0);
}

#[test]
fn double_descendant_then_child() {
    // `ul > li a` where the matching li is the outer one.
    let doc = parse("<ul><li><ul><li><a>x</a></li></ul></li></ul>").unwrap();
    // Outer li is a child of the outer ul; <a> descends from it. Inner li is a
    // child of the inner ul. Both <a> ancestries should yield exactly one <a>.
    assert_eq!(doc.select("ul > li a").unwrap().len(), 1);
}

#[test]
fn general_sibling_then_descendant_backtracks() {
    // `h2 ~ section p`: the <p> is under a <section> that follows an <h2>.
    let doc =
        parse("<div><h2>t</h2><section><article><p>hit</p></article></section></div>").unwrap();
    assert_eq!(doc.select("h2 ~ section p").unwrap().len(), 1);
}

#[test]
fn pure_descendant_chain_still_works() {
    let doc = parse("<a><b><c>x</c></b></a>").unwrap();
    assert_eq!(doc.select("a b c").unwrap().len(), 1);
}

#[test]
fn child_chain_still_works() {
    let doc = parse("<a><b><c>x</c></b></a>").unwrap();
    assert_eq!(doc.select("a > b > c").unwrap().len(), 1);
    assert_eq!(doc.select("a > c").unwrap().len(), 0);
}
