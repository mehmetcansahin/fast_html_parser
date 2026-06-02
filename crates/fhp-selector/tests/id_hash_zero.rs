//! Regression test for an id whose FNV-1a hash is exactly 0.
//!
//! `id_hash == 0` doubles as the "no id attribute" sentinel on a node, so an id
//! that genuinely hashes to 0 must not be rejected by the hash fast-path.

use fhp_core::hash::selector_hash;
use fhp_selector::Selectable;
use fhp_tree::parse;

// "BR42qf" is a valid CSS ident whose FNV-1a selector_hash is 0 (found by
// brute force). Guarded below so a hash-function change is caught loudly.
const ZERO_HASH_ID: &str = "BR42qf";

#[test]
fn id_with_zero_hash_is_matched() {
    assert_eq!(selector_hash(ZERO_HASH_ID.as_bytes()), 0, "test premise");

    let html = format!("<div id=\"{ZERO_HASH_ID}\">hit</div>");
    let doc = parse(&html).unwrap();
    let sel = doc.select(&format!("#{ZERO_HASH_ID}")).unwrap();
    assert_eq!(
        sel.len(),
        1,
        "id selector must match an id that hashes to 0"
    );
    assert_eq!(sel.text(), "hit");
}

#[test]
fn zero_hash_id_selector_no_false_match() {
    assert_eq!(selector_hash(ZERO_HASH_ID.as_bytes()), 0, "test premise");

    // A node without that id (and a node with no id) must NOT match.
    let doc = parse("<div id=\"other\">a</div><span>b</span>").unwrap();
    assert_eq!(doc.select(&format!("#{ZERO_HASH_ID}")).unwrap().len(), 0);
}
