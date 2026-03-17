# fast-html-parser

[![CI](https://github.com/nicatdcw/fast-html-parser/actions/workflows/ci.yml/badge.svg)](https://github.com/nicatdcw/fast-html-parser/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fast-html-parser.svg)](https://crates.io/crates/fast-html-parser)
[![docs.rs](https://docs.rs/fast-html-parser/badge.svg)](https://docs.rs/fast-html-parser)
[![License](https://img.shields.io/crates/l/fast-html-parser.svg)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)

SIMD-optimized HTML parser for Rust, built for web scraping workloads.

Uses SIMD instructions (SSE4.2, AVX2, NEON) for tokenization and builds a cache-line aligned arena-based DOM tree for fast traversal.

## Installation

```toml
[dependencies]
fast-html-parser = "0.1"
```

To enable optional features:

```toml
[dependencies]
fast-html-parser = { version = "0.1", features = ["xpath", "encoding", "async-tokio"] }
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
    .fragment_mode(true)
    .build();

let doc = parser.parse_str("<p>fragment</p>").unwrap();
```

### Streaming

```rust
use fast_html_parser::streaming::parse_stream;

let html = b"<div><p>Hello</p></div>";
let doc = parse_stream(html.chunks(8)).unwrap();
```

### Encoding Detection

```rust
use fast_html_parser::HtmlParser;

// Automatically detects encoding from BOM or <meta charset>
let doc = HtmlParser::parse_bytes(b"<p>Hello</p>").unwrap();
```

## Feature Flags

| Feature | Default | Description |
|---|---|---|
| `css-selector` | Yes | CSS selector engine (type, class, ID, attribute, pseudo-class, combinators) |
| `entity-decode` | Yes | HTML entity decoding |
| `xpath` | No | XPath expression support |
| `encoding` | No | Auto-detect encoding from raw bytes (BOM, meta charset) |
| `async-tokio` | No | Async parsing via Tokio |

## Architecture

The parser is organized as a workspace of focused crates:

| Crate | Purpose |
|---|---|
| `fhp-core` | Interned HTML tags (PHF), entity table, error types |
| `fhp-simd` | SIMD abstraction layer with runtime dispatch |
| `fhp-tokenizer` | Two-phase tokenizer (structural indexing + token extraction) |
| `fhp-tree` | Arena-based DOM tree with 64-byte aligned nodes |
| `fhp-selector` | CSS selector engine with bloom filter + XPath evaluator |
| `fhp-encoding` | Encoding detection and conversion via encoding_rs |
| `fast-html-parser` | Facade crate that re-exports everything |

## Performance

Benchmarked on ARM64 (Apple Silicon, NEON):

### SIMD Throughput

| Operation | Throughput |
|---|---|
| skip_whitespace | 10.2 GiB/s |
| find_delimiters | 8.3 GiB/s |
| classify_bytes | 6.2 GiB/s |

NEON achieves ~5-5.5x speedup over scalar fallback.

### Real-World Parse Throughput

| Page | Size | Time | Throughput | vs tl | vs scraper |
|---|---|---|---|---|---|
| Hacker News | 34 KB | ~105 µs | 314 MiB/s | 1.2x slower | 7x faster |
| GitHub | 301 KB | ~323 µs | 893 MiB/s | 1.1x slower | 3x faster |
| Stack Overflow | 415 KB | ~640 µs | 620 MiB/s | 1.1x faster | 2x faster |
| Wikipedia | 590 KB | ~1.08 ms | 521 MiB/s | 1.4x faster | 4x faster |

### CSS Selector (100KB HTML)

| Selector | Time |
|---|---|
| Tag (`p`) | ~10 µs |
| Class (`.highlight`) | ~15 µs |
| ID (`#main`) | ~13 µs |
| Descendant (`div p`) | ~70 µs |
| Complex (`div > ul li a`) | ~63 µs |

Per-node hash rejection (64-bit class bloom filter, ID FNV-1a) with precomputed hashes at selector parse time provides fast early exit for non-matching nodes. `:nth-child` uses O(1) cached element index.

Run benchmarks locally:

```bash
cargo bench
```

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
