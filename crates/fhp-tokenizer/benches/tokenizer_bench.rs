//! Tokenizer throughput benchmarks.
//!
//! Measures GB/s throughput for both the structural indexer (stage 1)
//! and full tokenization (stage 1 + stage 2) at various input sizes.

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fhp_tokenizer::extract::extract_tokens;
use fhp_tokenizer::structural::StructuralIndexer;
use fhp_tokenizer::tokenize;

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

criterion_group!(
    benches,
    bench_structural_index,
    bench_extract_tokens,
    bench_tokenize_e2e,
);
criterion_main!(benches);
