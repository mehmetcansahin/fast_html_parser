use fhp_core::tag::Tag;
use fhp_selector::matcher::select_all_list;
use fhp_selector::parser::parse_selector;
#[cfg(feature = "xpath")]
use fhp_selector::xpath::ast::XPathResult;
use fhp_selector::{DocumentIndex, Selectable};
use fhp_tree::arena::Arena;
use fhp_tree::parse;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Upper bound for the linear-time regressions below (debug builds).
const TIME_LIMIT: Duration = Duration::from_secs(2);

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
fn chained_descendant_selectors_see_ancestors_above_the_roots() {
    // Selection pre-filters descendant selectors on the ancestors' tags,
    // classes, and ids; for chained roots those include the roots' own
    // ancestors, outside the searched subtrees.
    let doc = parse(
        "<div id='outer' class='wrap'><section><p>in</p></section></div>\
         <section><p>out</p></section>",
    )
    .unwrap();
    let sections = doc.select("section").unwrap();
    assert_eq!(sections.len(), 2);

    for css in [
        "div p",
        "#outer p",
        ".wrap p",
        "div > section p",
        "div section > p",
    ] {
        let selection = sections.select(css).unwrap();
        assert_eq!(selection.text(), "in", "{css}");
    }
    for css in ["span p", "#other p", ".other p", "div > p"] {
        assert!(sections.select(css).unwrap().is_empty(), "{css}");
    }
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

#[test]
fn sub_selections_match_ancestors_above_each_root() {
    let doc = parse("<div class=\"outer\"><section><p>x</p></section></div>").unwrap();
    assert_eq!(
        doc.select("section")
            .unwrap()
            .select(".outer p")
            .unwrap()
            .len(),
        1
    );

    let doc = parse(
        "<div class=\"outer\"><section><p>x</p></section><section><p>y</p></section></div>\
         <section><p>z</p></section>",
    )
    .unwrap();
    let texts: Vec<_> = doc
        .select("section")
        .unwrap()
        .select(".outer p")
        .unwrap()
        .iter()
        .map(|node| node.text_content())
        .collect();
    assert_eq!(texts, ["x", "y"]);
}

#[test]
fn sub_selections_over_nested_roots_are_unique_and_in_document_order() {
    const DEPTH: usize = 100;
    const LINKS_PER_LEVEL: usize = 50;
    let mut html = String::new();
    let mut next = 0usize;
    for _ in 0..DEPTH {
        html.push_str("<div>");
        for _ in 0..LINKS_PER_LEVEL {
            html.push_str(&format!("<a>{next}</a>"));
            next += 1;
        }
    }
    html.push_str(&"</div>".repeat(DEPTH));
    let doc = parse(&html).unwrap();
    let roots = doc.select("div").unwrap();
    assert_eq!(roots.len(), DEPTH);

    let start = Instant::now();
    let links = roots.select("a").unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < TIME_LIMIT,
        "nested-root sub-selection took {elapsed:?}"
    );

    let texts: Vec<_> = links.iter().map(|node| node.text_content()).collect();
    let expected: Vec<_> = (0..DEPTH * LINKS_PER_LEVEL)
        .map(|i| i.to_string())
        .collect();
    assert_eq!(texts, expected);
    let unique: HashSet<_> = links.node_ids().iter().copied().collect();
    assert_eq!(unique.len(), DEPTH * LINKS_PER_LEVEL);
}

#[test]
fn general_sibling_combinator_is_linear_in_the_sibling_count() {
    const SIBLINGS: usize = 40_000;
    let mut html = String::from("<div><p>before</p><h1>t</h1>");
    for index in 0..SIBLINGS {
        html.push_str(&format!("<p>{index}</p>"));
    }
    html.push_str("</div>");
    let doc = parse(&html).unwrap();

    let start = Instant::now();
    let matched = doc.select("h1 ~ p").unwrap();
    let chained = doc.select("h1 ~ p ~ p").unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < TIME_LIMIT, "`~` selection took {elapsed:?}");

    // Expected: the adjacent `p` and every element sibling after it.
    let adjacent = doc.select("h1 + p").unwrap();
    assert_eq!(adjacent.len(), 1);
    let mut expected = Vec::with_capacity(SIBLINGS);
    let mut sibling = adjacent.first();
    while let Some(node) = sibling {
        if node.tag() == Tag::P {
            expected.push(node.id());
        }
        sibling = node.next_sibling();
    }
    assert_eq!(expected.len(), SIBLINGS);
    assert_eq!(matched.node_ids(), expected.as_slice());
    assert_eq!(chained.node_ids(), &expected[1..]);
}

#[test]
fn descendant_sub_selection_over_many_roots_is_linear() {
    const ITEMS: usize = 2_000;
    const BOLD_PER_ITEM: usize = 49;
    let mut html = String::from("<ul>");
    for _ in 0..ITEMS {
        html.push_str("<li><a>");
        html.push_str(&"<b>x</b>".repeat(BOLD_PER_ITEM));
        html.push_str("</a></li>");
    }
    html.push_str("</ul>");
    let doc = parse(&html).unwrap();
    assert!(doc.arena().len() >= 200_000, "{}", doc.arena().len());
    let roots = doc.select("li").unwrap();
    assert_eq!(roots.len(), ITEMS);

    let start = Instant::now();
    let matched = roots.select("a b").unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < TIME_LIMIT, "multi-root `a b` took {elapsed:?}");

    assert_eq!(matched.len(), ITEMS * BOLD_PER_ITEM);
    assert_eq!(matched.node_ids(), doc.select("b").unwrap().node_ids());
}

