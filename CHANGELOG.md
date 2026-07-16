# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-07-16

### Added

- `StreamParser::with_max_input_size` and `parse_stream_with_limit`, with raw
  and decoded-byte enforcement capped at `u32::MAX`.
- Explicit `stop_on_create` and `stop_after_element` early-stop modes, plus
  owned `EarlyStopMatch`, `EarlyStopOutcome`, `EarlyStopProgress`, and
  `MatchCompleteness` results.
- A vendored, checksum-pinned WHATWG named-character-reference source and
  generated exact PHF and compact legacy-trie tables covering multi-codepoint
  and legacy forms.
- A curated canonical-DOM conformance corpus and fuzz targets for tokenizer
  chunking, one-shot/streaming tree equivalence, entities, and selectors.
- Selector complexity limits and memoized matching with dense and sparse state
  storage; XPath literal name tests now retain custom element names.
- `scripts/release.py check` and `release --version`, which provide the required
  local development and release gates without installing prerequisites.
- Per-crate MIT and Apache-2.0 license copies, validated against the root files
  and all seven packaged crate archives.
- [Compatibility](COMPATIBILITY.md) and [0.2 migration](MIGRATION-0.2.md)
  contracts.
- Reproducible local benchmark verification, baseline comparison, regression
  policy, environment metadata, and publishable result summaries.
- Cross-parser canonical DOM contracts and fixture provenance metadata for
  benchmark comparisons.
- A full Apple M1 benchmark report and a same-source local-baseline
  repeatability report documenting when regression candidates require reruns.

### Changed

- **Breaking:** `StreamParser::feed` now returns `Result<(), HtmlError>` and a
  terminal parser rejects later operations.
- **Breaking:** `EarlyStopParser::stop_when`/`ParseStatus` are replaced by the
  two explicit early-stop modes and owned results.
- **Breaking:** remove `fragment_mode`; successful parses continue to expose a
  synthetic document root without browser wrapper synthesis.
- **Breaking:** `TreeBuilder::process` and `finish` are fallible, and
  `HtmlError::Parse` carries tree-construction failures. Error enums are
  non-exhaustive.
- `self_closing` in the tokenizer/tree sink boundary now records only a source
  slash; `Tag::is_void()` determines HTML void elements.
- Make `simd` a real default feature with runtime dispatch; no-default-feature
  builds force the scalar backend.
- Process large streaming chunks in 64 KiB blocks after a bounded 1 KiB
  encoding prescan, and stop iterator/async sources on the first error or
  early match.
- Limit source nesting to 512 elements, excluding the synthetic root; the
  513th returns `ParseError::NestingTooDeep` without a partial document.
- Namespace Criterion benchmarks by regression, comparison, and diagnostic
  purpose; separate parse construction from lifecycle teardown and split
  selector/XPath compilation from evaluation.
- Replace untraceable README speed claims with summaries generated from
  categorized, contract-checked benchmark runs.
- Require a clean worktree for benchmark publication, update both benchmark
  indexes together, and counterbalance comparison order with complete cyclic
  rotations. The dirty 2026-07-15 report remains provisional and is not an
  official v0.2 result.
- Clarify in the benchmark and contribution guides that a single local
  threshold failure must be confirmed before it is described as a regression.

### Fixed

- Ignore a source `/>` marker on non-void elements and add bounded repair for
  optional end tags, tables/foster parenting, formatting elements, `select`,
  and `plaintext`.
- Apply ASCII-case-insensitive, first-wins duplicate-attribute handling before
  decoding later duplicate values, keeping DOM, serialization, and selector
  views consistent.
- Decode the complete named entity set with distinct text and attribute
  legacy-semicolon rules.

### Performance

- Cache insertion modes and element-kind counts in `TreeBuilder`, keeping
  well-formed append and close operations on constant-time hot paths.
- Decode named entities with an exact PHF plus a compact legacy trie in a
  single input scan.
- Track duplicate attributes inline for the first eight names and switch to a
  case-insensitive `HashSet` fallback for larger attribute sets.

## [0.1.2] - 2026-06-08

### Fixed

- CDATA: verify the full `<![CDATA[` literal before entering CDATA mode, preventing a panic / invalid-UTF-8 slice when `<![` was followed by a multi-byte character.
- Tokenizer: treat a `<` not followed by valid markup (tag-name start, `!`, `?`, or `/tag`) as literal text instead of opening a tag.
- Tokenizer: stop entity-decoding raw-text (`<script>` / `<style>`) content, which must be passed through verbatim.
- Selector: bloom filter now matches `class` / `id` attribute names case-insensitively, fixing false-negative pre-filter rejection for uppercase attribute names.
- Selector: `DocumentIndex` lookups now resolve against the document the index was built from, preventing cross-document `NodeId` misuse.
- Encoding: remap a `<meta>`-declared `utf-16` charset to UTF-8 per the HTML spec (an ASCII-prescannable document cannot be UTF-16).
- Tree: widen attribute name/value lengths from `u16` to `u32` (no truncation for attributes larger than 64 KiB) and add an explicit flag distinguishing an empty value from a valueless boolean attribute.

