#![cfg(all(feature = "css-selector", feature = "entity-decode"))]

#[path = "../benches/contract_support.rs"]
mod contract_support;

use contract_support::{
    FHP_BENCH_ORDERS, FhpBenchOrder, assert_selector_workload_counts, fast_html_parser_digest,
    fast_html_parser_owned_digest, fixture_contract, fixture_contracts, order_dom_parser_blocks,
    parse_fhp_bench_order, scraper_digest, selector_workload_contracts, selector_workload_counts,
    tl_digest, tl_owned_digest, validate_fixture_contract, validate_lol_html_passthrough,
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
fn fixture_canonical_digests_match_checked_in_contracts() {
    for &(id, html) in FIXTURES {
        let contract = fixture_contract(id);
        validate_fixture_contract(contract, html);
        validate_lol_html_passthrough(id, html);
    }
}

#[test]
fn parity_classification_matches_checked_in_digests() {
    for contract in fixture_contracts() {
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
    assert_eq!(parse_fhp_bench_order(None), FhpBenchOrder::FhpScraperTl);
    for &(name, order) in FHP_BENCH_ORDERS {
        assert_eq!(parse_fhp_bench_order(Some(name)), order);
    }

    let invalid = std::panic::catch_unwind(|| parse_fhp_bench_order(Some("random")));
    let message = invalid
        .expect_err("unknown benchmark order must panic")
        .downcast::<String>()
        .expect("panic should include a clear owned message");
    assert!(message.contains("fhp-scraper-tl"));
    assert!(message.contains("tl-scraper-fhp"));

    let fhp = vec!["fhp-build", "fhp-drop"];
    let scraper = vec!["scraper-build", "scraper-drop"];
    let tl = vec!["tl-build", "tl-drop"];
    assert_eq!(
        order_dom_parser_blocks(
            fhp.clone(),
            scraper.clone(),
            tl.clone(),
            FhpBenchOrder::FhpScraperTl,
        ),
        [fhp.clone(), scraper.clone(), tl.clone()].concat()
    );
    assert_eq!(
        order_dom_parser_blocks(
            fhp.clone(),
            scraper.clone(),
            tl.clone(),
            FhpBenchOrder::FhpTlScraper,
        ),
        [fhp.clone(), tl.clone(), scraper.clone()].concat()
    );
    assert_eq!(
        order_dom_parser_blocks(
            fhp.clone(),
            scraper.clone(),
            tl.clone(),
            FhpBenchOrder::TlScraperFhp,
        ),
        [tl, scraper, fhp].concat()
    );

    let pair_orders = FHP_BENCH_ORDERS
        .iter()
        .map(|(_, order)| order_dom_parser_blocks(vec!["fhp"], Vec::new(), vec!["tl"], *order))
        .collect::<Vec<_>>();
    assert_eq!(
        pair_orders.iter().filter(|order| order[0] == "fhp").count(),
        3
    );
    assert_eq!(
        pair_orders.iter().filter(|order| order[0] == "tl").count(),
        3
    );
}

#[test]
fn canonical_digest_covers_tag_attributes_text_and_child_order() {
    let baseline = fast_html_parser_digest("<main a='1' b='2'><i>x</i><b>y</b></main>");
    assert_eq!(
        baseline,
        fast_html_parser_digest("<main b='2' a='1'><i>x</i><b>y</b></main>"),
        "source attribute order must not affect the canonical DOM"
    );
    assert_ne!(
        baseline,
        fast_html_parser_digest("<section a='1' b='2'><i>x</i><b>y</b></section>"),
        "tag names must affect the canonical DOM"
    );
    assert_ne!(
        baseline,
        fast_html_parser_digest("<main a='changed' b='2'><i>x</i><b>y</b></main>"),
        "attribute values must affect the canonical DOM"
    );
    assert_ne!(
        baseline,
        fast_html_parser_digest("<main a='1' b='2'><i>z</i><b>y</b></main>"),
        "text must affect the canonical DOM"
    );
    assert_ne!(
        baseline,
        fast_html_parser_digest("<main a='1' b='2'><b>y</b><i>x</i></main>"),
        "child order must affect the canonical DOM"
    );

    assert_eq!(
        baseline,
        tl_digest("<main b='2' a='1'><i>x</i><b>y</b></main>"),
        "equivalent adapter DOMs must produce the same canonical digest"
    );
    assert_ne!(
        baseline,
        scraper_digest("<main a='1' b='2'><i>x</i><b>y</b></main>"),
        "scraper's synthetic html/head/body wrappers are part of its DOM contract"
    );
}

#[test]
#[ignore = "maintenance helper: emits values used to refresh benchmarks/contracts.json"]
fn dump_canonical_fixture_contracts() {
    for &(id, html) in FIXTURES {
        println!("{id}");
        for (implementation, digest) in [
            ("fast_html_parser", fast_html_parser_digest(html)),
            (
                "fast_html_parser_owned",
                fast_html_parser_owned_digest(html),
            ),
            ("scraper", scraper_digest(html)),
            ("tl", tl_digest(html)),
            ("tl_owned", tl_owned_digest(html)),
        ] {
            println!("  {implementation}: {digest:?}");
        }
    }
}

#[test]
fn selector_workloads_match_checked_in_counts_and_parity() {
    for contract in selector_workload_contracts() {
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
