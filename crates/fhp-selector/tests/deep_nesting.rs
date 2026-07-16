//! Regression test for strict failure at the nesting depth limit.

use fhp_core::error::ParseError;
use fhp_tree::{HtmlError, parse};

#[test]
fn depth_overflow_returns_a_typed_error_without_a_partial_document() {
    let mut html = String::new();
    for _ in 0..1000 {
        html.push_str("<x>");
    }

    assert!(
        matches!(
            parse(&html),
            Err(HtmlError::Parse(ParseError::NestingTooDeep {
                depth: 513,
                limit: 512
            }))
        ),
        "the 513th real element must fail instead of returning a truncated DOM"
    );
}
