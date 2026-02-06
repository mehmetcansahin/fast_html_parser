//! XPath query example.
//!
//! Demonstrates evaluating XPath expressions against a parsed document.
//!
//! Run: `cargo run --example xpath_query --features xpath`

use hp_parser::prelude::*;
use hp_parser::xpath::XPathResult;

fn main() {
    let html = r#"
        <html>
        <body>
            <div id="content">
                <h1>Main Title</h1>
                <p class="intro">Introduction paragraph.</p>
                <ul>
                    <li>Alpha</li>
                    <li>Beta</li>
                    <li>Gamma</li>
                </ul>
                <a href="/page1">Link 1</a>
                <a href="/page2">Link 2</a>
            </div>
        </body>
        </html>
    "#;

    let doc = HtmlParser::parse(html).unwrap();

    // Find all <li> elements
    println!("=== //li ===");
    match doc.xpath("//li").unwrap() {
        XPathResult::Nodes(nodes) => {
            println!("Found {} <li> elements", nodes.len());
            for id in &nodes {
                println!("  - {}", doc.get(*id).text_content());
            }
        }
        _ => println!("unexpected"),
    }

    // Extract text from <h1>
    println!("\n=== //h1/text() ===");
    match doc.xpath("//h1/text()").unwrap() {
        XPathResult::Strings(texts) => {
            for t in &texts {
                println!("  {t}");
            }
        }
        _ => println!("unexpected"),
    }

    // Find links with attribute predicate
    println!("\n=== //a[@href='/page1'] ===");
    match doc.xpath("//a[@href='/page1']").unwrap() {
        XPathResult::Nodes(nodes) => {
            for id in &nodes {
                let node = doc.get(*id);
                println!(
                    "  {} (href={})",
                    node.text_content(),
                    node.attr("href").unwrap_or("-")
                );
            }
        }
        _ => println!("unexpected"),
    }

    // Absolute path
    println!("\n=== /html/body/div/ul/li ===");
    match doc.xpath("/html/body/div/ul/li").unwrap() {
        XPathResult::Nodes(nodes) => {
            println!("Found {} nodes via absolute path", nodes.len());
        }
        _ => println!("unexpected"),
    }
}
