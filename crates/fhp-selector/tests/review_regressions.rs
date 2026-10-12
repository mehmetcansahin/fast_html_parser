use fhp_core::tag::Tag;
#[cfg(feature = "xpath")]
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

#[cfg(feature = "xpath")]
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

#[cfg(feature = "xpath")]
#[test]
fn selection_xpath_deduplicates_overlapping_text_nodes() {
    let doc = parse("<div><p>same</p></div>").unwrap();
    let selection = doc.select("div, p").unwrap();

    assert_eq!(
        selection.xpath("//p/text()").unwrap(),
        XPathResult::Strings(vec!["same".to_string()])
    );
}

#[cfg(feature = "xpath")]
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

#[cfg(feature = "xpath")]
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

#[cfg(feature = "xpath")]
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

#[test]
fn document_index_deduplicates_repeated_class_tokens_per_element() {
    let doc = parse(
        "<div id='first' class='target other target target'></div>\
         <span id='second' class='target'></span>",
    )
    .unwrap();
    let index = DocumentIndex::build(&doc);

    let indexed_ids: Vec<_> = index
        .find_by_class(&doc, "target")
        .into_iter()
        .map(|node| node.attr("id").unwrap())
        .collect();
    assert_eq!(indexed_ids, ["first", "second"]);
    assert_eq!(indexed_ids.len(), doc.find_by_class("target").len());
}

#[test]
fn convenience_queries_and_index_follow_foster_parented_dom_order() {
    let doc = parse(
        "<table>\
           <tr><td><div id='dup' class='ordered' data-order='inside'>second</div></td></tr>\
           <div id='dup' class='ordered' data-order='fostered'>first</div>\
         </table>",
    )
    .unwrap();

    let expected = ["fostered", "inside"];
    let selected: Vec<_> = doc
        .select("[data-order]")
        .unwrap()
        .iter()
        .map(|node| node.attr("data-order").unwrap())
        .collect();
    assert_eq!(selected, expected);

    assert_eq!(
        doc.find_by_id("dup").unwrap().attr("data-order"),
        Some("fostered")
    );
    assert_eq!(
        doc.find_by_class("ordered")
            .iter()
            .map(|node| node.attr("data-order").unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        doc.find_by_attr("class", "ordered")
            .iter()
            .map(|node| node.attr("data-order").unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        doc.find_by_tag(Tag::Div)
            .iter()
            .map(|node| node.attr("data-order").unwrap())
            .collect::<Vec<_>>(),
        expected
    );

    let index = DocumentIndex::build(&doc);
    assert_eq!(
        index.find_by_id(&doc, "dup").unwrap().attr("data-order"),
        Some("fostered")
    );
    assert_eq!(
        index
            .find_by_class(&doc, "ordered")
            .into_iter()
            .map(|node| node.attr("data-order").unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        index
            .find_by_tag(&doc, Tag::Div)
            .into_iter()
            .map(|node| node.attr("data-order").unwrap())
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn class_queries_use_html_space_characters_consistently() {
    let em_space_token = "alpha\u{2003}beta";
    let vertical_tab_token = "gamma\u{000b}delta";
    let html = format!(
        "<div id='em' class='{em_space_token}'></div>\
         <div id='vt' class='{vertical_tab_token}'></div>\
         <div id='spaces' class='one\ttwo\nthree\u{000c}four\rfive'></div>"
    );
    let doc = parse(&html).unwrap();
    let index = DocumentIndex::build(&doc);

    assert!(doc.select(".alpha").unwrap().is_empty());
    assert!(doc.select("[class~='alpha']").unwrap().is_empty());
    assert_eq!(doc.select("[class~='alpha\u{2003}beta']").unwrap().len(), 1);
    assert_eq!(doc.find_by_class(em_space_token).len(), 1);
    assert_eq!(index.find_by_class(&doc, em_space_token).len(), 1);
    let em = doc.find_by_id("em").unwrap();
    assert!(em.has_class(em_space_token));
    assert!(!em.has_class("alpha"));

    assert!(doc.select("[class~='gamma']").unwrap().is_empty());
    assert_eq!(
        doc.select("[class~='gamma\u{000b}delta']").unwrap().len(),
        1
    );
    assert_eq!(doc.find_by_class(vertical_tab_token).len(), 1);
    assert_eq!(index.find_by_class(&doc, vertical_tab_token).len(), 1);
    let vt = doc.find_by_id("vt").unwrap();
    assert!(vt.has_class(vertical_tab_token));
    assert!(!vt.has_class("gamma"));

    for class in ["one", "two", "three", "four", "five"] {
        assert_eq!(doc.select(&format!(".{class}")).unwrap().len(), 1);
        assert_eq!(doc.find_by_class(class).len(), 1);
        assert_eq!(index.find_by_class(&doc, class).len(), 1);
    }
}

#[cfg(feature = "xpath")]
#[test]
fn xpath_positions_scale_across_wide_sibling_sets() {
    const WIDTH: usize = 8_192;
    let mut html = String::with_capacity(WIDTH * 10 + 32);
    html.push_str("<html><body><ul>");
    for index in 1..=WIDTH {
        html.push_str("<li>");
        html.push_str(&index.to_string());
        html.push_str("</li>");
    }
    html.push_str("</ul></body></html>");

    let doc = parse(&html).unwrap();
    for expression in ["//li[8192]", "/html/body/ul/li[8192]"] {
        let XPathResult::Nodes(nodes) = doc.xpath(expression).unwrap() else {
            panic!("expected node result");
        };
        assert_eq!(nodes.len(), 1);
        assert_eq!(doc.get(nodes[0]).text_content(), WIDTH.to_string());
    }
}

#[cfg(feature = "xpath")]
#[test]
fn selection_xpath_positions_scale_across_wide_sibling_roots() {
    const WIDTH: usize = 8_192;
    let mut html = String::with_capacity(WIDTH * 10 + 32);
    html.push_str("<ul>");
    for index in 1..=WIDTH {
        html.push_str("<li>");
        html.push_str(&index.to_string());
        html.push_str("</li>");
    }
    html.push_str("</ul>");

    let doc = parse(&html).unwrap();
    let roots = doc.select("li").unwrap();
    let XPathResult::Nodes(nodes) = roots.xpath("//li[8192]").unwrap() else {
        panic!("expected node result");
    };

    assert_eq!(nodes.len(), 1);
    assert_eq!(doc.get(nodes[0]).text_content(), WIDTH.to_string());
}

#[cfg(feature = "xpath")]
#[test]
fn selection_xpath_positions_preserve_overlapping_root_semantics() {
    let doc = parse(
        "<div id='outer'>\
           <div id='first'><div id='nested'></div></div>\
           <div id='second'></div>\
         </div>",
    )
    .unwrap();
    let roots = doc.select("div").unwrap();

    let XPathResult::Nodes(nodes) = roots.xpath("//div[1]").unwrap() else {
        panic!("expected node result");
    };
    let ids: Vec<_> = nodes
        .into_iter()
        .map(|node| doc.get(node).attr("id").unwrap())
        .collect();

    // Descendant-or-self includes `outer`; nested selection roots do not
    // duplicate `first` or `nested`, and results remain in document order.
    assert_eq!(ids, ["outer", "first", "nested"]);
}