## [0.1.1] - 2026-06-03

### Fixed

- Correct crates.io homepage and repository metadata for all workspace crates to `https://github.com/mehmetcansahin/fast_html_parser`.

### Changed

- Bump internal workspace dependency requirements to `0.1.1` so new published crates resolve to the corrected metadata releases.

## [0.1.0] - 2026-03-17

### Added

- `CompiledSelector` — pre-compiled CSS selector for zero-parse-overhead reuse across documents and threads
- `select_compiled()` / `select_first_compiled()` methods on `Selectable` trait and `Selection`
- `parse_owned(String)` — zero-copy parsing that transfers the String allocation directly (avoids memcpy)
- `HtmlParser::parse_owned()` static method and `parser.parse_str_owned()` instance method
- Per-node `class_hash` (64-bit bloom filter) and `id_hash` (FNV-1a) fields for fast selector rejection
- `fhp_core::hash` module with `selector_hash()` and `class_bloom_bit()` shared hash functions
- Per-node `element_index: u16` for O(1) `:nth-child` matching (computed via TreeBuilder counter)
- Precomputed class bloom bit and id hash stored in selector AST at parse time
- XPath evaluation benchmark (`xpath_bench.rs`) with 7 query patterns
- Selector parse-only benchmark (parse cost without matching)
- Entity decode isolation benchmark (fast path, sparse, dense)
- Async streaming benchmark (gated behind `async-tokio` feature)
- Compiled selector benchmarks in selector_bench, e2e_bench, and realworld_bench
- `parse_owned` vs `parse` comparison benchmarks
- Apache-2.0 license (dual-licensed: MIT OR Apache-2.0)
- Security policy (SECURITY.md)
- Code of conduct (CODE_OF_CONDUCT.md)
- Contributing guide (CONTRIBUTING.md)
- Local validation tooling for workspace, feature-matrix, formatting, and
  lint checks
- Dependabot configuration
- cargo-deny license and advisory auditing
- GitHub issue and PR templates
- docs.rs metadata and feature annotations
- rustfmt configuration

### Changed

- Node cold section layout: `class_hash: u64` + `id_hash: u32` + `element_index: u16`, padding 7 bytes (still 64 bytes total)
- CSS selector matcher uses precomputed bloom bit for `.class` and precomputed hash for `#id` — eliminates per-node hashing
- Bloom filter hashes unified to FNV-1a across ancestor bloom (bloom.rs) and per-node hashes (hash.rs)
- NEON `neon_movemask` uses static bitmask array instead of stack allocation
- `TreeBuilder::process()` marked `#[inline]` for cross-crate optimization
- Fat LTO enabled (`lto = "fat"`) for aggressive cross-crate inlining
- Delimiter loop uses pre-mask to eliminate per-iteration bounds check
- Quote-aware string masking: fast-path when carry active but no quotes in block
- Multi-root selection dedup uses sorted merge instead of HashSet
- XPath absolute path evaluation reuses buffers instead of per-step Vec allocation
- `[class~=val]` attribute selector uses fast ASCII `contains_class_token` instead of `split_whitespace`
- Selector cache includes single-simple selectors (tag, .class, #id) — no longer bypassed
- Case-insensitive attribute name matching (`eq_ignore_ascii_case`) in NodeRef, XPath, and matcher
- StreamTokenizer `scan_safe_split` with raw text context awareness (script/style)
- Consolidated testdata to single root directory (removed 5.1 MB duplicate)
- All crate Cargo.toml files include `exclude` patterns for crates.io publishing
- License field updated to "MIT OR Apache-2.0" across all crates

### Performance

- `.class` selector: **-29%** (21 µs → 15 µs) via precomputed bloom bit
- `[attr=val]` selector: **-5%** via precomputed id hash
- Tokenization stage: **-10%** (fat LTO + delimiter pre-mask)
- `:nth-child`: O(1) via cached element_index (was O(n) sibling walk)
- Class bloom filter false positive rate: ~15% → ~8% (u32 → u64)

### Initial Release

- **fhp-core**: Interned HTML tag enum (75 tags), PHF entity table, error types
- **fhp-simd**: SIMD abstraction layer with runtime dispatch (SSE4.2, AVX2, NEON, scalar fallback)
- **fhp-tokenizer**: Two-phase SIMD-accelerated tokenizer (structural indexing + token extraction)
- **fhp-tree**: Arena-based DOM tree with 64-byte cache-line aligned nodes
- **fhp-selector**: CSS selector engine with bloom filter + XPath evaluator
- **fhp-encoding**: Encoding detection and conversion via encoding_rs
- **fast-html-parser**: Facade crate with builder pattern, streaming, async support
