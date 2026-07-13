//! Contract-checked synthetic comparisons against popular Rust HTML parsers.

mod benchmark_support;
mod contract_support;

use benchmark_support::register_parse_groups;
#[cfg(feature = "css-selector")]
use benchmark_support::register_selector_groups;
#[cfg(feature = "css-selector")]
use contract_support::validate_fixture_contract;
use contract_support::{fhp_bench_order, fixture_contract, validate_lol_html_passthrough};
use criterion::{Criterion, criterion_group, criterion_main};

const SMALL_HTML: &str = include_str!("../../../testdata/small_1kb.html");
const MEDIUM_HTML: &str = include_str!("../../../testdata/medium_100kb.html");
const LARGE_HTML: &str = include_str!("../../../testdata/large_5mb.html");

const FIXTURES: &[(&str, &str, &str)] = &[
    ("1kb", "synthetic_1kb", SMALL_HTML),
    ("100kb", "synthetic_100kb", MEDIUM_HTML),
    ("5mb", "synthetic_5mb", LARGE_HTML),
];

fn validate_comparison_fixtures(_c: &mut Criterion) {
    let _ = fhp_bench_order();
    for &(_, contract_id, html) in FIXTURES {
        #[cfg(feature = "css-selector")]
        validate_fixture_contract(fixture_contract(contract_id), html);
        validate_lol_html_passthrough(contract_id, html);
    }
}

fn bench_parse_comparison(c: &mut Criterion) {
    let order = fhp_bench_order();
    for &(benchmark_id, contract_id, html) in FIXTURES {
        register_parse_groups(
            c,
            "comparison/fast-html-parser/comparison_bench/synthetic",
            benchmark_id,
            html,
            fixture_contract(contract_id),
            order,
        );
    }
}

#[cfg(feature = "css-selector")]
fn bench_selector_comparison(c: &mut Criterion) {
    const SELECTORS: &[(&str, &str)] = &[
        ("tag_p", "p"),
        ("class_card", ".card"),
        ("descendant_div_p", "div p"),
    ];

    register_selector_groups(
        c,
        "comparison/fast-html-parser/comparison_bench/synthetic",
        "100kb",
        MEDIUM_HTML,
        "synthetic_100kb",
        SELECTORS,
        fhp_bench_order(),
    );
}

#[cfg(not(feature = "css-selector"))]
fn bench_selector_comparison(_c: &mut Criterion) {}

criterion_group!(
    benches,
    validate_comparison_fixtures,
    bench_parse_comparison,
    bench_selector_comparison
);
criterion_main!(benches);
