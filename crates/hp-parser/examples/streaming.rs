//! Streaming (chunk-based) parsing example.
//!
//! Demonstrates incremental parsing with `StreamParser` and early
//! termination with `EarlyStopParser`.
//!
//! Run: `cargo run --example streaming`

use hp_parser::Tag;
use hp_parser::streaming::{EarlyStopParser, ParseStatus, StreamParser, parse_stream};

fn main() {
    // --- parse_stream convenience ---
    println!("=== parse_stream ===");
    let html = b"<div><p>Hello from chunks</p></div>";
    let doc = parse_stream(html.chunks(8)).unwrap();
    println!("Text: {}", doc.root().text_content());

    // --- StreamParser step-by-step ---
    println!("\n=== StreamParser ===");
    let mut parser = StreamParser::new();
    parser.feed(b"<html><body>");
    parser.feed(b"<h1>Title</h1>");
    parser.feed(b"<p>Paragraph</p>");
    parser.feed(b"</body></html>");
    let doc = parser.finish().unwrap();
    println!("Node count: {}", doc.node_count());
    println!("Text: {}", doc.root().text_content());

    // --- EarlyStopParser ---
    println!("\n=== EarlyStopParser ===");
    let mut early = EarlyStopParser::stop_when(|node| node.tag == Tag::A);

    let status = early.feed(b"<div><p>text</p><ul><li>item</li></ul>");
    println!("After first chunk: {status:?}");

    let status = early.feed(b"<a href=\"/page\">link</a></div>");
    match status {
        ParseStatus::Found(id) => println!("Found <a> tag at node {id:?}"),
        other => println!("Unexpected: {other:?}"),
    }
}
