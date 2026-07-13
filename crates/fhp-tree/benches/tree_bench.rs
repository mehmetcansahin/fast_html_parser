//! Benchmarks for fhp-tree parse throughput and traversal operations.

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn load_testdata(name: &str) -> String {
    let path = format!(
        "{}/testdata/{name}",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("crates/fhp-tree")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

fn bench_parse(c: &mut Criterion) {
    let small = load_testdata("small_1kb.html");
    let medium = load_testdata("medium_100kb.html");
    let large = load_testdata("large_5mb.html");

    let inputs = [
        ("small_1kb", &small),
        ("medium_100kb", &medium),
        ("large_5mb", &large),
    ];

    let mut group = c.benchmark_group("regression/fhp-tree/tree_bench/parse");
    for (name, input) in &inputs {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::new("build", name), *input, |b, html| {
            b.iter_batched(
                || html.as_str(),
                |html| fhp_tree::parse(html).unwrap(),
                BatchSize::LargeInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("lifecycle", name), *input, |b, html| {
            b.iter_batched(
                || html.as_str(),
                |html| {
                    let doc = fhp_tree::parse(html).unwrap();
                    std::hint::black_box(doc.node_count());
                    drop(doc);
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_traversal(c: &mut Criterion) {
    let medium = load_testdata("medium_100kb.html");
    let doc = fhp_tree::parse(&medium).unwrap();

    let mut group = c.benchmark_group("regression/fhp-tree/tree_bench/traversal");

    group.bench_function("depth_first", |b| {
        b.iter(|| {
            let count = doc.root().descendants().count();
            std::hint::black_box(count);
        });
    });

    group.bench_function("breadth_first", |b| {
        b.iter(|| {
            let count = doc.root().descendants_bfs().count();
            std::hint::black_box(count);
        });
    });

    group.bench_function("text_content", |b| {
        b.iter_batched(|| (), |_| doc.root().text_content(), BatchSize::LargeInput);
    });

    group.finish();
}

criterion_group!(benches, bench_parse, bench_traversal);
criterion_main!(benches);
