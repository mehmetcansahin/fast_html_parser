use fhp_selector::xpath::ast::XPathResult;
use fhp_selector::{DocumentIndex, Selectable};
use fhp_tree::parse;

#[test]
fn selector_lists_return_matches_in_document_order() {
    let doc = parse("<div id='first'></div><span id='second'></span>").unwrap();

    let selection = doc.select("span, div").unwrap();
    let ids: Vec<_> = selection
        .iter()
        .map(|node| node.attr("id").unwrap())
        .collect();

    assert_eq!(ids, ["first", "second"]);
    assert_eq!(selection.first().unwrap().attr("id"), Some("first"));
    assert_eq!(
        doc.select_first("span, div").unwrap().unwrap().attr("id"),
        Some("first")
    );
}

#[test]
fn xpath_descendant_position_is_relative_to_each_parent() {
    let doc = parse(
        "<ul><li>a1</li><li>a2</li></ul>\
         <ol><li>b1</li><li>b2</li></ol>",
    )
    .unwrap();

    let XPathResult::Nodes(nodes) = doc.xpath("//li[2]").unwrap() else {
        panic!("expected node result");
    };
    let texts: Vec<_> = nodes
        .into_iter()
        .map(|node| doc.get(node).text_content())
        .collect();

    assert_eq!(texts, ["a2", "b2"]);
}

#[test]
fn selection_xpath_deduplicates_overlapping_text_nodes() {
    let doc = parse("<div><p>same</p></div>").unwrap();
    let selection = doc.select("div, p").unwrap();

    assert_eq!(
        selection.xpath("//p/text()").unwrap(),
        XPathResult::Strings(vec!["same".to_string()])
    );
}

#[test]
fn selection_xpath_preserves_empty_string_result_variant() {
    let doc = parse("<p>text</p>").unwrap();
    let selection = doc.select("p").unwrap();

    assert_eq!(
        selection.xpath("//span/text()").unwrap(),
        XPathResult::Strings(Vec::new())
    );
}

#[test]
fn nth_child_above_u16_max_uses_the_actual_sibling_index() {
    let mut html = String::with_capacity(65_536 * 7 + 16);
    html.push_str("<div>");
    for _ in 0..65_536 {
        html.push_str("<i></i>");
    }
    html.push_str("</div>");

    let doc = parse(&html).unwrap();

    assert_eq!(doc.select("i:nth-child(65536)").unwrap().len(), 1);
}

#[test]
fn xpath_text_selects_only_direct_child_text_nodes() {
    let doc = parse("<p>outer <b>nested</b> tail</p>").unwrap();

    assert_eq!(
        doc.xpath("//p/text()").unwrap(),
        XPathResult::Strings(vec!["outer ".to_string(), " tail".to_string()])
    );
}

#[test]
fn empty_css_substring_operands_never_match() {
    let doc = parse("<div data-x=''></div><div data-x='abc'></div><div data-x></div>").unwrap();

    assert!(doc.select("[data-x^='']").unwrap().is_empty());
    assert!(doc.select("[data-x$='']").unwrap().is_empty());
    assert!(doc.select("[data-x*='']").unwrap().is_empty());
}

#[test]
fn valueless_attributes_have_an_empty_string_value_for_queries() {
    let doc = parse("<input disabled>").unwrap();

    assert_eq!(doc.select("[disabled='']").unwrap().len(), 1);
    assert_eq!(
        doc.xpath("//*[@disabled='']").unwrap(),
        XPathResult::Nodes(doc.select("input").unwrap().node_ids().to_vec())
    );
}

#[test]
fn overflowing_nth_child_integers_are_rejected() {
    let doc = parse("<i></i>").unwrap();

    assert!(doc.select(":nth-child(999999999999999999999)").is_err());
    assert!(doc.select(":nth-child(999999999999n)").is_err());
    assert!(doc.select(":nth-child(n+999999999999)").is_err());
}

#[test]
fn minimum_i32_nth_child_terms_are_accepted() {
    let doc = parse("<i></i>").unwrap();

    assert!(doc.select(":nth-child(-2147483648)").is_ok());
    assert!(doc.select(":nth-child(n-2147483648)").is_ok());
    assert!(doc.select(":nth-child(-2147483648n)").is_ok());
}

#[test]
fn multiple_type_selectors_in_one_compound_are_rejected() {
    let doc = parse("<div></div>").unwrap();

    assert!(doc.select("div*").is_err());
    assert!(doc.select("*div").is_err());
    assert!(doc.select(".class*").is_err());
}

#[test]
fn duplicate_id_attributes_do_not_cause_hash_false_negatives() {
    let doc = parse("<div id='target' id='other'>hit</div>").unwrap();

    let selection = doc.select("#target").unwrap();
    assert_eq!(selection.len(), 1);
    assert_eq!(selection.text(), "hit");
}

#[test]
fn document_index_keeps_the_first_element_for_duplicate_ids() {
    let doc = parse("<div id='dup'>first</div><span id='dup'>second</span>").unwrap();
    let index = DocumentIndex::build(&doc);

    assert_eq!(
        index.find_by_id(&doc, "dup").unwrap().text_content(),
        "first"
    );
}
