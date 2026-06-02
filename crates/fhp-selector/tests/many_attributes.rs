//! Regression test: a class/id attribute beyond the 256th attribute must still
//! be indexed (attr_count is u16, not u8).

use fhp_selector::Selectable;
use fhp_tree::parse;

#[test]
fn class_after_255_attributes_is_matched() {
    let mut html = String::from("<div");
    for i in 0..300 {
        html.push_str(&format!(" data-a{i}=\"x\""));
    }
    // The class is the 301st attribute — beyond the old u8 (255) cap.
    html.push_str(" class=\"findme\">hit</div>");

    let doc = parse(&html).unwrap();
    assert_eq!(
        doc.select(".findme").unwrap().len(),
        1,
        "a class beyond attribute #255 must still match"
    );
}
