# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Apache-2.0 license (dual-licensed: MIT OR Apache-2.0)
- Security policy (SECURITY.md)
- Code of conduct (CODE_OF_CONDUCT.md)
- Contributing guide (CONTRIBUTING.md)
- GitHub CI workflow with matrix testing
- Dependabot configuration
- cargo-deny license and advisory auditing
- GitHub issue and PR templates
- docs.rs metadata and feature annotations
- rustfmt configuration

### Changed

- License field updated to "MIT OR Apache-2.0" across all crates
- README updated with badges and dual-license info

## [0.1.0] - 2026-02-06

### Added

- **fhp-core**: Interned HTML tag enum (75 tags), PHF entity table, error types
- **fhp-simd**: SIMD abstraction layer with runtime dispatch (SSE4.2, AVX2, NEON, scalar fallback)
- **fhp-tokenizer**: Two-phase SIMD-accelerated tokenizer (structural indexing + token extraction)
- **fhp-tree**: Arena-based DOM tree with 64-byte cache-line aligned nodes
  - Depth-first and breadth-first traversal iterators
  - Inner/outer HTML reconstruction
  - Implicit close rules, void element handling, broken HTML recovery
- **fhp-selector**: CSS selector engine with bloom filter acceleration
  - Full CSS selector support: type, class, ID, attribute, pseudo-class, combinators
  - XPath subset: descendant search, attribute predicates, position, text(), contains()
  - Right-to-left matching with ancestor bloom filter pre-filtering
- **fhp-encoding**: Encoding detection and conversion via encoding_rs
  - BOM detection (UTF-8, UTF-16 LE/BE)
  - Meta charset prescan (first 1 KB)
  - Streaming decoder for chunk-based processing
- **fhp-tree streaming**: Streaming and incremental parsing
  - `StreamParser` with encoding-aware chunk buffering
  - `EarlyStopParser` with predicate-based early termination
  - Async parsing via Tokio (`async-tokio` feature)
- **fast-html-parser**: Facade crate with builder pattern API
  - `HtmlParser::parse()`, `HtmlParser::parse_bytes()` convenience methods
  - `HtmlParser::builder()` for custom configuration
  - Re-exports of all sub-crate public APIs
  - Feature flags: `css-selector`, `xpath`, `encoding`, `async-tokio`, `entity-decode`
  - End-to-end benchmarks with criterion
  - 5 example programs

### Performance

- SIMD tokenization: ~5-5.5x speedup over scalar on ARM64 (NEON)
- Parse throughput: 173-222 MiB/s on ARM64 (including tree build)
- CSS selector (100KB HTML): tag select ~15us, descendant ~67us
