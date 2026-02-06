//! Comprehensive integration tests for hp-selector.

use hp_core::tag::Tag;
use hp_selector::Selectable;
use hp_selector::xpath::ast::XPathResult;
use hp_tree::parse;

// ---------------------------------------------------------------
// Simple selectors
// ---------------------------------------------------------------

#[test]
fn select_by_tag() {
    let doc = parse("<div><p>Hello</p><span>World</span></div>").unwrap();
    let sel = doc.select("p").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "Hello");
}

#[test]
fn select_by_class() {
    let doc = parse("<div class=\"a\"><span class=\"b\">x</span></div>").unwrap();
    let sel = doc.select(".b").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "x");
}

#[test]
fn select_by_id() {
    let doc = parse("<div id=\"main\">content</div>").unwrap();
    let sel = doc.select("#main").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "content");
}

#[test]
fn select_universal() {
    let doc = parse("<div><p>a</p><span>b</span></div>").unwrap();
    let sel = doc.select("*").unwrap();
    // root, div, p, span — all elements
    assert!(sel.len() >= 3);
}

// ---------------------------------------------------------------
// Attribute selectors
// ---------------------------------------------------------------

#[test]
fn select_attr_exists() {
    let doc = parse("<a href=\"x\">link</a><span>text</span>").unwrap();
    let sel = doc.select("[href]").unwrap();
    assert_eq!(sel.len(), 1);
}

#[test]
fn select_attr_equals() {
    let doc = parse("<a href=\"x\">a</a><a href=\"y\">b</a>").unwrap();
    let sel = doc.select("[href=\"x\"]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "a");
}

#[test]
fn select_attr_includes() {
    let doc = parse("<div class=\"foo bar baz\">x</div><div class=\"qux\">y</div>").unwrap();
    let sel = doc.select("[class~=bar]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "x");
}

#[test]
fn select_attr_starts_with() {
    let doc = parse("<a href=\"https://a.com\">a</a><a href=\"http://b.com\">b</a>").unwrap();
    let sel = doc.select("[href^=\"https\"]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "a");
}

#[test]
fn select_attr_ends_with() {
    let doc = parse("<a href=\"a.html\">a</a><a href=\"b.php\">b</a>").unwrap();
    let sel = doc.select("[href$=\".html\"]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "a");
}

#[test]
fn select_attr_substring() {
    let doc = parse("<a href=\"https://example.com\">a</a><a href=\"other\">b</a>").unwrap();
    let sel = doc.select("[href*=\"example\"]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "a");
}

// ---------------------------------------------------------------
// Combinators
// ---------------------------------------------------------------

#[test]
fn select_descendant() {
    let doc = parse("<div><ul><li>1</li><li>2</li></ul></div>").unwrap();
    let sel = doc.select("div li").unwrap();
    assert_eq!(sel.len(), 2);
}

#[test]
fn select_child() {
    let doc = parse("<div><p>direct</p><span><p>nested</p></span></div>").unwrap();
    let sel = doc.select("div > p").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "direct");
}

#[test]
fn select_adjacent_sibling() {
    let doc = parse("<div><h1>T</h1><p>A</p><p>B</p></div>").unwrap();
    let sel = doc.select("h1 + p").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "A");
}

#[test]
fn select_general_sibling() {
    let doc = parse("<div><h1>T</h1><p>A</p><span>X</span><p>B</p></div>").unwrap();
    let sel = doc.select("h1 ~ p").unwrap();
    assert_eq!(sel.len(), 2);
}

// ---------------------------------------------------------------
// Pseudo-classes
// ---------------------------------------------------------------

#[test]
fn select_first_child() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li></ul>").unwrap();
    let sel = doc.select("li:first-child").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "1");
}

#[test]
fn select_last_child() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li></ul>").unwrap();
    let sel = doc.select("li:last-child").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "3");
}

