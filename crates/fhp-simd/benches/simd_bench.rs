use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use fhp_simd::AllMasks;
use fhp_simd::dispatch;
use fhp_simd::scalar;

fn make_html_like_input(size: usize) -> Vec<u8> {
    let pattern = b"<div class=\"container\"><p>Hello &amp; world</p></div>\n";
    pattern.iter().copied().cycle().take(size).collect()
}

fn masks_checksum(masks: AllMasks) -> u64 {
    masks.lt ^ masks.gt.rotate_left(1) ^ masks.quot.rotate_left(2) ^ masks.apos.rotate_left(3)
}

fn scan_all_masks_scalar(input: &[u8]) -> u64 {
    input.chunks(64).fold(0, |acc, chunk| {
        acc ^ masks_checksum(scalar::compute_all_masks_safe(chunk))
    })
}

fn scan_all_masks_dispatch(input: &[u8], compute_all_masks: unsafe fn(&[u8]) -> AllMasks) -> u64 {
    input.chunks(64).fold(0, |acc, chunk| {
        let masks = unsafe { compute_all_masks(chunk) };
        acc ^ masks_checksum(masks)
    })
}

fn bench_compute_all_masks(c: &mut Criterion) {
    let ops = dispatch::ops();
    let compute_all_masks = ops.compute_all_masks;
    let mut group = c.benchmark_group("regression/fhp-simd/simd_bench/compute_all_masks");

    for size in [64, 1024, 64 * 1024] {
        let html = make_html_like_input(size);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("scalar", size), &html, |b, input| {
            b.iter(|| scan_all_masks_scalar(input))
        });

        group.bench_with_input(BenchmarkId::new("dispatch", size), &html, |b, input| {
            b.iter(|| scan_all_masks_dispatch(input, compute_all_masks))
        });
    }

    group.finish();
}

fn bench_dispatch_lookup(c: &mut Criterion) {
    // Force initialization before timing so this measures only the steady-state
    // OnceLock lookup. Hot-path benchmarks above call the cached function table.
    std::hint::black_box(dispatch::ops());

    let mut group = c.benchmark_group("diagnostic/fhp-simd/simd_bench/dispatch_lookup");
    group.bench_function("warm_once_lock", |b| {
        b.iter(|| std::hint::black_box(dispatch::ops()))
    });
    group.finish();
}

criterion_group!(benches, bench_compute_all_masks, bench_dispatch_lookup,);
criterion_main!(benches);
