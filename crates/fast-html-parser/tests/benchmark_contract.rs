#![cfg(all(feature = "css-selector", feature = "entity-decode"))]

#[path = "../benches/contract_support.rs"]
mod contract_support;

use contract_support::{
    FIXTURE_CONTRACTS, FhpBenchOrder, SELECTOR_WORKLOAD_CONTRACTS, assert_selector_workload_counts,
    fixture_contract, order_fhp_blocks, parse_fhp_bench_order, selector_workload_counts,
    validate_fixture_contract, validate_lol_html_passthrough,
};

const SMALL_HTML: &str = include_str!("../../../testdata/small_1kb.html");
const MEDIUM_HTML: &str = include_str!("../../../testdata/medium_100kb.html");
const LARGE_HTML: &str = include_str!("../../../testdata/large_5mb.html");
const HACKERNEWS_HTML: &str = include_str!("../../../testdata/hackernews.html");
const GITHUB_HTML: &str = include_str!("../../../testdata/github.html");
const STACKOVERFLOW_HTML: &str = include_str!("../../../testdata/stackoverflow.html");
const WIKIPEDIA_HTML: &str = include_str!("../../../testdata/wikipedia.html");

const FIXTURES: &[(&str, &str)] = &[
    ("synthetic_1kb", SMALL_HTML),
    ("synthetic_100kb", MEDIUM_HTML),
    ("synthetic_5mb", LARGE_HTML),
    ("realworld_hackernews_34kb", HACKERNEWS_HTML),
    ("realworld_github_301kb", GITHUB_HTML),
    ("realworld_stackoverflow_415kb", STACKOVERFLOW_HTML),
    ("realworld_wikipedia_590kb", WIKIPEDIA_HTML),
];

#[test]
fn fixture_observables_match_checked_in_contracts() {
    for &(id, html) in FIXTURES {
        let contract = fixture_contract(id);
        validate_fixture_contract(contract, html);
        validate_lol_html_passthrough(id, html);
    }
}

#[test]
fn parity_classification_matches_checked_in_signatures() {
    for contract in FIXTURE_CONTRACTS {
        assert_eq!(
            contract.parity.fast_html_parser_scraper,
            contract.fast_html_parser == contract.scraper,
            "stale fast-html-parser/scraper parity for {}",
            contract.id
        );
        assert_eq!(
            contract.parity.fast_html_parser_tl,
            contract.fast_html_parser == contract.tl,
            "stale fast-html-parser/tl parity for {}",
            contract.id
        );
        assert_eq!(
            contract.parity.scraper_tl,
            contract.scraper == contract.tl,
            "stale scraper/tl parity for {}",
            contract.id
        );
    }
}

#[test]
fn benchmark_registration_order_contract_is_explicit() {
    assert_eq!(parse_fhp_bench_order(None), FhpBenchOrder::Middle);
    assert_eq!(
        parse_fhp_bench_order(Some("fhp-first")),
        FhpBenchOrder::First
    );
    assert_eq!(
        parse_fhp_bench_order(Some("fhp-middle")),
        FhpBenchOrder::Middle
    );
    assert_eq!(parse_fhp_bench_order(Some("fhp-last")), FhpBenchOrder::Last);

    let fhp = vec![0, 1];
    let others = vec![vec![10, 11], vec![20, 21], vec![30]];
    assert_eq!(
        order_fhp_blocks(fhp.clone(), others.clone(), FhpBenchOrder::First),
        vec![0, 1, 10, 11, 20, 21, 30]
    );
    assert_eq!(
        order_fhp_blocks(fhp.clone(), others.clone(), FhpBenchOrder::Middle),
        vec![10, 11, 0, 1, 20, 21, 30]
    );
    assert_eq!(
        order_fhp_blocks(fhp.clone(), others.clone(), FhpBenchOrder::Last),
        vec![10, 11, 20, 21, 30, 0, 1]
    );

    let only_other = vec![vec![10, 11]];
    assert_eq!(
        order_fhp_blocks(fhp.clone(), only_other.clone(), FhpBenchOrder::First),
        vec![0, 1, 10, 11]
    );
    assert_eq!(
        order_fhp_blocks(fhp.clone(), only_other.clone(), FhpBenchOrder::Middle),
        vec![0, 1, 10, 11],
        "two implementations have no unique middle; middle intentionally matches first"
    );
    assert_eq!(
        order_fhp_blocks(fhp, only_other, FhpBenchOrder::Last),
        vec![10, 11, 0, 1]
    );

    let invalid = std::panic::catch_unwind(|| parse_fhp_bench_order(Some("random")));
    let message = invalid
        .expect_err("unknown benchmark order must panic")
        .downcast::<String>()
        .expect("panic should include a clear owned message");
    assert!(message.contains("fhp-first, fhp-middle, fhp-last"));
}

#[test]
fn selector_workloads_match_checked_in_counts_and_parity() {
    for contract in SELECTOR_WORKLOAD_CONTRACTS {
        let html = match contract.fixture_id {
            "synthetic_100kb" => MEDIUM_HTML,
            "realworld_wikipedia_590kb" => WIKIPEDIA_HTML,
            fixture => panic!("selector contract references unknown fixture {fixture}"),
        };
        let actual = selector_workload_counts(html, contract.css);
        assert_selector_workload_counts(contract, &actual);

        assert_eq!(
            contract.parity.fast_html_parser_scraper,
            contract.fast_html_parser_count == contract.scraper_count,
            "stale selector FHP/scraper parity for {}/{}",
            contract.fixture_id,
            contract.id
        );
        assert_eq!(
            contract.parity.fast_html_parser_tl,
            contract.fast_html_parser_count == contract.tl_count,
            "stale selector FHP/tl parity for {}/{}",
            contract.fixture_id,
            contract.id
        );
        assert_eq!(
            contract.parity.scraper_tl,
            contract.scraper_count == contract.tl_count,
            "stale selector scraper/tl parity for {}/{}",
            contract.fixture_id,
            contract.id
        );
    }
}
