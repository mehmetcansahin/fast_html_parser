//! End-to-end benchmarks for the HTML parser.
//!
//! Measures full-pipeline throughput: tokenize + tree build + (optionally) selector.

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use fast_html_parser::HtmlParser;
#[cfg(feature = "css-selector")]
use fast_html_parser::Selectable;
use fast_html_parser::streaming::parse_stream;

const SMALL_HTML: &str = include_str!("../../../testdata/small_1kb.html");
const MEDIUM_HTML: &str = include_str!("../../../testdata/medium_100kb.html");

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fast-html-parser/e2e_bench/parse");

    for (name, html) in [("1kb", SMALL_HTML), ("100kb", MEDIUM_HTML)] {
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(BenchmarkId::new("build", name), html, |b, input| {
            b.iter_batched(
                || input,
                |input| HtmlParser::parse(input).unwrap(),
                BatchSize::LargeInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("lifecycle", name), html, |b, input| {
            b.iter_batched(
                || input,
                |input| {
                    let document = HtmlParser::parse(input).unwrap();
                    std::hint::black_box(document.node_count());
                    drop(document);
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_parse_owned(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fast-html-parser/e2e_bench/parse_owned");

    for (name, html) in [("1kb", SMALL_HTML), ("100kb", MEDIUM_HTML)] {
        group.throughput(Throughput::Bytes(html.len() as u64));

        group.bench_with_input(BenchmarkId::new("borrow/build", name), html, |b, input| {
            b.iter_batched(
                || input,
                |input| HtmlParser::parse(input).unwrap(),
                BatchSize::LargeInput,
            );
        });
        group.bench_with_input(
            BenchmarkId::new("borrow/lifecycle", name),
            html,
            |b, input| {
                b.iter_batched(
                    || input,
                    |input| {
                        let document = HtmlParser::parse(input).unwrap();
                        std::hint::black_box(document.node_count());
                        drop(document);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
        group.bench_with_input(BenchmarkId::new("owned/build", name), html, |b, input| {
            b.iter_batched(
                || input.to_owned(),
                |input| HtmlParser::parse_owned(input).unwrap(),
                BatchSize::LargeInput,
            );
        });
        group.bench_with_input(
            BenchmarkId::new("owned/lifecycle", name),
            html,
            |b, input| {
                b.iter_batched(
                    || input.to_owned(),
                    |input| {
                        let document = HtmlParser::parse_owned(input).unwrap();
                        std::hint::black_box(document.node_count());
                        drop(document);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_parse_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fast-html-parser/e2e_bench/parse_bytes");

    for (name, html) in [("1kb", SMALL_HTML), ("100kb", MEDIUM_HTML)] {
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("auto_encoding/build", name),
            html.as_bytes(),
            |b, input| {
                b.iter_batched(
                    || input,
                    |input| HtmlParser::parse_bytes(input).unwrap(),
                    BatchSize::LargeInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("auto_encoding/lifecycle", name),
            html.as_bytes(),
            |b, input| {
                b.iter_batched(
                    || input,
                    |input| {
                        let document = HtmlParser::parse_bytes(input).unwrap();
                        std::hint::black_box(document.node_count());
                        drop(document);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();
}

fn bench_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fast-html-parser/e2e_bench/streaming");

    let chunk_sizes = [64, 1024, 8192, 65536];

    for &chunk_size in &chunk_sizes {
        let html = MEDIUM_HTML.as_bytes();
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("sync/build", format!("chunk_{chunk_size}")),
            &chunk_size,
            |b, &cs| {
                b.iter_batched(
                    || cs,
                    |cs| parse_stream(html.chunks(cs)).unwrap(),
                    BatchSize::LargeInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("sync/lifecycle", format!("chunk_{chunk_size}")),
            &chunk_size,
            |b, &cs| {
                b.iter_batched(
                    || cs,
                    |cs| {
                        let document = parse_stream(html.chunks(cs)).unwrap();
                        std::hint::black_box(document.node_count());
                        drop(document);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    // Async streaming via tokio (if feature enabled).
    #[cfg(feature = "async-tokio")]
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        for &chunk_size in &chunk_sizes {
            let html = MEDIUM_HTML.as_bytes();
            group.throughput(Throughput::Bytes(html.len() as u64));
            group.bench_with_input(
                BenchmarkId::new("async/build", format!("chunk_{chunk_size}")),
                &chunk_size,
                |b, &cs| {
                    b.iter_batched(
                        || tokio::io::BufReader::with_capacity(cs, html),
                        |reader| {
                            rt.block_on(async {
                                fast_html_parser::async_parser::parse_async(reader)
                                    .await
                                    .unwrap()
                            })
                        },
                        BatchSize::LargeInput,
                    );
                },
            );
            group.bench_with_input(
                BenchmarkId::new("async/lifecycle", format!("chunk_{chunk_size}")),
                &chunk_size,
                |b, &cs| {
                    b.iter_batched(
                        || tokio::io::BufReader::with_capacity(cs, html),
                        |reader| {
                            rt.block_on(async {
                                let document = fast_html_parser::async_parser::parse_async(reader)
                                    .await
                                    .unwrap();
                                std::hint::black_box(document.node_count());
                                drop(document);
                            })
                        },
                        BatchSize::LargeInput,
                    );
                },
            );
        }
    }

    group.finish();
}

#[cfg(feature = "css-selector")]
fn bench_select(c: &mut Criterion) {
    use fast_html_parser::CompiledSelector;

    let doc = HtmlParser::parse(MEDIUM_HTML).unwrap();
    let selectors = [
        ("tag_p", "p"),
        ("class", ".content"),
        ("descendant", "div p"),
        ("complex", "div.article > h2 + p"),
    ];
    let compiled: Vec<_> = selectors
        .iter()
        .map(|(name, css)| (*name, CompiledSelector::new(css).unwrap()))
        .collect();

    let mut evaluate = c.benchmark_group("regression/fast-html-parser/e2e_bench/select/evaluate");
    for (name, selector) in &compiled {
        evaluate.bench_with_input(
            BenchmarkId::from_parameter(name),
            selector,
            |b, selector| {
                b.iter_batched(
                    || (),
                    |_| doc.select_compiled(selector).unwrap(),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    evaluate.finish();

    let mut compile = c.benchmark_group("regression/fast-html-parser/e2e_bench/select/compile");
    for (name, css) in selectors {
        compile.bench_with_input(BenchmarkId::from_parameter(name), css, |b, css| {
            b.iter_batched(
                || css,
                |css| CompiledSelector::new(css).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }
    compile.finish();

    let mut string =
        c.benchmark_group("diagnostic/fast-html-parser/e2e_bench/select/string_convenience");
    for (name, css) in selectors {
        // Warm the thread-local cache before measuring the string convenience API.
        doc.select(css).unwrap();
        string.bench_with_input(BenchmarkId::from_parameter(name), css, |b, css| {
            b.iter_batched(|| (), |_| doc.select(css).unwrap(), BatchSize::LargeInput);
        });
    }
    string.finish();
}

#[cfg(not(feature = "css-selector"))]
fn bench_select(_c: &mut Criterion) {}

fn bench_tree_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression/fast-html-parser/e2e_bench/traversal");

    let doc = HtmlParser::parse(MEDIUM_HTML).unwrap();

    group.bench_function("text_content", |b| {
        b.iter_batched(|| (), |_| doc.root().text_content(), BatchSize::LargeInput);
    });

    group.bench_function("depth_first", |b| {
        b.iter(|| {
            let mut count = 0u64;
            for _ in doc.root().descendants() {
                count += 1;
            }
            count
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_parse_owned,
    bench_parse_bytes,
    bench_streaming,
    bench_select,
    bench_tree_traversal,
);
criterion_main!(benches);
