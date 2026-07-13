//! Benchmarks for XPath evaluation throughput.

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use fhp_selector::Selectable;
use fhp_selector::xpath::{eval::evaluate, parser::parse_xpath};
use fhp_tree::parse;

fn load_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{name}",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("crates/fhp-selector")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

const EXPRESSIONS: [(&str, &str); 7] = [
    ("descendant_p", "//p"),
    ("descendant_attr", "//a[@href]"),
    ("absolute_path", "/html/body/div"),
    ("wildcard_all", "//*"),
    ("contains", "//div[contains(@class, 'content')]"),
    ("text_extract", "//p/text()"),
    ("position", "//li[1]"),
];

fn bench_xpath_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fhp-selector/xpath_bench/compile");

    for (name, xpath) in EXPRESSIONS {
        group.bench_with_input(BenchmarkId::from_parameter(name), xpath, |b, xpath| {
            b.iter_batched(
                || xpath,
                |xpath| parse_xpath(xpath).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_xpath_evaluate(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();
    let compiled: Vec<_> = EXPRESSIONS
        .iter()
        .map(|(name, xpath)| (*name, parse_xpath(xpath).unwrap()))
        .collect();

    let mut group = c.benchmark_group("regression/fhp-selector/xpath_bench/evaluate");
    for (name, expression) in &compiled {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            expression,
            |b, expression| {
                b.iter_batched(
                    || (),
                    |_| evaluate(expression, doc.arena(), doc.root_id()),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_xpath_string_convenience(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = parse(&medium).unwrap();

    let mut group = c.benchmark_group("diagnostic/fhp-selector/xpath_bench/string_convenience");
    for (name, xpath) in EXPRESSIONS {
        group.bench_with_input(BenchmarkId::from_parameter(name), xpath, |b, xpath| {
            b.iter_batched(|| (), |_| doc.xpath(xpath).unwrap(), BatchSize::LargeInput);
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_xpath_compile,
    bench_xpath_evaluate,
    bench_xpath_string_convenience,
);
criterion_main!(benches);
