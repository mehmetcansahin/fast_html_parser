# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A tokenizer oracle shared by `cargo test` and the new
  `tokenizer_differential` fuzz target, which replaces `tokenizer_equivalence`.
  It compares all five tokenizer entry points under arbitrary chunking,
  checks span and token invariants, shifts input across SIMD blocks, and
  round-trips serialized tokens.
- An HTML fuzzing dictionary and committed tokenizer seeds, both used by the
  release gate.
- The html5lib-tests tokenizer suite, vendored at a pinned commit and run by
  `cargo test` against a categorized expected-failures list.
- A test that one-shot and streaming tokenization time grows linearly on
  adversarial input, such as unterminated comments, tags, and raw text.

### Fixed

- Trim only HTML whitespace around doctype content; vertical tab, NBSP, and
  other Unicode whitespace are now kept, as the WHATWG tokenizer requires.
- Resolve benchmark fixture paths on Windows.
- Preserve Rust 1.85 support for multi-root positioned XPath evaluation.
- Bound oversized raw-text/RCDATA closing-tag candidates without accumulating
  their discarded source; preserve the original tag name and quote state.
- Drop a raw-text/RCDATA closing tag interrupted by EOF identically across
  one-shot, callback, streaming, and early-stop parsing.

## [0.2.0] - 2026-07-16

### Added

- `StreamParser::with_max_input_size` and `parse_stream_with_limit`, with raw
  and decoded-byte enforcement capped at 2 GiB.
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
- `Tag::Xmp`, `Tag::Noembed`, `Tag::Noframes`, and `Tag::Plaintext`; `xmp`,
  `noembed`, and `noframes` content is tokenized as raw text.
- `fhp_core::attr`, the single start-tag attribute grammar shared by the
  tokenizer, tree arena, and encoding prescan.
- `fast_html_parser::selector` and `fast_html_parser::encoding` re-export the
  complete `fhp-selector` and `fhp-encoding` crates (including
  `DecodingReader`).
- docs.rs metadata for every library crate; the release gate builds nightly
  rustdoc with `--cfg docsrs` and checks Rust 1.85 against a freshly resolved
  lockfile.

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
- **Breaking:** decoding is lossy. Malformed byte sequences become U+FFFD in
  byte, streaming, and async entry points; `fhp_encoding::decode` and
  `decode_or_detect` return the decoded `String` directly. `HtmlError::Encoding`
  and `EncodingError` are removed. An explicit `decode` encoding is honored
  rather than overridden by a different BOM.
- **Breaking:** XPath APIs require the `xpath` feature; 0.1.2 compiled them
  with `css-selector`.
- **Breaking:** the effective input ceiling is `min(configured_limit, 2 GiB)`,
  so named-reference expansion (at most 6/5) cannot overflow `u32` arena
  offsets.
- **Breaking:** `TreeBuilder::set_source` and `set_source_ptr` are crate-private.
- **Breaking:** `Tag` gained four variants; those names no longer appear as
  `Tag::Unknown`.
- **Breaking:** `fhp-simd` exposes only `compute_all_masks` and `level`;
  `find_delimiters`, `classify_bytes`, `skip_whitespace`, `compute_byte_mask`,
  `DelimiterResult`, and the byte-class API are removed.
- Formatting end tags are still repaired, but formatting elements closed
  implicitly by another start tag are no longer reconstructed; the write-only
  active-formatting list is removed.
- A doctype ends at its first `>`, even inside quotes, as in browsers.
- `cargo deny` audits the all-features graph, including Windows. RUSTSEC-2025-0052
  (async-std is discontinued) is acknowledged; the optional `async-async-std`
  feature remains for existing users.

### Fixed

- Ignore a source `/>` marker on non-void elements and add bounded repair for
  optional end tags, tables/foster parenting, formatting elements, `select`,
  and `plaintext`.
- Apply ASCII-case-insensitive, first-wins duplicate-attribute handling before
  decoding later duplicate values, keeping DOM, serialization, and selector
  views consistent.
- Decode the complete named entity set with distinct text and attribute
  legacy-semicolon rules.
- Tokenizer: quotes inside attribute names and unquoted values are literal, so
  `<img alt=it's>` no longer swallows the following markup; a `/` inside an
  unquoted value is kept and does not mark the tag self-closing; an end tag's
  name ends at its first `>` even when it contains `=` or a quote.
- Tokenizer: `<?…>`, unknown `<!…>` declarations, and `</` plus a non-letter
  become comments; `</>` is ignored; `<_x>` is text; `--!>` closes a comment;
  unterminated comments, CDATA, doctypes, and bogus comments are emitted at EOF,
  while an unterminated tag is dropped.
- Tokenizer: script data honors the `<!--` escape and double-escape states.
- Streaming tokenizer matches the one-shot tokenizer for every case above,
  compacts its residual once per feed, and keeps oversized tags, close-tag
  candidates, and invalid UTF-8 tails bounded without splitting valid
  characters.
- Tree: nested lists, nested tables, captions, column groups, and in-table
  `script`, `style`, `form`, and hidden `input` keep their structure instead of
  being flattened or fostered out; foster parenting depends on the current
  node; `p`, `li`, `dt`/`dd`, heading, and table-part closes respect element
  scopes; `</dd>`/`</dt>` close only an element with the same name; `</br>` and
  stray `</p>` follow the HTML rules; the leading newline after `pre`,
  `textarea`, and `listing` is dropped.
- Tree: table end tags inside a `select` in a table are ignored unless that
  element is in table scope, other end tags close an open column group, and
  formatting elements reopened by end-tag repair are foster-parented out of a
  table.
- Tree: `descendants()` of a leaf yields only the leaf, `plaintext` content
  serializes literally, and CDATA text, as well as the content of a raw-text
  start tag the builder ignores (such as `<xmp>` inside `<select>`), is
  identical across one-shot and streaming parsers.
- Encoding prescan: `<!-->` and `<!--->` are complete comments, tag ends use the
  shared attribute grammar, and `x-user-defined` maps to windows-1252.
- Selector: selecting within a `Selection` matches ancestors outside the
  selected roots, multi-root selections no longer drop matches, and `:not(...)`
  never matches the synthetic root.
- SIMD: AVX2 masks no longer sign-extend bit 31 into the upper 32 bits.
- `fhp-tree` documentation builds on docs.rs.

### Security

- Remove quadratic paths reachable from untrusted input: repeated duplicate
  attributes, crafted duplicate-name hash collisions (now randomly keyed),
  foster-parented insertions before a table, `~` sibling chains, sub-selection
  over many roots, positioned XPath text over many roots, and per-split
  residual copying in the streaming tokenizer.

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
