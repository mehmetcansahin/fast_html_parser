//! Benchmarks for fhp-selector: select throughput and matching speed.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fhp_selector::Selectable;
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

    let mut group = c.benchmark_group("select");
    for (name, css) in &selectors {
        group.bench_with_input(BenchmarkId::from_parameter(name), *css, |b, css| {
            b.iter(|| {
                let sel = doc.select(css).unwrap();
                std::hint::black_box(sel.len());
            });
        });
    }
    group.finish();
}

fn bench_find_convenience(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let mut group = c.benchmark_group("find");

    group.bench_function("find_by_tag", |b| {
        b.iter(|| {
            let sel = doc.find_by_tag(fhp_core::tag::Tag::P);
            std::hint::black_box(sel.len());
        });
    });

    group.bench_function("find_by_class", |b| {
        b.iter(|| {
            let sel = doc.find_by_class("highlight");
            std::hint::black_box(sel.len());
        });
    });

    group.bench_function("find_by_id", |b| {
        b.iter(|| {
            let node = doc.find_by_id("main");
            std::hint::black_box(node.is_some());
        });
    });

    group.bench_function("document_index_build", |b| {
        b.iter(|| {
            let idx = fhp_selector::DocumentIndex::build(&doc);
            std::hint::black_box(&idx);
        });
    });

    group.finish();
}

fn bench_chaining(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    c.bench_function("chaining", |b| {
        b.iter(|| {
            let sel = doc.select("div").unwrap();
            let inner = sel.select("p").unwrap();
            std::hint::black_box(inner.len());
        });
    });
}

fn bench_compiled_selector(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let mut group = c.benchmark_group("compiled");

    // Measure string-based select (includes cache lookup / parse).
    group.bench_function("string_class", |b| {
        b.iter(|| {
            let sel = doc.select(".highlight").unwrap();
            std::hint::black_box(sel.len());
        });
    });

    // Measure pre-compiled select (zero parse overhead).
    let compiled = fhp_selector::CompiledSelector::new(".highlight").unwrap();
    group.bench_function("compiled_class", |b| {
        b.iter(|| {
            let sel = doc.select_compiled(&compiled).unwrap();
            std::hint::black_box(sel.len());
        });
    });

    // Compound selector comparison.
    group.bench_function("string_compound", |b| {
        b.iter(|| {
            let sel = doc.select("div > ul li a").unwrap();
            std::hint::black_box(sel.len());
        });
    });

    let compiled_compound = fhp_selector::CompiledSelector::new("div > ul li a").unwrap();
    group.bench_function("compiled_compound", |b| {
        b.iter(|| {
            let sel = doc.select_compiled(&compiled_compound).unwrap();
            std::hint::black_box(sel.len());
        });
    });

    group.finish();
}

fn bench_selector_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_selector");

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
            b.iter(|| {
                let list = fhp_selector::parser::parse_selector(css).unwrap();
                std::hint::black_box(&list);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_select,
    bench_find_convenience,
    bench_chaining,
    bench_compiled_selector,
    bench_selector_parse
);
criterion_main!(benches);
