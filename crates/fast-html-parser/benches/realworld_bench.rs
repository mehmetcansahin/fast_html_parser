//! Real-world HTML benchmark — actual pages from Wikipedia, GitHub, HN, StackOverflow.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn load_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{name}",
        env!("CARGO_MANIFEST_DIR")
            .trim_end_matches("crates/fast-html-parser")
            .trim_end_matches('/')
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

fn bench_realworld_parse(c: &mut Criterion) {
    let pages: Vec<(&str, String)> = vec![
        ("hackernews_34kb", load_testdata("hackernews.html")),
        ("github_301kb", load_testdata("github.html")),
        ("stackoverflow_415kb", load_testdata("stackoverflow.html")),
        ("wikipedia_590kb", load_testdata("wikipedia.html")),
    ];

    for (name, html) in &pages {
        let mut group = c.benchmark_group(format!("realworld/{name}"));
        group.throughput(Throughput::Bytes(html.len() as u64));

        // fast-html-parser
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

        // lol_html (streaming)
        group.bench_function("lol_html", |b| {
            let bytes = html.as_bytes();
            b.iter(|| {
                let mut rewriter = lol_html::HtmlRewriter::new(
                    lol_html::Settings {
                        element_content_handlers: vec![lol_html::element!("*", |_el| Ok(()))],
                        ..lol_html::Settings::new()
                    },
                    |_: &[u8]| {},
                );
                rewriter.write(bytes).unwrap();
                rewriter.end().unwrap();
            });
        });

        group.finish();
    }
}

fn bench_realworld_parse_owned(c: &mut Criterion) {
    let pages: Vec<(&str, String)> = vec![
        ("hackernews_34kb", load_testdata("hackernews.html")),
        ("wikipedia_590kb", load_testdata("wikipedia.html")),
    ];

    for (name, html) in &pages {
        let mut group = c.benchmark_group(format!("realworld_owned/{name}"));
        group.throughput(criterion::Throughput::Bytes(html.len() as u64));

        group.bench_function("parse", |b| {
            b.iter(|| fast_html_parser::HtmlParser::parse(html).unwrap());
        });

        group.bench_function("parse_owned", |b| {
            b.iter(|| fast_html_parser::HtmlParser::parse_owned(html.clone()).unwrap());
        });

        group.finish();
    }
}

fn bench_realworld_select(c: &mut Criterion) {
    let wikipedia = load_testdata("wikipedia.html");

    let mut group = c.benchmark_group("realworld_select/wikipedia");

    // fast-html-parser — string selector
    #[cfg(feature = "css-selector")]
    {
        use fast_html_parser::Selectable;
        let doc = fast_html_parser::HtmlParser::parse(&wikipedia).unwrap();

        group.bench_function("fhp/a[href]", |b| {
            b.iter(|| doc.select("a[href]").unwrap());
        });
        group.bench_function("fhp/div.mw-body", |b| {
            b.iter(|| doc.select("div.mw-body").unwrap());
        });
        group.bench_function("fhp/table td", |b| {
            b.iter(|| doc.select("table td").unwrap());
        });
    }

    // fast-html-parser — compiled selector
    #[cfg(feature = "css-selector")]
    {
        use fast_html_parser::{CompiledSelector, Selectable};
        let doc = fast_html_parser::HtmlParser::parse(&wikipedia).unwrap();
        let sel_a = CompiledSelector::new("a[href]").unwrap();
        let sel_div = CompiledSelector::new("div.mw-body").unwrap();
        let sel_td = CompiledSelector::new("table td").unwrap();

        group.bench_function("fhp_compiled/a[href]", |b| {
            b.iter(|| doc.select_compiled(&sel_a).unwrap());
        });
        group.bench_function("fhp_compiled/div.mw-body", |b| {
            b.iter(|| doc.select_compiled(&sel_div).unwrap());
        });
        group.bench_function("fhp_compiled/table td", |b| {
            b.iter(|| doc.select_compiled(&sel_td).unwrap());
        });
    }

    // scraper
    {
        let doc = scraper::Html::parse_document(&wikipedia);
        let sel_a = scraper::Selector::parse("a[href]").unwrap();
        let sel_div = scraper::Selector::parse("div.mw-body").unwrap();
        let sel_td = scraper::Selector::parse("table td").unwrap();

        group.bench_function("scraper/a[href]", |b| {
            b.iter(|| doc.select(&sel_a).count());
        });
        group.bench_function("scraper/div.mw-body", |b| {
            b.iter(|| doc.select(&sel_div).count());
        });
        group.bench_function("scraper/table td", |b| {
            b.iter(|| doc.select(&sel_td).count());
        });
    }

    // tl
    {
        let dom = tl::parse(&wikipedia, tl::ParserOptions::default()).unwrap();

        group.bench_function("tl/a[href]", |b| {
            b.iter(|| {
                dom.query_selector("a[href]")
                    .map(|iter| iter.count())
                    .unwrap_or(0)
            });
        });
        group.bench_function("tl/div.mw-body", |b| {
            b.iter(|| {
                dom.query_selector("div.mw-body")
                    .map(|iter| iter.count())
                    .unwrap_or(0)
            });
        });
        group.bench_function("tl/table td", |b| {
            b.iter(|| {
                dom.query_selector("table td")
                    .map(|iter| iter.count())
                    .unwrap_or(0)
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_realworld_parse,
    bench_realworld_parse_owned,
    bench_realworld_select
);
criterion_main!(benches);
