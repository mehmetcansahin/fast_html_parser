# fast-html-parser

[![Crates.io](https://img.shields.io/crates/v/fast-html-parser.svg)](https://crates.io/crates/fast-html-parser)
[![docs.rs](https://docs.rs/fast-html-parser/badge.svg)](https://docs.rs/fast-html-parser)
[![License](https://img.shields.io/crates/l/fast-html-parser.svg)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)

A high-speed, pragmatic HTML parser for Rust web-scraping workloads.

`fast-html-parser` deliberately is not a browser engine or a fully conforming
HTML5 implementation. It keeps a synthetic document root and implements a
curated set of malformed-markup repairs useful to scrapers, while preserving a
compact, 64-byte arena node layout. Browser wrapper synthesis (`html`, `head`,
and `body`), foreign SVG/MathML content, template/scripting algorithms, and
context-aware fragment parsing are outside the v0.2 contract. See
[Compatibility](COMPATIBILITY.md) before relying on browser-specific DOM shape.

The default `simd` feature uses runtime dispatch for SSE4.2, AVX2, or NEON when
available. Disabling default features forces the portable scalar backend.

## Installation

```toml
[dependencies]
fast-html-parser = "0.2.0"
```

To enable optional features:

```toml
[dependencies]
fast-html-parser = { version = "0.2.0", features = ["xpath", "encoding", "async-tokio"] }
```

## Quick Start

```rust
use fast_html_parser::HtmlParser;

let doc = HtmlParser::parse("<div><p>Hello</p></div>").unwrap();
assert_eq!(doc.root().text_content(), "Hello");
```

### CSS Selectors

```rust
use fast_html_parser::prelude::*;

let doc = HtmlParser::parse("<ul><li>one</li><li>two</li></ul>").unwrap();
let items = doc.select("li").unwrap();
assert_eq!(items.len(), 2);
```

### Compiled Selectors

Pre-compile a selector once and reuse it across many documents — ideal for scraping loops:

```rust
use fast_html_parser::prelude::*;

let selector = CompiledSelector::new("a.link").unwrap();

for html in &["<a class=\"link\">one</a>", "<a class=\"link\">two</a>"] {
    let doc = HtmlParser::parse(html).unwrap();
    let links = doc.select_compiled(&selector).unwrap();
    println!("{}", links.text());
}
```

### Zero-Copy Parsing

When you already own a `String` (e.g. from an HTTP response), avoid the internal memcpy:

```rust
use fast_html_parser::HtmlParser;

let body = String::from("<div><p>Hello</p></div>");
let doc = HtmlParser::parse_owned(body).unwrap();
assert_eq!(doc.root().text_content(), "Hello");
```

### XPath

```rust
use fast_html_parser::prelude::*;

let doc = HtmlParser::parse("<div><a href=\"/\">Home</a></div>").unwrap();
let result = doc.xpath("//a[@href='/']").unwrap();
```

### Builder Pattern

```rust
use fast_html_parser::HtmlParser;

let parser = HtmlParser::builder()
    .max_input_size(64 * 1024 * 1024)
    .build();

let doc = parser.parse_str("<p>page section</p>").unwrap();
assert_eq!(doc.root().text_content(), "page section");
```

The returned root is always the parser's synthetic document root. v0.2 removes
`fragment_mode`; it does not synthesize browser `html`, `head`, or `body`
wrappers.

### Streaming

```rust
use fast_html_parser::streaming::{StreamParser, parse_stream_with_limit};

let html = b"<div><p>Hello</p></div>";
let doc = parse_stream_with_limit(html.chunks(8), 1024 * 1024).unwrap();
assert_eq!(doc.root().text_content(), "Hello");

let mut parser = StreamParser::with_max_input_size(1024 * 1024);
parser.feed(b"<article>").unwrap();
parser.feed(b"from chunks</article>").unwrap();
let doc = parser.finish().unwrap();
assert_eq!(doc.root().text_content(), "from chunks");
```

Both raw and decoded byte counts are limited; the effective ceiling is
`min(configured_limit, u32::MAX)`. Encoding, nesting, and size failures make a
stream parser terminal. Iterator and async helpers stop reading after the first
error.

### Early Stop

```rust
use fast_html_parser::Tag;
use fast_html_parser::streaming::{
    EarlyStopOutcome, EarlyStopParser, EarlyStopProgress, MatchCompleteness,
};

let mut parser = EarlyStopParser::stop_after_element(|node| node.tag() == Tag::Article);
let progress = parser.feed(b"<article><b>complete</b></article><p>unread</p>").unwrap();
assert_eq!(progress, EarlyStopProgress::Matched);

let EarlyStopOutcome::Matched(found) = parser.finish().unwrap() else {
    panic!("expected an article")
};
assert_eq!(found.completeness(), MatchCompleteness::SubtreeComplete);
assert_eq!(found.node().text_content(), "complete");
```

Use `stop_on_create` when a start tag and its attributes are sufficient, or
`stop_after_element` when the complete matched subtree is required. The match
owns its `Document`; `node_id()` remains valid for that document.

### Encoding Detection

```rust
use fast_html_parser::HtmlParser;

// Detects encoding from a BOM or an HTML <meta> prescan, then falls back to UTF-8.
let doc = HtmlParser::parse_bytes(b"<p>Hello</p>").unwrap();
```

HTTP `Content-Type` encoding hints are not accepted by the parser in v0.2;
apply an HTTP-layer decision before calling the parser if your scraper needs
that signal.

## Feature Flags

| Feature | Default | Description |
|---|---|---|
| `css-selector` | Yes | CSS selector engine (type, class, ID, attribute, pseudo-class, combinators) |
| `entity-decode` | Yes | HTML entity decoding |
| `xpath` | No | XPath expression support |
| `encoding` | Yes | Raw-byte and streaming parsing with encoding detection (BOM, meta charset) |
| `async-tokio` | No | Async parsing via Tokio |
| `async-async-std` | No | Async parsing via async-std |
| `simd` | Yes | Runtime-dispatched SIMD scanning; disable default features for scalar-only execution |

## Architecture

The parser is organized as a workspace of focused crates:

| Crate | Purpose |
|---|---|
| `fhp-core` | Interned HTML tags, generated WHATWG entity PHF/legacy trie, error types |
| `fhp-simd` | SIMD abstraction layer with runtime dispatch |
| `fhp-tokenizer` | Two-phase tokenizer (structural indexing + token extraction) |
| `fhp-tree` | Arena-based DOM tree with 64-byte aligned nodes |
| `fhp-selector` | CSS selector engine with bloom filter + XPath evaluator |
| `fhp-encoding` | Encoding detection and conversion via encoding_rs |
| `fast-html-parser` | Facade crate that re-exports everything |

## Performance

Benchmarks use Criterion with explicit workload boundaries:

- `regression/` measures project-owned hot paths and participates in the local
  regression policy.
- `comparison/` reports cross-library workloads but never gates a change.
- `diagnostic/` isolates implementation costs such as selector compilation,
  dispatch lookup, and source copying.

DOM construction, zero-copy parsing, owned-input transfer, and streaming
rewriting are reported separately. Cross-parser speed ratios are emitted only
when the parsers produce the same canonical DOM digest for that fixture. Raw
Criterion samples remain machine-local; each published summary
records the source, fixture, machine, toolchain, build flags, and commands.

<!-- benchmark-summary:start -->
_No official schema-3 v0.2 benchmark has been published yet. A clean,
user-approved commit must be measured with `python3 scripts/bench.py publish`
before release._
<!-- benchmark-summary:end -->

The [2026-07-15 report](benchmarks/results/2026-07-15-b4fcf640b253-aarch64-apple-darwin.md)
came from a dirty worktree and is kept only as provisional historical data; it
is excluded from the latest link and official comparisons.
`bench.py publish` now requires a clean source tree and updates this README and
the benchmark index together. A clean, approved commit must replace the report
before release.

The current comparison harness counterbalances parser order with the complete
set of all six parser permutations. The provisional report predates that contract.
Local `save`/`compare` failures are regression candidates and require confirmation;
the [same-source repeatability study](benchmarks/results/2026-07-13-local-baseline-repeatability.md)
shows where microbenchmarks, async scheduling, and lifecycle teardown were
sensitive to run order or machine state.

Run the quick local suite:

```bash
python3 scripts/bench.py quick
```

See [Benchmark results](benchmarks/README.md) for verification, baseline,
comparison, and publication commands. Fixture sizes, digests, and provenance
are documented in [Benchmark fixtures](testdata/README.md).

## Local Quality Gates

This repository intentionally does not rely on GitHub Actions. Run the required
development gate locally:

```bash
python3 scripts/release.py check
```

Maintainers run the stricter clean-tree release gate with the intended version:

```bash
python3 scripts/release.py release --version 0.2.0
```

The release command checks the MSRV, platform targets, cargo-deny, SIMD modes,
fuzz targets, package contents and licenses, and clean benchmark metadata. It
does not install missing toolchains, targets, `cargo-deny`, or `cargo-fuzz`.
See [Contributing](CONTRIBUTING.md) for prerequisites.

## Examples

```bash
cargo run --example basic_parse
cargo run --example web_scraping --features css-selector
cargo run --example streaming
cargo run --example xpath_query --features xpath
cargo run --example encoding --features encoding
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