#[test]
fn select_nth_child_odd() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li><li>4</li></ul>").unwrap();
    let sel = doc.select("li:nth-child(odd)").unwrap();
    assert_eq!(sel.len(), 2);
    assert_eq!(sel.text(), "13");
}

#[test]
fn select_nth_child_even() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li><li>4</li></ul>").unwrap();
    let sel = doc.select("li:nth-child(even)").unwrap();
    assert_eq!(sel.len(), 2);
    assert_eq!(sel.text(), "24");
}

#[test]
fn select_nth_child_2n_plus_1() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li><li>4</li><li>5</li></ul>").unwrap();
    let sel = doc.select("li:nth-child(2n+1)").unwrap();
    assert_eq!(sel.len(), 3);
    assert_eq!(sel.text(), "135");
}

#[test]
fn select_not_class() {
    let doc = parse(
        "<div class=\"visible\">a</div><div class=\"hidden\">b</div><div class=\"visible\">c</div>",
    )
    .unwrap();
    let sel = doc.select("div:not(.hidden)").unwrap();
    assert_eq!(sel.len(), 2);
    assert_eq!(sel.text(), "ac");
}

// ---------------------------------------------------------------
// Compound selectors
// ---------------------------------------------------------------

#[test]
fn select_compound_tag_class_id() {
    let doc =
        parse("<div class=\"active\" id=\"main\">yes</div><div class=\"active\">no</div>").unwrap();
    let sel = doc.select("div.active#main").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "yes");
}

#[test]
fn select_compound_tag_attr() {
    let doc = parse("<a href=\"x\" class=\"link\">a</a><a class=\"link\">b</a>").unwrap();
    let sel = doc.select("a[href]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "a");
}

#[test]
fn select_compound_with_attr_value() {
    let doc = parse(
        "<div class=\"active\" data-x=\"1\">a</div><div class=\"active\" data-x=\"2\">b</div>",
    )
    .unwrap();
    let sel = doc.select("div.active[data-x=\"1\"]").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "a");
}

// ---------------------------------------------------------------
// Chaining
// ---------------------------------------------------------------

#[test]
fn select_chaining_ul_li_a() {
    let doc = parse(
        "<ul><li><a href=\"1\">A</a></li><li><a href=\"2\">B</a></li></ul><a href=\"3\">C</a>",
    )
    .unwrap();
    let lis = doc.select("ul").unwrap();
    assert_eq!(lis.len(), 1);
    let links = lis.select("li > a").unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links.text(), "AB");
}

// ---------------------------------------------------------------
// Comma-separated selectors
// ---------------------------------------------------------------

#[test]
fn select_comma_list() {
    let doc = parse("<div>a</div><span>b</span><p>c</p>").unwrap();
    let sel = doc.select("div, p").unwrap();
    assert_eq!(sel.len(), 2);
}

// ---------------------------------------------------------------
// Convenience API
// ---------------------------------------------------------------

#[test]
fn find_by_tag() {
    let doc = parse("<div><span>a</span><span>b</span></div>").unwrap();
    let sel = doc.find_by_tag(Tag::Span);
    assert_eq!(sel.len(), 2);
}

#[test]
fn find_by_id() {
    let doc = parse("<div id=\"target\">found</div>").unwrap();
    let node = doc.find_by_id("target");
    assert!(node.is_some());
    assert_eq!(node.unwrap().text_content(), "found");
}

#[test]
fn find_by_class() {
    let doc =
        parse("<p class=\"intro\">a</p><p class=\"body\">b</p><p class=\"intro\">c</p>").unwrap();
    let sel = doc.find_by_class("intro");
    assert_eq!(sel.len(), 2);
}

#[test]
fn find_by_attr() {
    let doc = parse("<img src=\"a.png\"><img src=\"b.png\">").unwrap();
    let sel = doc.find_by_attr("src", "a.png");
    assert_eq!(sel.len(), 1);
}

// ---------------------------------------------------------------
// DocumentIndex
// ---------------------------------------------------------------

