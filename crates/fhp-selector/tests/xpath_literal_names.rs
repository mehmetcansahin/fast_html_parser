use fhp_selector::Selectable;
use fhp_selector::xpath::ast::XPathResult;
use fhp_tree::parse;

#[test]
fn xpath_matches_non_interned_standard_and_custom_names() {
    let doc = parse(
        "<select><option id='one'>one</option><option id='two'>two</option></select>\
         <my-widget data-kind='card'>custom</my-widget>",
    )
    .unwrap();

    let XPathResult::Nodes(options) = doc.xpath("//OPTION").unwrap() else {
        panic!("expected nodes");
    };
    assert_eq!(options.len(), 2);

    let XPathResult::Nodes(widgets) = doc.xpath("//MY-WIDGET[@data-kind='card']").unwrap() else {
        panic!("expected nodes");
    };
    assert_eq!(widgets.len(), 1);
}

#[test]
fn xpath_literal_names_work_in_absolute_paths_and_positions() {
    let doc =
        parse("<select><option>one</option><my-widget>x</my-widget><option>two</option></select>")
            .unwrap();

    let XPathResult::Nodes(path) = doc.xpath("/select/option").unwrap() else {
        panic!("expected nodes");
    };
    assert_eq!(path.len(), 2);

    let XPathResult::Nodes(positioned) = doc.xpath("//option[2]").unwrap() else {
        panic!("expected nodes");
    };
    assert_eq!(positioned.len(), 1);
    assert_eq!(doc.get(positioned[0]).text_content(), "two");
}
