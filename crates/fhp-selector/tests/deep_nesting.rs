//! Regression test for documents nested past the depth limit.

use fhp_selector::Selectable;
use fhp_tree::parse;

#[test]
fn elements_past_the_depth_limit_are_inserted_empty_and_selectable() {
    let html = "<x>".repeat(1000);
    let doc = parse(&html).expect("deep input must parse instead of failing");

    // The first 512 elements nest; the rest are empty children of the 512th.
    let deepest = (0..512).fold(doc.root(), |node, _| node.first_child().unwrap());
    assert_eq!(deepest.children().count(), 1000 - 512);
    assert!(
        deepest
            .children()
            .all(|child| !doc.get(child).has_children())
    );
    assert_eq!(doc.select("x").unwrap().len(), 1000);
    assert_eq!(doc.select("x > x > x").unwrap().len(), 1000 - 2);
}
