//! Web scraping example using CSS selectors.
//!
//! Demonstrates selecting elements, extracting attributes, and chaining.
//!
//! Run: `cargo run --example web_scraping --features css-selector`

use fast_html_parser::prelude::*;

fn main() {
    let html = r#"
        <html>
        <body>
            <nav>
                <a href="/" class="active">Home</a>
                <a href="/about">About</a>
                <a href="/contact">Contact</a>
            </nav>
            <main>
                <article class="post">
                    <h2>First Post</h2>
                    <p class="summary">This is the first post summary.</p>
                    <a href="/posts/1">Read more</a>
                </article>
                <article class="post">
                    <h2>Second Post</h2>
                    <p class="summary">This is the second post summary.</p>
                    <a href="/posts/2">Read more</a>
                </article>
            </main>
        </body>
        </html>
    "#;

    let doc = HtmlParser::parse(html).unwrap();

    // Extract all links
    println!("All links:");
    let links = doc.select("a").unwrap();
    for node in links.iter() {
        let href = node.attr("href").unwrap_or("-");
        let text = node.text_content();
        println!("  [{text}]({href})");
    }

    // Find articles and extract titles
    println!("\nArticle titles:");
    let articles = doc.select("article.post").unwrap();
    let titles = articles.select("h2").unwrap();
    for node in titles.iter() {
        println!("  - {}", node.text_content());
    }

    // Extract summaries
    println!("\nSummaries:");
    let summaries = doc.select("p.summary").unwrap();
    for node in summaries.iter() {
        println!("  {}", node.text_content());
    }

    // Find by ID
    println!("\nActive nav link:");
    let active = doc.select("a.active").unwrap();
    if let Some(node) = active.first() {
        println!(
            "  {} -> {}",
            node.text_content(),
            node.attr("href").unwrap_or("-")
        );
    }

    // Pre-compiled selector — parse once, reuse across many documents.
    // Ideal for scraping loops where the same selector is applied repeatedly.
    println!("\nUsing CompiledSelector:");
    let link_sel = CompiledSelector::new("a").unwrap();
    let docs = vec![
        HtmlParser::parse("<a href=\"/one\">One</a>").unwrap(),
        HtmlParser::parse("<a href=\"/two\">Two</a>").unwrap(),
    ];
    for d in &docs {
        let links = d.select_compiled(&link_sel).unwrap();
        for node in links.iter() {
            println!(
                "  [{text}]({href})",
                text = node.text_content(),
                href = node.attr("href").unwrap_or("-")
            );
        }
    }
}
