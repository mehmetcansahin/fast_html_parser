use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use hp_simd::dispatch;
use hp_simd::scalar;

fn make_html_like_input(size: usize) -> Vec<u8> {
    let pattern = b"<div class=\"container\"><p>Hello &amp; world</p></div>\n";
    pattern.iter().copied().cycle().take(size).collect()
}

fn make_whitespace_heavy(size: usize) -> Vec<u8> {
    let pattern = b"    \t\n\r  ";
    let mut v: Vec<u8> = pattern.iter().copied().cycle().take(size).collect();
    // Put a non-whitespace byte at the end so skip_whitespace has work to do.
    if let Some(last) = v.last_mut() {
        *last = b'X';
    }
    v
}

fn make_no_delimiters(size: usize) -> Vec<u8> {
    vec![b'a'; size]
}

fn bench_find_delimiters(c: &mut Criterion) {
    let mut group = c.benchmark_group("find_delimiters");

    for size in [64, 1024, 64 * 1024] {
        let html = make_html_like_input(size);
        let no_delim = make_no_delimiters(size);

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("scalar/html", size), &html, |b, input| {
            b.iter(|| scalar::find_delimiters_safe(input))
        });

        group.bench_with_input(
            BenchmarkId::new("dispatch/html", size),
            &html,
            |b, input| {
                let ops = dispatch::ops();
                b.iter(|| unsafe { (ops.find_delimiters)(input) })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("scalar/no_match", size),
            &no_delim,
            |b, input| b.iter(|| scalar::find_delimiters_safe(input)),
        );

        group.bench_with_input(
            BenchmarkId::new("dispatch/no_match", size),
            &no_delim,
            |b, input| {
                let ops = dispatch::ops();
                b.iter(|| unsafe { (ops.find_delimiters)(input) })
            },
        );
    }

    group.finish();
}

fn bench_classify_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("classify_bytes");

    for size in [64, 1024, 64 * 1024] {
        let html = make_html_like_input(size);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("scalar", size), &html, |b, input| {
            b.iter(|| scalar::classify_bytes_safe(input))
        });

        group.bench_with_input(BenchmarkId::new("dispatch", size), &html, |b, input| {
            let ops = dispatch::ops();
            b.iter(|| unsafe { (ops.classify_bytes)(input) })
        });
    }

    group.finish();
}

fn bench_skip_whitespace(c: &mut Criterion) {
    let mut group = c.benchmark_group("skip_whitespace");

    for size in [64, 1024, 64 * 1024] {
        let ws = make_whitespace_heavy(size);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("scalar", size), &ws, |b, input| {
            b.iter(|| scalar::skip_whitespace_safe(input))
        });

        group.bench_with_input(BenchmarkId::new("dispatch", size), &ws, |b, input| {
            let ops = dispatch::ops();
            b.iter(|| unsafe { (ops.skip_whitespace)(input) })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_find_delimiters,
    bench_classify_bytes,
    bench_skip_whitespace,
);
criterion_main!(benches);
