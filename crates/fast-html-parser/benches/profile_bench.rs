//! Profiling benchmark — breaks down parse cost into individual components.
//!
//! Measures: SIMD indexing, tokenization, tree building, source copy, entity overhead.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

const MEDIUM_HTML: &str = include_str!("../../../testdata/medium_100kb.html");

fn bench_cost_breakdown(c: &mut Criterion) {
    let html = MEDIUM_HTML;
    let html_bytes = html.as_bytes();

    let mut group = c.benchmark_group("cost_100kb");
    group.throughput(Throughput::Bytes(html.len() as u64));

    // 1) SIMD structural indexing only
    group.bench_function("01_simd_index", |b| {
        let indexer = fhp_tokenizer::structural::StructuralIndexer::new();
        b.iter(|| {
            let si = indexer.index(html_bytes);
            std::hint::black_box(&si);
        });
    });

    // 2) Full tokenize → Vec<Token> (old path)
    group.bench_function("02_tokenize_vec", |b| {
        b.iter(|| {
            let tokens = fhp_tokenizer::tokenize(html);
            std::hint::black_box(tokens.len());
        });
    });

    // 3) Fused tokenize_with → callback (no tree, just count)
    group.bench_function("03_tokenize_with_noop", |b| {
        b.iter(|| {
            let mut count = 0u64;
            fhp_tokenizer::tokenize_with(html, |_token| {
                count += 1;
            });
            std::hint::black_box(count);
        });
    });

    // 4) Full parse (tokenize_with + tree build + source copy)
    group.bench_function("04_full_parse", |b| {
        b.iter(|| {
            let doc = fhp_tree::parse(html).unwrap();
            std::hint::black_box(doc.node_count());
        });
    });

    // 5) Tree build only — no source copy (isolate tree build cost)
    group.bench_function("05_tree_build_no_source", |b| {
        b.iter(|| {
            let mut builder = fhp_tree::builder::TreeBuilder::with_capacity_hint(html.len());
            // Don't call set_source — skips 100KB memcpy
            fhp_tokenizer::tokenize_with(html, |token| {
                builder.process(&token);
            });
            let (arena, root) = builder.finish();
            std::hint::black_box(arena.len());
            std::hint::black_box(root);
        });
    });

    // 6) Source copy cost (isolated memcpy)
    group.bench_function("06_memcpy_100kb", |b| {
        b.iter(|| {
            let copy: Vec<u8> = html_bytes.to_vec();
            std::hint::black_box(copy.len());
        });
    });

    // 7) tl for reference
    group.bench_function("07_tl_parse", |b| {
        b.iter(|| {
            let dom = tl::parse(html, tl::ParserOptions::default()).unwrap();
            std::hint::black_box(dom.nodes().len());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_cost_breakdown);
criterion_main!(benches);
