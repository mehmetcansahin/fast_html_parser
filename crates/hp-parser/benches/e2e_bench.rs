//! End-to-end benchmarks for the HTML parser.
//!
//! Measures full-pipeline throughput: tokenize + tree build + (optionally) selector.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use hp_parser::HtmlParser;
#[cfg(feature = "css-selector")]
use hp_parser::Selectable;
use hp_parser::streaming::parse_stream;

const SMALL_HTML: &str = include_str!("../testdata/small_1kb.html");
const MEDIUM_HTML: &str = include_str!("../testdata/medium_100kb.html");

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    for (name, html) in [("1kb", SMALL_HTML), ("100kb", MEDIUM_HTML)] {
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(BenchmarkId::new("one_shot", name), html, |b, input| {
            b.iter(|| HtmlParser::parse(input).unwrap());
        });
    }

    group.finish();
}

fn bench_parse_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_bytes");

    for (name, html) in [("1kb", SMALL_HTML), ("100kb", MEDIUM_HTML)] {
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("auto_encoding", name),
            html.as_bytes(),
            |b, input| {
                b.iter(|| HtmlParser::parse_bytes(input).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming");

    let chunk_sizes = [64, 1024, 8192, 65536];

    for &chunk_size in &chunk_sizes {
        let html = MEDIUM_HTML.as_bytes();
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("100kb", format!("chunk_{chunk_size}")),
            &chunk_size,
            |b, &cs| {
                b.iter(|| parse_stream(html.chunks(cs)).unwrap());
            },
        );
    }

    group.finish();
}

#[cfg(feature = "css-selector")]
fn bench_select(c: &mut Criterion) {
    let mut group = c.benchmark_group("select");

    let doc = HtmlParser::parse(MEDIUM_HTML).unwrap();

    group.bench_function("tag_p", |b| {
        b.iter(|| doc.select("p").unwrap());
    });

    group.bench_function("class", |b| {
        b.iter(|| doc.select(".content").unwrap());
    });

    group.bench_function("descendant", |b| {
        b.iter(|| doc.select("div p").unwrap());
    });

    group.bench_function("complex", |b| {
        b.iter(|| doc.select("div.article > h2 + p").unwrap());
    });

    group.finish();
}

#[cfg(not(feature = "css-selector"))]
fn bench_select(_c: &mut Criterion) {}

fn bench_tree_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("traversal");

    let doc = HtmlParser::parse(MEDIUM_HTML).unwrap();

    group.bench_function("text_content", |b| {
        b.iter(|| doc.root().text_content());
    });

    group.bench_function("depth_first", |b| {
        b.iter(|| {
            let mut count = 0u64;
            for _ in doc.root().descendants() {
                count += 1;
            }
            count
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_parse_bytes,
    bench_streaming,
    bench_select,
    bench_tree_traversal,
);
criterion_main!(benches);
