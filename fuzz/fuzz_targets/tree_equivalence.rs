#![no_main]

use fhp_tree::streaming::StreamParser;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Document(String),
    Error(&'static str),
}

fn error_kind(error: &fhp_tree::HtmlError) -> &'static str {
    match error {
        fhp_tree::HtmlError::InputTooLarge { .. } => "input-too-large",
        fhp_tree::HtmlError::Parse(_) => "parse",
        fhp_tree::HtmlError::ParserTerminated => "terminated",
        fhp_tree::HtmlError::Io(_) => "io",
        _ => "future-error",
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let chunk_size = usize::from(data[0]).max(1);
    let html = &data[1..];
    let expected = match fhp_tree::parse_bytes(html) {
        Ok(document) => Outcome::Document(document.to_html()),
        Err(error) => Outcome::Error(error_kind(&error)),
    };

    let mut parser = StreamParser::new();
    let mut feed_error = None;
    for chunk in html.chunks(chunk_size) {
        if let Err(error) = parser.feed(chunk) {
            feed_error = Some(error);
            break;
        }
    }
    let actual = match feed_error {
        Some(error) => Outcome::Error(error_kind(&error)),
        None => match parser.finish() {
            Ok(document) => Outcome::Document(document.to_html()),
            Err(error) => Outcome::Error(error_kind(&error)),
        },
    };

    assert_eq!(actual, expected);
});
