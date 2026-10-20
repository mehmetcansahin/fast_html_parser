//! Tokenizer throughput benchmarks.
//!
//! Measures GB/s throughput for both the structural indexer (stage 1)
//! and full tokenization (stage 1 + stage 2) at various input sizes.

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fhp_tokenizer::extract::extract_tokens;
use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::structural::StructuralIndexer;
use fhp_tokenizer::{tokenize, tokenize_with};

/// Repository fixtures used by the streaming comparison.
const FIXTURES: &[(&str, &str)] = &[
    (
        "hackernews",
        include_str!("../../../testdata/hackernews.html"),
    ),
    (
        "medium_100kb",
        include_str!("../../../testdata/medium_100kb.html"),
    ),
    ("github", include_str!("../../../testdata/github.html")),
    (
        "stackoverflow",
        include_str!("../../../testdata/stackoverflow.html"),
    ),
    (
        "wikipedia",
        include_str!("../../../testdata/wikipedia.html"),
    ),
    (
        "large_5mb",
        include_str!("../../../testdata/large_5mb.html"),
    ),
];

/// Streaming chunk size: a typical network or file read buffer.
const STREAM_CHUNK: usize = 64 * 1024;

/// Split `input` into chunks of at most `size` bytes on character boundaries.
fn str_chunks(input: &str, size: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < input.len() {
        let mut end = (start + size).min(input.len());
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&input[start..end]);
        start = end;
    }
    chunks
}

/// Generate a realistic HTML body of approximately `target_bytes` length.
fn generate_html(target_bytes: usize) -> String {
    let row = r#"<tr class="row"><td class="id">42</td><td class="name">John &amp; Jane</td><td class="email"><a href="mailto:test@example.com">test@example.com</a></td></tr>"#;
    let header = "<!DOCTYPE html><html><head><title>Benchmark</title></head><body><table>\n";
    let footer = "</table></body></html>";

    let mut html = String::with_capacity(target_bytes + 256);
    html.push_str(header);
    while html.len() < target_bytes {
        html.push_str(row);
        html.push('\n');
    }
    html.push_str(footer);
    html
}

fn bench_structural_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fhp-tokenizer/tokenizer_bench/structural_index");

    for &size in &[1_024, 100_000, 5_000_000] {
        let html = generate_html(size);
        let bytes = html.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &bytes, |b, &input| {
            let indexer = StructuralIndexer::new();
            b.iter_batched(
                || input,
                |input| indexer.index(input),
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_extract_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fhp-tokenizer/tokenizer_bench/extract_tokens");

    for &size in &[1_024, 100_000, 5_000_000] {
        let html = generate_html(size);
        let bytes = html.as_bytes();
        let indexer = StructuralIndexer::new();
        let index = indexer.index(bytes);
        let text = std::str::from_utf8(bytes).expect("generated benchmark HTML must be UTF-8");
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &text, |b, &input| {
            b.iter_batched(
                || input,
                |input| extract_tokens(input, &index),
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_tokenize_e2e(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e");

    for &size in &[1_024, 100_000, 5_000_000] {
        let html = generate_html(size);
        group.throughput(Throughput::Bytes(html.len() as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &html, |b, input| {
            b.iter_batched(|| input.as_str(), tokenize, BatchSize::LargeInput);
        });
    }

    group.finish();
}

/// One-shot callback tokenization against both streaming entry points, all
/// fed the same fixture in 64 KiB chunks.
fn bench_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fhp-tokenizer/tokenizer_bench/streaming");
    group.sample_size(20);

    for &(name, html) in FIXTURES {
        let chunks = str_chunks(html, STREAM_CHUNK);
        group.throughput(Throughput::Bytes(html.len() as u64));

        group.bench_with_input(BenchmarkId::new("tokenize_with", name), html, |b, input| {
            b.iter(|| {
                let mut count = 0usize;
                tokenize_with(std::hint::black_box(input), |token| {
                    std::hint::black_box(&token);
                    count += 1;
                });
                count
            });
        });
        group.bench_with_input(BenchmarkId::new("feed", name), &chunks, |b, chunks| {
            b.iter(|| {
                let mut tokenizer = StreamTokenizer::new();
                let mut count = 0usize;
                for chunk in chunks {
                    let tokens = tokenizer.feed(std::hint::black_box(chunk.as_bytes()));
                    count += tokens.len();
                    drop(std::hint::black_box(tokens));
                }
                count + tokenizer.finish().len()
            });
        });
        group.bench_with_input(
            BenchmarkId::new("feed_str_with", name),
            &chunks,
            |b, chunks| {
                b.iter(|| {
                    let mut tokenizer = StreamTokenizer::new();
                    let mut count = 0usize;
                    for chunk in chunks {
                        tokenizer.feed_str_with(std::hint::black_box(chunk), |token| {
                            std::hint::black_box(token);
                            count += 1;
                        });
                    }
                    tokenizer.finish_with(|token| {
                        std::hint::black_box(token);
                        count += 1;
                    });
                    count
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_structural_index,
    bench_extract_tokens,
    bench_tokenize_e2e,
    bench_streaming,
);
criterion_main!(benches);