#[test]
fn negations_never_match_the_document_root() {
    let doc = parse("<p>x</p>").unwrap();
    for selector in [":not(div) > p", ":not(*) p"] {
        assert_eq!(doc.select(selector).unwrap().len(), 0, "{selector}");
        assert!(doc.select_first(selector).unwrap().is_none(), "{selector}");
    }
}

#[cfg(feature = "xpath")]
#[test]
fn selection_xpath_positioned_text_scales_across_wide_sibling_roots() {
    const WIDTH: usize = 40_000;
    let mut html = String::with_capacity(WIDTH * 15 + 16);
    html.push_str("<ul>");
    for index in 1..=WIDTH {
        html.push_str(&format!("<li>{index}</li>"));
    }
    html.push_str("</ul>");
    let doc = parse(&html).unwrap();
    let roots = doc.select("li").unwrap();
    assert_eq!(roots.len(), WIDTH);

    let start = Instant::now();
    let result = roots.xpath("//li[40000]/text()").unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < TIME_LIMIT, "positioned text() took {elapsed:?}");

    let XPathResult::Strings(texts) = result else {
        panic!("expected string result");
    };
    assert_eq!(texts, ["40000"]);
}

#[test]
fn non_ascii_and_escaped_identifiers_are_accepted() {
    let doc = parse(
        "<div id='ürün' class='café md:flex'>a</div>\
         <p id='123' data-k='x]y' title='say \"hi\"'>b</p>\
         <section><x-ürün>c</x-ürün></section>",
    )
    .unwrap();

    for (css, expected) in [
        ("#ürün", "a"),
        (".café", "a"),
        ("div.md\\:flex", "a"),
        ("\\64 iv", "a"),
        ("#\\31 23", "b"),
        ("[data-k='x\\]y']", "b"),
        ("[data-k=x\\]y]", "b"),
        ("[title=\"say \\\"hi\\\"\"]", "b"),
        ("x-ürün", "c"),
        ("section X-ürün", "c"),
    ] {
        assert_eq!(doc.select(css).unwrap().text(), expected, "{css}");
    }
}

#[test]
fn nth_child_matches_in_arenas_built_through_the_public_api() {
    let mut arena = Arena::new();
    let root = arena.new_element(Tag::Unknown, 0);
    let list = arena.new_element(Tag::Ul, 1);
    arena.append_child(root, list);
    let mut items = Vec::new();
    for _ in 0..3 {
        let text = arena.new_text(2, " ");
        arena.append_child(list, text);
        let item = arena.new_element(Tag::Li, 2);
        arena.append_child(list, item);
        items.push(item);
    }

    let select = |css: &str| select_all_list(&arena, root, &parse_selector(css).unwrap());
    assert_eq!(select("li:nth-child(1)"), [items[0]]);
    assert_eq!(select("li:nth-child(1)"), select("li:first-child"));
    assert_eq!(select("li:nth-child(2n+1)"), [items[0], items[2]]);
    assert_eq!(select("li:nth-child(3)"), select("li:last-child"));
}

#[test]
fn find_by_attr_treats_valueless_attributes_as_empty() {
    let doc = parse("<input disabled><input disabled=''><input disabled='x'>").unwrap();

    let found = doc.find_by_attr("disabled", "");
    assert_eq!(found.len(), 2);
    assert_eq!(
        found.node_ids(),
        doc.select("[disabled='']").unwrap().node_ids()
    );
}

#[test]
fn pseudo_class_names_are_case_insensitive() {
    let doc = parse("<ul><li>1</li><li class=x>2</li><li>3</li></ul>").unwrap();

    assert_eq!(doc.select("li:First-Child").unwrap().text(), "1");
    assert_eq!(doc.select("li:LAST-CHILD").unwrap().text(), "3");
    assert_eq!(doc.select("li:Nth-Child(2)").unwrap().text(), "2");
    assert_eq!(doc.select("li:NOT(.x)").unwrap().text(), "13");
}

#[test]
fn selector_errors_report_the_character_at_its_input_offset() {
    // Positions are byte offsets into the untrimmed input.
    let err = parse_selector("  #ürün !").unwrap_err().to_string();
    assert!(
        err.contains("unexpected character '!' at position 10"),
        "{err}"
    );

    let err = parse_selector("  [a=b é").unwrap_err().to_string();
    assert!(
        err.contains("expected ']', found 'é' at position 7"),
        "{err}"
    );
}

#[cfg(feature = "xpath")]
#[test]
fn xpath_errors_report_the_character_at_its_input_offset() {
    let doc = parse("<p></p>").unwrap();

    let err = doc.xpath("//p[@a='x'é]").unwrap_err().to_string();
    assert!(err.contains("expected ']', got 'é'"), "{err}");

    let err = doc.xpath("  //[").unwrap_err().to_string();
    assert!(err.contains("expected identifier at position 4"), "{err}");
}
