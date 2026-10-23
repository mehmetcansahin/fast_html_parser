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
/// Far below the ratio the per-move rewrite produced, far above noise.
const MAX_RATIO: f64 = 4.0;

fn page(end_tag: &str) -> String {
    let mut html = "<b><span>".repeat(PAIRS);
    html.push_str("<div>");
    html.push_str(&"<i>x</i>".repeat(BLOCK_ITEMS));
    html.push_str(&end_tag.repeat(PAIRS));
    html
}

fn median_parse_ms(html: &str) -> f64 {
    let mut samples: Vec<f64> = (0..5)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(parse(html).unwrap());
            start.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
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
    let control = page("</q>");
    let misnested = page("</b>");

    let document = parse(&misnested).unwrap();
    assert_depths_are_exact(&document, document.root().id());

    let control_ms = median_parse_ms(&control);
    let misnested_ms = median_parse_ms(&misnested);
    let ratio = misnested_ms / control_ms;
    assert!(
        ratio < MAX_RATIO,
        "misnested page took {misnested_ms:.1} ms, {ratio:.1}x the control's {control_ms:.1} ms"
    );
}
