#![no_main]

use fhp_tree::Document;
use fhp_tree::streaming::{EarlyStopOutcome, EarlyStopParser, StreamParser};
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

/// Every node sits exactly one level below its parent. Depths are fixed up
/// lazily after adoption agency moves, so check the finished document.
fn assert_depths_are_exact(document: &Document) {
    let arena = document.arena();
    for node in document.root().descendants() {
        let parent = arena.get(node).parent;
        if !parent.is_null() {
            assert_eq!(arena.get(node).depth, arena.get(parent).depth + 1);
        }
    }
}

fn outcome(result: Result<Document, fhp_tree::HtmlError>) -> Outcome {
    match result {
        Ok(document) => {
            assert_depths_are_exact(&document);
            Outcome::Document(document.to_html())
        }
        Err(error) => Outcome::Error(error_kind(&error)),
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let chunk_size = usize::from(data[0]).max(1);
    let html = &data[1..];
    let expected = outcome(fhp_tree::parse_bytes(html));

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
        None => outcome(parser.finish()),
    };
    assert_eq!(actual, expected);

    // The early-stop predicate sees nodes mid-parse, so their depths must be
    // exact after every token. It never matches, so the whole input is parsed.
    let mut early = EarlyStopParser::stop_on_create(|node| {
        assert_eq!(usize::from(node.depth()), node.ancestors().count());
        false
    });
    let mut feed_error = None;
    for chunk in html.chunks(chunk_size) {
        if let Err(error) = early.feed(chunk) {
            feed_error = Some(error);
            break;
        }
    }
    let early_outcome = match feed_error {
        Some(error) => Outcome::Error(error_kind(&error)),
        None => outcome(early.finish().map(|outcome| match outcome {
            EarlyStopOutcome::Done(document) => document,
            EarlyStopOutcome::Matched(_) => unreachable!("the predicate never matches"),
        })),
    };
    assert_eq!(early_outcome, expected);
});
