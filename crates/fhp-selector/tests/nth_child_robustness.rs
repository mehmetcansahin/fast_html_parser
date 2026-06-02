//! Robustness tests for nth-child indexing and arithmetic.

use fhp_selector::Selectable;
use fhp_tree::parse;

#[test]
fn many_element_children_do_not_overflow_index() {
    // More than u16::MAX element children under one parent. The per-parent
    // element-index counter must not overflow (debug panic / release wrap).
    let mut html = String::with_capacity(70_000 * 7 + 16);
    html.push_str("<div>");
    for _ in 0..70_000 {
        html.push_str("<i></i>");
    }
    html.push_str("</div>");

    let doc = parse(&html).unwrap();
    assert_eq!(doc.select("i").unwrap().len(), 70_000);
}

#[test]
fn pathological_nth_b_does_not_overflow() {
    // `n-2147483647`: a=1, b=-2147483647. The `index - b` subtraction must not
    // overflow i32 (debug panic / release wrong result).
    let doc = parse("<ul><li>a</li><li>b</li><li>c</li></ul>").unwrap();
    // an+b with hugely negative b matches every positive index, so all 3 li.
    assert_eq!(doc.select("li:nth-child(n-2147483647)").unwrap().len(), 3);
}
