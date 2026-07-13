//! Benchmarks for fhp-selector: select throughput and matching speed.

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use fhp_selector::{CompiledSelector, Selectable};
use fhp_tree::parse;

fn load_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{name}",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("crates/fhp-selector")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

fn bench_select(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let selectors = [
        ("tag", "p"),
        ("class", ".highlight"),
        ("id", "#main"),
        ("descendant", "div p"),
        ("child", "div > p"),
        ("compound", "p.highlight"),
        ("attr_exists", "[href]"),
        ("attr_equals", "[class=\"highlight\"]"),
        ("first_child", "p:first-child"),
        ("nth_child", "li:nth-child(odd)"),
        ("not", "p:not(.highlight)"),
        ("complex", "div > ul li a"),
    ];

    let compiled: Vec<_> = selectors
        .iter()
        .map(|(name, css)| (*name, CompiledSelector::new(css).unwrap()))
        .collect();

    let mut group = c.benchmark_group("regression/fhp-selector/selector_bench/evaluate");
    for (name, selector) in &compiled {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            selector,
            |b, selector| {
                b.iter_batched(
                    || (),
                    |_| doc.select_compiled(selector).unwrap(),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_find_convenience(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let mut group = c.benchmark_group("regression/fhp-selector/selector_bench/find");

    group.bench_function("find_by_tag", |b| {
        b.iter_batched(
            || (),
            |_| doc.find_by_tag(fhp_core::tag::Tag::P),
            BatchSize::LargeInput,
        );
    });

    group.bench_function("find_by_class", |b| {
        b.iter_batched(
            || (),
            |_| doc.find_by_class("highlight"),
            BatchSize::LargeInput,
        );
    });

    group.bench_function("find_by_id", |b| {
        b.iter(|| {
            let node = doc.find_by_id("main");
            std::hint::black_box(node.is_some());
        });
    });

    group.bench_function("document_index_build", |b| {
        b.iter_batched(
            || (),
            |_| fhp_selector::DocumentIndex::build(&doc),
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_chaining(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let div = CompiledSelector::new("div").unwrap();
    let p = CompiledSelector::new("p").unwrap();

    let mut group = c.benchmark_group("regression/fhp-selector/selector_bench/chaining");
    group.bench_function("compiled", |b| {
        b.iter_batched(
            || (),
            |_| {
                let selection = doc.select_compiled(&div).unwrap();
                selection.select_compiled(&p).unwrap()
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

fn bench_string_convenience(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let mut group = c.benchmark_group("diagnostic/fhp-selector/selector_bench/string_convenience");

    // The selector cache is deliberately warm here. Compile cost has its own
    // benchmark and evaluation uses CompiledSelector directly.
    doc.select(".highlight").unwrap();
    group.bench_function("string_class", |b| {
        b.iter_batched(
            || (),
            |_| doc.select(".highlight").unwrap(),
            BatchSize::LargeInput,
        );
    });

    doc.select("div > ul li a").unwrap();
    group.bench_function("string_compound", |b| {
        b.iter_batched(
            || (),
            |_| doc.select("div > ul li a").unwrap(),
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_selector_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fhp-selector/selector_bench/compile");

    let selectors = [
        ("tag", "p"),
        ("class", ".highlight"),
        ("id", "#main"),
        ("descendant", "div p"),
        ("compound", "p.highlight#main"),
        ("complex", "div > ul li a[href]"),
        ("nth_child", "li:nth-child(2n+1)"),
        ("not", "p:not(.hidden)"),
    ];

    for (name, css) in &selectors {
        group.bench_with_input(BenchmarkId::from_parameter(name), *css, |b, css| {
            b.iter_batched(
                || css,
                |css| CompiledSelector::new(css).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_select,
    bench_find_convenience,
    bench_chaining,
    bench_string_convenience,
    bench_selector_parse
);
criterion_main!(benches);
