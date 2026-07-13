//! Profiling benchmark — breaks down parse cost into individual components.
//!
//! Measures: SIMD indexing, tokenization, tree building, source copy, entity overhead.

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

const MEDIUM_HTML: &str = include_str!("../../../testdata/medium_100kb.html");

fn bench_cost_breakdown(c: &mut Criterion) {
    let html = MEDIUM_HTML;
    let html_bytes = html.as_bytes();
    let pretokenized = fhp_tokenizer::tokenize(html);

    let mut group = c.benchmark_group("diagnostic/fast-html-parser/profile_bench/cost_100kb");
    group.throughput(Throughput::Bytes(html.len() as u64));

    // 1) SIMD structural indexing only
    group.bench_function("01_simd_index", |b| {
        let indexer = fhp_tokenizer::structural::StructuralIndexer::new();
        b.iter_batched(
            || html_bytes,
            |input| indexer.index(input),
            BatchSize::LargeInput,
        );
    });

    // 2) Full tokenize → Vec<Token> (old path)
    group.bench_function("02_tokenize_vec", |b| {
        b.iter_batched(|| html, fhp_tokenizer::tokenize, BatchSize::LargeInput);
    });

    // 3) Fused tokenize_with → callback (no tree, just count)
    group.bench_function("03_tokenize_with_noop", |b| {
        b.iter(|| {
            let mut count = 0u64;
            fhp_tokenizer::tokenize_with(std::hint::black_box(html), |_token| {
                count += 1;
            });
            std::hint::black_box(count);
        });
    });

    // 4) Full parse (tokenize_with + tree build + source copy)
    group.bench_function("04_full_parse", |b| {
        b.iter_batched(
            || html,
            |input| fhp_tree::parse(input).unwrap(),
            BatchSize::LargeInput,
        );
    });

    // 5) Tree construction from an already-tokenized input. Tokenization and
    // source-buffer setup are deliberately outside this measurement.
    group.bench_function("05_tree_build_from_pretokenized", |b| {
        b.iter_batched(
            || (),
            |_| {
                let mut builder = fhp_tree::builder::TreeBuilder::with_capacity_hint(html.len());
                for token in &pretokenized {
                    builder.process(token);
                }
                builder.finish()
            },
            BatchSize::LargeInput,
        );
    });

    // 6) Source copy cost (isolated memcpy)
    group.bench_function("06_memcpy_100kb", |b| {
        b.iter_batched(|| html_bytes, <[u8]>::to_vec, BatchSize::LargeInput);
    });

    // 7) tl for reference
    group.bench_function("07_tl_parse", |b| {
        b.iter_batched(
            || html,
            |input| tl::parse(input, tl::ParserOptions::default()).unwrap(),
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_entity_decode(c: &mut Criterion) {
    // Text with no entities (fast path — borrowed, zero alloc).
    let no_entities = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(100);
    // Text with sparse entities (~5% of content has entities).
    let sparse_entities = "Hello &amp; world &lt;div&gt; test &quot;value&quot; end. ".repeat(100);
    // Text with dense entities (every token has entities).
    let dense_entities = "&lt;div class=&quot;foo&quot;&gt;&amp;bar&lt;/div&gt;".repeat(100);

    let mut group = c.benchmark_group("regression/fast-html-parser/profile_bench/entity_decode");

    group.throughput(criterion::Throughput::Bytes(no_entities.len() as u64));
    group.bench_function("no_entities", |b| {
        b.iter_batched(
            || no_entities.as_str(),
            fhp_tokenizer::entity::decode_entities,
            BatchSize::LargeInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(sparse_entities.len() as u64));
    group.bench_function("sparse_entities", |b| {
        b.iter_batched(
            || sparse_entities.as_str(),
            fhp_tokenizer::entity::decode_entities,
            BatchSize::LargeInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(dense_entities.len() as u64));
    group.bench_function("dense_entities", |b| {
        b.iter_batched(
            || dense_entities.as_str(),
            fhp_tokenizer::entity::decode_entities,
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_cost_breakdown, bench_entity_decode);
criterion_main!(benches);
