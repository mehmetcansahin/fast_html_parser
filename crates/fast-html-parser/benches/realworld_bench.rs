//! Contract-checked comparisons on pinned real-world HTML snapshots.

mod benchmark_support;
mod contract_support;

use benchmark_support::register_parse_groups;
#[cfg(feature = "css-selector")]
use benchmark_support::register_selector_groups;
#[cfg(feature = "css-selector")]
use contract_support::validate_fixture_contract;
use contract_support::{fhp_bench_order, fixture_contract, validate_lol_html_passthrough};
use criterion::{Criterion, criterion_group, criterion_main};

fn load_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{name}",
        env!("CARGO_MANIFEST_DIR")
            .trim_end_matches("crates/fast-html-parser")
            .trim_end_matches('/')
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

fn load_pages() -> Vec<(&'static str, &'static str, String)> {
    vec![
        (
            "hackernews_34kb",
            "realworld_hackernews_34kb",
            load_testdata("hackernews.html"),
        ),
        (
            "github_301kb",
            "realworld_github_301kb",
            load_testdata("github.html"),
        ),
        (
            "stackoverflow_415kb",
            "realworld_stackoverflow_415kb",
            load_testdata("stackoverflow.html"),
        ),
        (
            "wikipedia_590kb",
            "realworld_wikipedia_590kb",
            load_testdata("wikipedia.html"),
        ),
    ]
}

fn validate_realworld_fixtures(_c: &mut Criterion) {
    let _ = fhp_bench_order();
    for (_, contract_id, html) in load_pages() {
        #[cfg(feature = "css-selector")]
        validate_fixture_contract(fixture_contract(contract_id), &html);
        validate_lol_html_passthrough(contract_id, &html);
    }
}

fn bench_realworld_parse(c: &mut Criterion) {
    let order = fhp_bench_order();
    for (benchmark_id, contract_id, html) in load_pages() {
        register_parse_groups(
            c,
            "comparison/fast-html-parser/realworld_bench/realworld",
            benchmark_id,
            &html,
            fixture_contract(contract_id),
            order,
        );
    }
}

#[cfg(feature = "css-selector")]
fn bench_realworld_select(c: &mut Criterion) {
    const SELECTORS: &[(&str, &str)] = &[
        ("link_with_href", "a[href]"),
        ("class_mw_body", "main.mw-body"),
        ("descendant_table_td", "table td"),
    ];

    let wikipedia = load_testdata("wikipedia.html");
    register_selector_groups(
        c,
        "comparison/fast-html-parser/realworld_bench/realworld",
        "wikipedia_590kb",
        &wikipedia,
        "realworld_wikipedia_590kb",
        SELECTORS,
        fhp_bench_order(),
    );
}

#[cfg(not(feature = "css-selector"))]
fn bench_realworld_select(_c: &mut Criterion) {}

criterion_group!(
    benches,
    validate_realworld_fixtures,
    bench_realworld_parse,
    bench_realworld_select
);
criterion_main!(benches);
