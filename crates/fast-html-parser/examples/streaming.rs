//! Streaming (chunk-based) parsing example.
//!
//! Demonstrates incremental parsing with `StreamParser` and early
//! termination with `EarlyStopParser`.
//!
//! Run: `cargo run --example streaming`

use fast_html_parser::Tag;
use fast_html_parser::streaming::{
    EarlyStopOutcome, EarlyStopParser, EarlyStopProgress, StreamParser, parse_stream,
};

fn main() {
    // --- parse_stream convenience ---
    println!("=== parse_stream ===");
    let html = b"<div><p>Hello from chunks</p></div>";
    let doc = parse_stream(html.chunks(8)).unwrap();
    println!("Text: {}", doc.root().text_content());

    // --- StreamParser step-by-step ---
    println!("\n=== StreamParser ===");
    let mut parser = StreamParser::new();
    parser.feed(b"<html><body>").unwrap();
    parser.feed(b"<h1>Title</h1>").unwrap();
    parser.feed(b"<p>Paragraph</p>").unwrap();
    parser.feed(b"</body></html>").unwrap();
    let doc = parser.finish().unwrap();
    println!("Node count: {}", doc.node_count());
    println!("Text: {}", doc.root().text_content());

    // --- EarlyStopParser ---
    println!("\n=== EarlyStopParser ===");
    let mut early = EarlyStopParser::stop_on_create(|node| node.tag() == Tag::A);

    let status = early
        .feed(b"<div><p>text</p><ul><li>item</li></ul>")
        .unwrap();
    println!("After first chunk: {status:?}");

    let status = early.feed(b"<a href=\"/page\">link</a></div>").unwrap();
    match status {
        EarlyStopProgress::Matched => match early.finish().unwrap() {
            EarlyStopOutcome::Matched(found) => {
                println!("Found <a> tag at node {:?}", found.node_id())
            }
            EarlyStopOutcome::Done(_) => unreachable!(),
        },
        other => println!("Unexpected: {other:?}"),
    }
}
