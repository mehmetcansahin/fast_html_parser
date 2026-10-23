//! Misnested formatting end tags make the adoption agency move a block, and
//! everything inside it, up the tree. A page that repeats such moves may cost
//! only a small constant factor over the same page without them; rewriting
//! the moved subtree's depths on every move made it ~15x slower.

use std::time::Instant;

use fhp_tree::node::NodeId;
use fhp_tree::{Document, parse};

/// `<b><span>` pairs: each `</b>` drops one `span` from between the `b` and
/// the block, so the block's content moves up one level per end tag.
const PAIRS: usize = 250;
const BLOCK_ITEMS: usize = 20_000;
/// Deferred depths cost ~1.0-1.2x here (debug builds). Rejected designs
/// cost more: rewriting depths per move ~12x for `parse` and ~4.2x under an
/// early-stop predicate, a plain stale flag ~3.9x, and climbing to the root
/// for every older node's depth ~2.4x when the predicate reads the parent's.
const MAX_RATIO: f64 = 2.0;

/// The block's content first, then one end tag per pair: `</b>` moves the
/// content every time, `</q>` (the control) never.
fn repeated_moves(end_tag: &str) -> String {
    let mut html = "<b><span>".repeat(PAIRS);
    html.push_str("<div>");
    html.push_str(&"<i>x</i>".repeat(BLOCK_ITEMS));
    html.push_str(&end_tag.repeat(PAIRS));
    html
}

/// One end tag over a block with ten nested open `section`s, then the
/// content, ~500 levels deep. `</b>` leaves the older nodes with stale
/// depths while the content is created.
fn content_after_a_move(end_tag: &str) -> String {
    let mut html = "<b><span>".repeat(PAIRS);
    html.push_str("<div><i>y</i>");
    html.push_str(&"<section>".repeat(10));
    html.push_str(end_tag);
    html.push_str(&"<i>x</i>".repeat(BLOCK_ITEMS));
    html
}

fn median_ms(mut run: impl FnMut()) -> f64 {
    let mut samples: Vec<f64> = (0..5)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn assert_constant_factor(what: &str, page: fn(&str) -> String, mut run: impl FnMut(&str)) {
    let control = page("</q>");
    let misnested = page("</b>");
    let control_ms = median_ms(|| run(&control));
    let misnested_ms = median_ms(|| run(&misnested));
    let ratio = misnested_ms / control_ms;
    assert!(
        ratio < MAX_RATIO,
        "{what}: misnested page took {misnested_ms:.1} ms, \
         {ratio:.1}x the control's {control_ms:.1} ms"
    );
}

fn assert_depths_are_exact(document: &Document, node: NodeId) {
    let arena = document.arena();
    let mut child = arena.get(node).first_child;
    while !child.is_null() {
        assert_eq!(arena.get(child).depth, arena.get(node).depth + 1);
        assert_depths_are_exact(document, child);
        child = arena.get(child).next_sibling;
    }
}

#[test]
fn repeated_adoption_moves_cost_a_constant_factor() {
    for page in [repeated_moves, content_after_a_move] {
        let document = parse(&page("</b>")).unwrap();
        assert_depths_are_exact(&document, document.root().id());

        assert_constant_factor("parse", page, |html| {
            std::hint::black_box(parse(html).unwrap());
        });
    }
}

#[test]
#[cfg(feature = "encoding")]
fn early_stop_parsing_pays_a_constant_factor_too() {
    use fhp_tree::NodeRef;
    use fhp_tree::streaming::{EarlyStopOutcome, EarlyStopParser};

    type Predicate = fn(NodeRef<'_>) -> bool;

    // Each predicate observes every node, reads a depth, and never matches,
    // so the whole page is parsed under observation. A new node's parent is
    // an older, open element.
    let predicates: [(&str, Predicate); 2] = [
        ("own depth", |node| node.depth() == u16::MAX),
        ("parent depth", |node| {
            node.parent()
                .is_some_and(|parent| parent.depth() == u16::MAX)
        }),
    ];

    for (name, predicate) in predicates {
        let early_stop = |html: &str| {
            let mut parser = EarlyStopParser::stop_on_create(predicate);
            parser.feed(html.as_bytes()).unwrap();
            let EarlyStopOutcome::Done(document) = parser.finish().unwrap() else {
                panic!("the predicate never matches");
            };
            document
        };

        for page in [repeated_moves, content_after_a_move] {
            let document = early_stop(&page("</b>"));
            assert_depths_are_exact(&document, document.root().id());

            assert_constant_factor(&format!("EarlyStopParser, {name}"), page, |html| {
                std::hint::black_box(early_stop(html));
            });
        }
    }
}
