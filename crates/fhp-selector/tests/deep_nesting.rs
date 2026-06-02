//! Regression test for open/close desync at the nesting depth limit.
//!
//! When an open tag is dropped because the depth limit (512) is reached, its
//! matching close tag must be swallowed too — otherwise the close pops a real,
//! still-open element of the same tag and truncates the stack at the wrong
//! place, corrupting the rest of the tree.

use fhp_selector::Selectable;
use fhp_tree::parse;

#[test]
fn dropped_deep_open_does_not_let_close_pop_real_element() {
    // Far past the 512 limit with a single repeated tag. The first ~511 <x>
    // fill the stack; the rest are dropped. A single </x> must NOT pop a real
    // <x> off the (still saturated) stack. While the stack stays saturated, a
    // following <probe> is itself beyond the depth limit and must be dropped.
    //
    // With the desync bug, the </x> wrongly pops a real <x>, the depth drops
    // below the limit, and <probe> gets created.
    let mut html = String::new();
    for _ in 0..1000 {
        html.push_str("<x>");
    }
    html.push_str("</x>");
    html.push_str("<probe></probe>");

    let doc = parse(&html).unwrap();
    assert_eq!(
        doc.select("probe").unwrap().len(),
        0,
        "a single close of a depth-dropped element must not unwind the real stack"
    );
}
