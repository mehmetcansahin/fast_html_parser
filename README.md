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

| Operation | Throughput |
|---|---|
| SIMD skip_whitespace | 10.2 GiB/s |
| SIMD find_delimiters | 8.3 GiB/s |
| SIMD classify_bytes | 6.2 GiB/s |
| Parse + tree build | 173-222 MiB/s |
| CSS tag select (100KB) | ~15 us |
| CSS descendant select (100KB) | ~67 us |

NEON achieves ~5-5.5x speedup over scalar fallback.

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
