//! Comparison benchmarks against popular Rust HTML parsers.
//!
//! Compares parse throughput of:
//! - `fast-html-parser` (this crate)
//! - `tl` (zero-copy, fast)
//! - `scraper` (html5ever wrapper, spec-compliant)
//! - `lol_html` (Cloudflare streaming rewriter)

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

const SMALL_HTML: &str = include_str!("../../../testdata/small_1kb.html");
const MEDIUM_HTML: &str = include_str!("../../../testdata/medium_100kb.html");
const LARGE_HTML: &str = include_str!("../../../testdata/large_5mb.html");

// ---------------------------------------------------------------------------
// Parse throughput comparison
// ---------------------------------------------------------------------------

fn bench_parse_comparison(c: &mut Criterion) {
    let inputs: &[(&str, &str)] = &[
        ("1kb", SMALL_HTML),
        ("100kb", MEDIUM_HTML),
        ("5mb", LARGE_HTML),
    ];

    for &(name, html) in inputs {
        let mut group = c.benchmark_group(format!("parse_{name}"));
        group.throughput(Throughput::Bytes(html.len() as u64));

        // fast-html-parser (ours)
        group.bench_function("fast_html_parser", |b| {
            b.iter(|| fast_html_parser::HtmlParser::parse(html).unwrap());
        });

        // tl
        group.bench_function("tl", |b| {
            b.iter(|| tl::parse(html, tl::ParserOptions::default()).unwrap());
        });

        // scraper (html5ever)
        group.bench_function("scraper", |b| {
            b.iter(|| scraper::Html::parse_document(html));
        });

        // lol_html (streaming, no tree)
        group.bench_function("lol_html", |b| {
            let html_bytes = html.as_bytes();
            b.iter(|| {
                let mut rewriter = lol_html::HtmlRewriter::new(
                    lol_html::Settings {
                        element_content_handlers: vec![lol_html::element!("*", |_el| Ok(()))],
                        ..lol_html::Settings::new()
                    },
                    |_: &[u8]| {},
                );
                rewriter.write(html_bytes).unwrap();
                rewriter.end().unwrap();
            });
        });

        group.finish();
    }
}

// ---------------------------------------------------------------------------
// CSS selector comparison (fast-html-parser vs scraper)
// ---------------------------------------------------------------------------

fn bench_selector_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("select_100kb");
    let html = MEDIUM_HTML;

    // fast-html-parser
    {
        use fast_html_parser::Selectable;
        let doc = fast_html_parser::HtmlParser::parse(html).unwrap();

        group.bench_function("fast_html_parser/tag_p", |b| {
            b.iter(|| doc.select("p").unwrap());
        });
        group.bench_function("fast_html_parser/class", |b| {
            b.iter(|| doc.select(".content").unwrap());
        });
        group.bench_function("fast_html_parser/descendant", |b| {
            b.iter(|| doc.select("div p").unwrap());
        });
    }

    // scraper
    {
        let doc = scraper::Html::parse_document(html);
        let sel_p = scraper::Selector::parse("p").unwrap();
        let sel_class = scraper::Selector::parse(".content").unwrap();
        let sel_desc = scraper::Selector::parse("div p").unwrap();

        group.bench_function("scraper/tag_p", |b| {
            b.iter(|| doc.select(&sel_p).count());
        });
        group.bench_function("scraper/class", |b| {
            b.iter(|| doc.select(&sel_class).count());
        });
        group.bench_function("scraper/descendant", |b| {
            b.iter(|| doc.select(&sel_desc).count());
        });
    }

    // tl
    {
        let dom = tl::parse(html, tl::ParserOptions::default()).unwrap();
        let _parser = dom.parser();

        group.bench_function("tl/tag_p", |b| {
            b.iter(|| {
                dom.query_selector("p")
                    .map(|iter| iter.count())
                    .unwrap_or(0)
            });
        });
        group.bench_function("tl/class", |b| {
            b.iter(|| {
                dom.query_selector(".content")
                    .map(|iter| iter.count())
                    .unwrap_or(0)
            });
        });
        group.bench_function("tl/descendant", |b| {
            b.iter(|| {
                dom.query_selector("div p")
                    .map(|iter| iter.count())
                    .unwrap_or(0)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_parse_comparison, bench_selector_comparison);
criterion_main!(benches);
