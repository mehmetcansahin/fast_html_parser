//! `HtmlParser::reparse_*` parse into an existing document with the
//! configured limit and leave it unchanged when the limit rejects the input.

use fast_html_parser::{Document, HtmlError, HtmlParser};

fn digest(doc: &Document) -> (usize, String) {
    (doc.node_count(), doc.to_html())
}

#[test]
fn reparse_methods_match_their_one_shot_counterparts() {
    let parser = HtmlParser::builder().max_input_size(1024).build();
    let mut doc = parser.parse_str("<p>seed</p>").unwrap();

    parser
        .reparse_str(&mut doc, "<ul><li>a</li><li>b &amp; c</li></ul>")
        .unwrap();
    assert_eq!(
        digest(&doc),
        digest(
            &parser
                .parse_str("<ul><li>a</li><li>b &amp; c</li></ul>")
                .unwrap()
        )
    );

    let owned = String::from("<table><tr><td>cell</td></tr></table>");
    parser.reparse_str_owned(&mut doc, owned.clone()).unwrap();
    assert_eq!(
        digest(&doc),
        digest(&parser.parse_str_owned(owned).unwrap())
    );

    #[cfg(feature = "encoding")]
    {
        let raw = &b"<meta charset=\"windows-1252\"><p>caf\xE9</p>"[..];
        parser.reparse_raw(&mut doc, raw).unwrap();
        assert_eq!(digest(&doc), digest(&parser.parse_raw(raw).unwrap()));
        assert_eq!(doc.root().text_content(), "caf\u{E9}");
    }
}

#[test]
fn reparse_methods_apply_the_configured_limit_and_keep_the_document() {
    let parser = HtmlParser::builder().max_input_size(16).build();
    let mut doc = parser.parse_str("<p>kept</p>").unwrap();
    let before = digest(&doc);
    let too_long = "<p>longer than sixteen bytes</p>";

    let err = parser.reparse_str(&mut doc, too_long).unwrap_err();
    assert!(matches!(err, HtmlError::InputTooLarge { max: 16, .. }));
    assert_eq!(digest(&doc), before);

    let err = parser
        .reparse_str_owned(&mut doc, too_long.to_owned())
        .unwrap_err();
    assert!(matches!(err, HtmlError::InputTooLarge { max: 16, .. }));
    assert_eq!(digest(&doc), before);

    #[cfg(feature = "encoding")]
    {
        let err = parser
            .reparse_raw(&mut doc, too_long.as_bytes())
            .unwrap_err();
        assert!(matches!(err, HtmlError::InputTooLarge { max: 16, .. }));
        assert_eq!(digest(&doc), before);
    }

    // The same parser still accepts input within the limit.
    parser.reparse_str(&mut doc, "<p>ok</p>").unwrap();
    assert_eq!(doc.root().text_content(), "ok");
}
