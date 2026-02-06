//! Basic HTML parsing example.
//!
//! Demonstrates one-shot parsing, tree traversal, and text extraction.
//!
//! Run: `cargo run --example basic_parse`

use hp_parser::HtmlParser;

fn main() {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Example Page</title></head>
        <body>
            <h1>Welcome</h1>
            <p>This is a <b>simple</b> paragraph.</p>
            <ul>
                <li>Item 1</li>
                <li>Item 2</li>
                <li>Item 3</li>
            </ul>
        </body>
        </html>
    "#;

    // Parse HTML
    let doc = HtmlParser::parse(html).unwrap();
    let root = doc.root();

    println!("Node count: {}", doc.node_count());
    println!("Text content:\n{}", root.text_content().trim());

    // Traverse children
    println!("\nRoot children:");
    for child_id in root.children() {
        let child = doc.get(child_id);
        println!("  <{}>", child.tag());
    }

    // Builder pattern with custom limit
    let parser = HtmlParser::builder().max_input_size(1024 * 1024).build();
    let doc = parser.parse_str("<p>configured</p>").unwrap();
    println!("\nBuilder result: {}", doc.root().text_content());
}