#[test]
fn document_index_o1_lookup() {
    let doc = parse("<div id=\"a\">x</div><div id=\"b\">y</div><div id=\"c\">z</div>").unwrap();
    let idx = hp_selector::DocumentIndex::build(&doc);
    assert_eq!(idx.find_by_id(&doc, "b").unwrap().text_content(), "y");
    assert_eq!(idx.find_by_id(&doc, "c").unwrap().text_content(), "z");
    assert!(idx.find_by_id(&doc, "missing").is_none());
}

// ---------------------------------------------------------------
// Bloom filter accuracy
// ---------------------------------------------------------------

#[test]
fn bloom_filter_no_false_negatives() {
    // If a descendant selector matches, the bloom must not reject it.
    let doc = parse("<div><section><p class=\"target\">found</p></section></div>").unwrap();
    let sel = doc.select("div p.target").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "found");
}

#[test]
fn bloom_deep_nesting() {
    let doc = parse(
        "<div><section><article><ul><li><a class=\"deep\">x</a></li></ul></article></section></div>",
    )
    .unwrap();
    let sel = doc.select("div a.deep").unwrap();
    assert_eq!(sel.len(), 1);
}

// ---------------------------------------------------------------
// Large HTML selector performance
// ---------------------------------------------------------------

#[test]
fn large_html_selector() {
    let mut html = String::with_capacity(50000);
    html.push_str("<div>");
    for i in 0..500 {
        if i % 5 == 0 {
            html.push_str(&format!("<p class=\"highlight\">{i}</p>"));
        } else {
            html.push_str(&format!("<p>{i}</p>"));
        }
    }
    html.push_str("</div>");

    let doc = parse(&html).unwrap();

    let all_p = doc.select("p").unwrap();
    assert_eq!(all_p.len(), 500);

    let highlighted = doc.select("p.highlight").unwrap();
    assert_eq!(highlighted.len(), 100);

    let child = doc.select("div > p").unwrap();
    assert_eq!(child.len(), 500);

    let first = doc.select("p:first-child").unwrap();
    assert_eq!(first.len(), 1);

    let last = doc.select("p:last-child").unwrap();
    assert_eq!(last.len(), 1);

    let descendant = doc.select("div p.highlight").unwrap();
    assert_eq!(descendant.len(), 100);
}

// ---------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------

#[test]
fn empty_document() {
    let doc = parse("").unwrap();
    let sel = doc.select("div").unwrap();
    assert!(sel.is_empty());
}

#[test]
fn text_only_document() {
    let doc = parse("just text").unwrap();
    let sel = doc.select("div").unwrap();
    assert!(sel.is_empty());
}

#[test]
fn invalid_selector() {
    let doc = parse("<div>x</div>").unwrap();
    assert!(doc.select("").is_err());
    assert!(doc.select("!!!").is_err());
}

#[test]
fn void_element_select() {
    let doc = parse("<div><br><img src=\"x.png\"><hr></div>").unwrap();
    let sel = doc.select("br").unwrap();
    assert_eq!(sel.len(), 1);
    let sel = doc.select("img[src]").unwrap();
    assert_eq!(sel.len(), 1);
}

#[test]
fn deep_descendant() {
    let doc = parse(
        "<div><div><div><div><div><span class=\"deep\">found</span></div></div></div></div></div>",
    )
    .unwrap();
    let sel = doc.select("div span.deep").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "found");
}

#[test]
fn multiple_classes() {
    let doc = parse("<div class=\"a b c\">x</div>").unwrap();
    assert_eq!(doc.select(".a").unwrap().len(), 1);
    assert_eq!(doc.select(".b").unwrap().len(), 1);
    assert_eq!(doc.select(".c").unwrap().len(), 1);
    assert_eq!(doc.select(".d").unwrap().len(), 0);
}

#[test]
fn selection_attr_and_inner_html() {
    let doc = parse("<a href=\"url\" class=\"link\"><b>bold</b></a>").unwrap();
    let sel = doc.select("a").unwrap();
    assert_eq!(sel.attr("href"), Some("url"));
    assert_eq!(sel.inner_html(), "<b>bold</b>");
}

