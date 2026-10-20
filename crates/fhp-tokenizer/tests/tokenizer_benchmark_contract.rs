//! Checks the tokenizer comparison benchmark's contract without Criterion.
//!
//! Refresh the `tokenizer` section of `benchmarks/contracts.json` after an
//! intended behavior change with:
//!
//! ```text
//! cargo test -p fhp-tokenizer --test tokenizer_benchmark_contract -- --ignored --nocapture
//! ```

#![cfg(feature = "entity-decode")]

#[path = "../benches/tokenizer_contract_support.rs"]
mod tokenizer_contract_support;

use std::collections::BTreeSet;

use tokenizer_contract_support::{
    ApiShape, BENCH_ORDERS, FIXTURES, Implementation, Library, WorkClass, contract_equal_sets,
    fixture_contract, fixture_contracts, observed_fixture_json, order_implementations,
    parse_bench_order, validate_fixture_contract,
};

#[test]
fn fixture_token_streams_match_checked_in_contracts() {
    for fixture in FIXTURES {
        validate_fixture_contract(fixture_contract(fixture.contract_id), fixture.html);
    }
}

#[test]
fn contracts_cover_exactly_the_benchmark_fixtures() {
    let contracted = fixture_contracts()
        .iter()
        .map(|contract| contract.id)
        .collect::<BTreeSet<_>>();
    let benchmarked = FIXTURES
        .iter()
        .map(|fixture| fixture.contract_id)
        .collect::<BTreeSet<_>>();
    assert_eq!(contracted, benchmarked);
}

#[test]
fn contract_equal_groups_follow_checked_in_token_streams() {
    for shape in ApiShape::ALL {
        assert_eq!(shape.fhp().library(), Library::Fhp);
        for competitor in shape.competitors() {
            assert_eq!(competitor.work_class(), WorkClass::FullToken);
            assert_ne!(competitor.library(), Library::Fhp);
        }
    }

    for fixture in FIXTURES {
        let contract = fixture_contract(fixture.contract_id);
        for (shape, members) in contract_equal_sets(contract) {
            let reference = contract.expected(shape.fhp()).token_stream;
            assert!(reference.is_some());
            assert_eq!(members[0], shape.fhp());
            assert!(members.len() > 1, "a group needs a competitor");
            for competitor in shape.competitors() {
                assert_eq!(
                    members.contains(competitor),
                    contract.expected(*competitor).token_stream == reference,
                    "{} membership in {}/{} must follow its token stream",
                    competitor.id(),
                    fixture.contract_id,
                    shape.namespace()
                );
            }
            let names = members
                .iter()
                .map(|member| member.contract_equal_id())
                .collect::<Vec<_>>();
            assert_eq!(names.iter().filter(|name| **name == "fhp").count(), 1);
        }
    }
}

#[test]
fn benchmark_registration_order_contract_is_explicit() {
    assert_eq!(
        parse_bench_order(None),
        [Library::Fhp, Library::Html5gum, Library::Html5ever]
    );
    for &(name, order) in BENCH_ORDERS {
        assert_eq!(parse_bench_order(Some(name)), order);
    }
    let names = BENCH_ORDERS
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), 6);

    let invalid = std::panic::catch_unwind(|| parse_bench_order(Some("random")));
    let message = invalid
        .expect_err("unknown benchmark order must panic")
        .downcast::<String>()
        .expect("panic should include a clear owned message");
    assert!(message.contains("fhp-scraper-tl"));
    assert!(message.contains("tl-scraper-fhp"));

    // Every ordered pair of full-token libraries occurs in three permutations,
    // and the absolute-only references always follow them.
    for (first, second) in [
        (Library::Fhp, Library::Html5gum),
        (Library::Fhp, Library::Html5ever),
        (Library::Html5gum, Library::Html5ever),
    ] {
        let first_leads = BENCH_ORDERS
            .iter()
            .filter(|(_, order)| {
                let ordered = order_implementations(&Implementation::ALL, *order);
                let position = |library| {
                    ordered
                        .iter()
                        .position(|implementation| implementation.library() == library)
                        .unwrap()
                };
                position(first) < position(second)
            })
            .count();
        assert_eq!(
            first_leads, 3,
            "{first:?} must lead {second:?} in half the orders"
        );
    }
    for &(_, order) in BENCH_ORDERS {
        let ordered = order_implementations(&Implementation::ALL, order);
        assert_eq!(ordered.len(), Implementation::ALL.len());
        assert!(
            ordered[7..]
                .iter()
                .all(|implementation| implementation.work_class() != WorkClass::FullToken)
        );
    }
}

/// Prints the observed `tokenizer.fixtures` entries for `contracts.json`.
#[test]
#[ignore = "prints the observed contract for maintainers"]
fn print_observed_tokenizer_contract() {
    let entries = FIXTURES
        .iter()
        .map(observed_fixture_json)
        .collect::<Vec<_>>()
        .join(",\n");
    println!("{entries}");
}
