//! Benchmarks for XPath evaluation throughput.

use criterion::{Criterion, criterion_group, criterion_main};
use fhp_selector::Selectable;
use fhp_tree::parse;

fn load_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{name}",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("crates/fhp-selector")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

fn bench_xpath(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let mut group = c.benchmark_group("xpath");

    // Descendant: //tag
    group.bench_function("descendant_p", |b| {
        b.iter(|| {
            let r = doc.xpath("//p").unwrap();
            std::hint::black_box(&r);
        });
    });

    // Descendant with attribute predicate: //tag[@attr='value']
    group.bench_function("descendant_attr", |b| {
        b.iter(|| {
            let r = doc.xpath("//a[@href]").unwrap();
            std::hint::black_box(&r);
        });
    });

    // Absolute path: /html/body/div
    group.bench_function("absolute_path", |b| {
        b.iter(|| {
            let r = doc.xpath("/html/body/div").unwrap();
            std::hint::black_box(&r);
        });
    });

    // Wildcard: //*
    group.bench_function("wildcard_all", |b| {
        b.iter(|| {
            let r = doc.xpath("//*").unwrap();
            std::hint::black_box(&r);
        });
    });

    // Contains function: //tag[contains(@attr, 'sub')]
    group.bench_function("contains", |b| {
        b.iter(|| {
            let r = doc.xpath("//div[contains(@class, 'content')]").unwrap();
            std::hint::black_box(&r);
        });
    });

    // Text extraction: //tag/text()
    group.bench_function("text_extract", |b| {
        b.iter(|| {
            let r = doc.xpath("//p/text()").unwrap();
            std::hint::black_box(&r);
        });
    });

    // Position predicate: //tag[1]
    group.bench_function("position", |b| {
        b.iter(|| {
            let r = doc.xpath("//li[1]").unwrap();
            std::hint::black_box(&r);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_xpath);
criterion_main!(benches);
