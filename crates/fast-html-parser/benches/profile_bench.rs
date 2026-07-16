//! Profiling benchmark — breaks down parse cost into individual components.
//!
//! Measures: SIMD indexing, tokenization, tree building, source copy, entity overhead.

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use fhp_core::tag::Tag;
use fhp_tree::arena::Arena;

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
                    builder.process(token).unwrap();
                }
                builder.finish().unwrap()
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_calibration(c: &mut Criterion) {
    let html = MEDIUM_HTML;
    let html_bytes = html.as_bytes();
    let mut group = c.benchmark_group("diagnostic/calibration");
    group.throughput(Throughput::Bytes(html.len() as u64));

    group.bench_function("memcpy_100kb", |b| {
        b.iter_batched(|| html_bytes, <[u8]>::to_vec, BatchSize::LargeInput);
    });
    group.bench_function("tl_parse_100kb", |b| {
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

fn unique_attributes(count: usize) -> (String, String) {
    let mut raw = String::new();
    for index in 0..count {
        raw.push_str(&format!(" data-{index}={index}"));
    }
    let html = format!("<div{raw}></div>");
    (html, raw)
}

fn duplicate_attributes(count: usize) -> (String, String) {
    let mut raw = String::new();
    for index in 0..count {
        let name = if index % 2 == 0 {
            "data-value"
        } else {
            "DATA-VALUE"
        };
        raw.push_str(&format!(" {name}={index}"));
    }
    let html = format!("<div{raw}></div>");
    (html, raw)
}

fn bench_attribute_dedup(c: &mut Criterion) {
    let unique = [
        ("unique_1", unique_attributes(1)),
        ("unique_3", unique_attributes(3)),
        ("unique_16", unique_attributes(16)),
        ("unique_64", unique_attributes(64)),
        ("duplicate_64", duplicate_attributes(64)),
    ];

    let mut tokenizer =
        c.benchmark_group("regression/fast-html-parser/profile_bench/attributes/tokenizer");
    for (name, (html, _)) in &unique {
        tokenizer.throughput(Throughput::Bytes(html.len() as u64));
        tokenizer.bench_function(*name, |b| {
            b.iter_batched(
                || html.as_str(),
                fhp_tokenizer::tokenize,
                BatchSize::LargeInput,
            );
        });
    }
    tokenizer.finish();

    let mut arena =
        c.benchmark_group("regression/fast-html-parser/profile_bench/attributes/arena_raw");
    for (name, (_, raw)) in &unique {
        arena.throughput(Throughput::Bytes(raw.len() as u64));
        arena.bench_function(*name, |b| {
            b.iter_batched(
                Arena::new,
                |mut arena| {
                    let node = arena.new_element(Tag::Div, 0);
                    arena.set_attrs_from_raw(node, raw);
                    std::hint::black_box(arena);
                },
                BatchSize::LargeInput,
            );
        });
    }
    arena.finish();
}

criterion_group!(
    benches,
    bench_cost_breakdown,
    bench_calibration,
    bench_entity_decode,
    bench_attribute_dedup
);
criterion_main!(benches);
