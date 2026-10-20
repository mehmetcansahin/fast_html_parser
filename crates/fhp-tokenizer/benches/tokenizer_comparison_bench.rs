//! Contract-checked comparisons against popular Rust HTML tokenizers.
//!
//! Every fixture registers one `semantic_reference` group with all
//! implementations as absolute estimates, labelled by work class:
//! `full_token`, `lazy_rewriter` (lol_html, which does less work) and
//! `nonspec_dom` (tl, which is not WHATWG-compliant). `contract_equal` groups
//! pair an fhp entry point with each competitor that has the same API shape
//! and produced the same canonical token stream; only those form ratios.
//! See `tokenizer_contract_support.rs` for the contract.

mod tokenizer_contract_support;

use std::time::Duration;

use criterion::{
    BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use tokenizer_contract_support::{
    FIXTURES, Implementation, bench_order, contract_equal_sets, fixture_contract,
    order_implementations, validate_fixture_contract,
};

const SUITE: &str = "comparison/fhp-tokenizer/tokenizer_comparison_bench";

fn validate_tokenizer_fixtures(_c: &mut Criterion) {
    let _ = bench_order();
    for fixture in FIXTURES {
        validate_fixture_contract(fixture_contract(fixture.contract_id), fixture.html);
    }
}

fn bench_tokenizer_comparison(c: &mut Criterion) {
    let order = bench_order();
    for fixture in FIXTURES {
        let html = fixture.html;
        let prefix = format!("{SUITE}/{}/tokenize", fixture.benchmark_id);

        let mut absolute = c.benchmark_group(format!("{prefix}/semantic_reference"));
        absolute.throughput(Throughput::Bytes(html.len() as u64));
        for implementation in order_implementations(&Implementation::ALL, order) {
            let count = implementation.count_fn();
            let id = BenchmarkId::new(implementation.work_class().namespace(), implementation.id());
            absolute.bench_function(id, |b| b.iter(|| count(std::hint::black_box(html))));
        }
        absolute.finish();

        for (shape, members) in contract_equal_sets(fixture_contract(fixture.contract_id)) {
            let mut direct =
                c.benchmark_group(format!("{prefix}/contract_equal/{}", shape.namespace()));
            direct.throughput(Throughput::Bytes(html.len() as u64));
            direct.sampling_mode(SamplingMode::Flat);
            direct.measurement_time(Duration::from_secs(10));
            for implementation in order_implementations(&members, order) {
                let count = implementation.count_fn();
                direct.bench_function(implementation.contract_equal_id(), |b| {
                    b.iter(|| count(std::hint::black_box(html)))
                });
            }
            direct.finish();
        }
    }
}

criterion_group!(
    benches,
    validate_tokenizer_fixtures,
    bench_tokenizer_comparison
);
criterion_main!(benches);