#[test]
fn nth_child_negative_formula() {
    // :nth-child(-n+3) selects first 3 children
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li><li>4</li><li>5</li></ul>").unwrap();
    let sel = doc.select("li:nth-child(-n+3)").unwrap();
    assert_eq!(sel.len(), 3);
    assert_eq!(sel.text(), "123");
}

#[test]
fn complex_selector_chain() {
    let html = r#"
        <div id="nav">
            <ul>
                <li class="active"><a href="/home">Home</a></li>
                <li><a href="/about">About</a></li>
            </ul>
        </div>
    "#;
    let doc = parse(html).unwrap();
    let sel = doc.select("#nav ul > li.active a").unwrap();
    assert_eq!(sel.len(), 1);
    assert_eq!(sel.text(), "Home");
}

// ---------------------------------------------------------------
// XPath: descendant search
// ---------------------------------------------------------------

#[test]
fn xpath_descendant_tag() {
    let doc = parse("<div><p>Hello</p><p>World</p></div>").unwrap();
    let result = doc.xpath("//p").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 2),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_descendant_nested() {
    let doc = parse("<div><section><article><p>deep</p></article></section></div>").unwrap();
    let result = doc.xpath("//p").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

// ---------------------------------------------------------------
// XPath: attribute predicates
// ---------------------------------------------------------------

#[test]
fn xpath_attr_equals() {
    let doc = parse("<a href=\"x\">a</a><a href=\"y\">b</a>").unwrap();
    let result = doc.xpath("//a[@href='x']").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_attr_exists() {
    let doc = parse("<a href=\"x\">a</a><span>b</span>").unwrap();
    let result = doc.xpath("//a[@href]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

// ---------------------------------------------------------------
// XPath: contains predicate
// ---------------------------------------------------------------

#[test]
fn xpath_contains() {
    let doc = parse("<div class=\"nav-main\">a</div><div class=\"footer\">b</div>").unwrap();
    let result = doc.xpath("//div[contains(@class, 'nav')]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

// ---------------------------------------------------------------
// XPath: position predicate
// ---------------------------------------------------------------

#[test]
fn xpath_position() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li></ul>").unwrap();
    let result = doc.xpath("//li[position()=2]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_position_shorthand() {
    let doc = parse("<ul><li>1</li><li>2</li><li>3</li></ul>").unwrap();
    let result = doc.xpath("//li[1]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

// ---------------------------------------------------------------
// XPath: text extraction
// ---------------------------------------------------------------

#[test]
fn xpath_text_extract() {
    let doc = parse("<div><p>Hello</p><p>World</p></div>").unwrap();
    let result = doc.xpath("//p/text()").unwrap();
    match result {
        XPathResult::Strings(texts) => {
            assert_eq!(texts.len(), 2);
            assert_eq!(texts[0], "Hello");
            assert_eq!(texts[1], "World");
        }
        _ => panic!("expected Strings"),
    }
}

#[test]
fn xpath_text_nested() {
    let doc = parse("<p><b>bold</b> text</p>").unwrap();
    let result = doc.xpath("//p/text()").unwrap();
    match result {
        XPathResult::Strings(texts) => {
            assert_eq!(texts.len(), 1);
            assert_eq!(texts[0], "bold text");
        }
        _ => panic!("expected Strings"),
    }
}

// ---------------------------------------------------------------
// XPath: absolute path
// ---------------------------------------------------------------

#[test]
fn xpath_absolute_path() {
    let doc = parse("<html><body><div>content</div></body></html>").unwrap();
    let result = doc.xpath("/html/body/div").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_absolute_path_with_predicate() {
    let doc = parse("<html><body><div class=\"main\">a</div><div>b</div></body></html>").unwrap();
    let result = doc.xpath("/html/body/div[@class='main']").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_absolute_path_text() {
    let doc = parse("<html><body><p>text</p></body></html>").unwrap();
    let result = doc.xpath("/html/body/p/text()").unwrap();
    match result {
        XPathResult::Strings(texts) => {
            assert_eq!(texts.len(), 1);
            assert_eq!(texts[0], "text");
        }
        _ => panic!("expected Strings"),
    }
}

// ---------------------------------------------------------------
// XPath: wildcard
// ---------------------------------------------------------------

#[test]
fn xpath_wildcard() {
    let doc = parse("<div><p>a</p><span>b</span></div>").unwrap();
    let result = doc.xpath("//*").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert!(nodes.len() >= 3),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_wildcard_attr() {
    let doc = parse("<div id=\"main\">a</div><span>b</span>").unwrap();
    let result = doc.xpath("//*[@id='main']").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}

// ---------------------------------------------------------------
// XPath: edge cases
// ---------------------------------------------------------------

#[test]
fn xpath_empty_document() {
    let doc = parse("").unwrap();
    let result = doc.xpath("//div").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert!(nodes.is_empty()),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_no_match() {
    let doc = parse("<div>text</div>").unwrap();
    let result = doc.xpath("//span").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert!(nodes.is_empty()),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_position_out_of_range() {
    let doc = parse("<ul><li>1</li></ul>").unwrap();
    let result = doc.xpath("//li[position()=5]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert!(nodes.is_empty()),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn xpath_invalid_expr() {
    let doc = parse("<div>x</div>").unwrap();
    assert!(doc.xpath("").is_err());
    assert!(doc.xpath("bad").is_err());
    assert!(doc.xpath("//foobar").is_err());
}

// ---------------------------------------------------------------
// CSS vs XPath comparison tests
// ---------------------------------------------------------------

#[test]
fn css_xpath_same_result_tag() {
    let doc = parse("<div><p>a</p><p>b</p></div>").unwrap();
    let css = doc.select("p").unwrap();
    let xpath = doc.xpath("//p").unwrap();
    match xpath {
        XPathResult::Nodes(nodes) => assert_eq!(css.len(), nodes.len()),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn css_xpath_same_result_attr() {
    let doc = parse("<a href=\"x\">a</a><a href=\"y\">b</a>").unwrap();
    let css = doc.select("[href=\"x\"]").unwrap();
    let xpath = doc.xpath("//a[@href='x']").unwrap();
    match xpath {
        XPathResult::Nodes(nodes) => assert_eq!(css.len(), nodes.len()),
        _ => panic!("expected Nodes"),
    }
}

#[test]
fn css_xpath_same_result_multiple() {
    let html = "<div><ul><li>1</li><li>2</li><li>3</li></ul></div>";
    let doc = parse(html).unwrap();
    let css = doc.select("li").unwrap();
    let xpath = doc.xpath("//li").unwrap();
    match xpath {
        XPathResult::Nodes(nodes) => assert_eq!(css.len(), nodes.len()),
        _ => panic!("expected Nodes"),
    }
}

// ---------------------------------------------------------------
// XPath: large HTML
// ---------------------------------------------------------------

#[test]
fn xpath_large_html() {
    let mut html = String::with_capacity(50000);
    html.push_str("<div>");
    for i in 0..500 {
        if i % 5 == 0 {
            html.push_str(&format!("<p class=\"highlight\">{i}</p>"));
        } else {
            html.push_str(&format!("<p>{i}</p>"));
        }
    }
    html.push_str("</div>");

    let doc = parse(&html).unwrap();

    let result = doc.xpath("//p").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 500),
        _ => panic!("expected Nodes"),
    }

    let result = doc.xpath("//p[contains(@class, 'highlight')]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 100),
        _ => panic!("expected Nodes"),
    }

    let result = doc.xpath("//p[1]").unwrap();
    match result {
        XPathResult::Nodes(nodes) => assert_eq!(nodes.len(), 1),
        _ => panic!("expected Nodes"),
    }
}
