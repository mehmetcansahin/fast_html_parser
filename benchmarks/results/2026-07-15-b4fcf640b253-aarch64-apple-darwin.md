# Benchmark report

> Historical provisional result. This report was generated from a dirty
> worktree with schema 2. It is excluded from the latest official result,
> baseline compatibility, release provenance, and performance gates.

Generated at `2026-07-15T07:20:19+00:00`.

## Reproducibility metadata

| Field | Value |
|---|---|
| Source digest | `b4fcf640b25364784668d33ed913ff0f47b2c917ab915c1677221e65f5bca299` |
| Fixture manifest digest | `273aaf25eb2d36b5fcefb89d507a4cff68cb6030093df4d7eca7adad171710c8` |
| Git commit | `e257bbc508b85f8c3714c11e7cfc5a9221dded05` (dirty) |
| Target | `aarch64-apple-darwin` |
| CPU | Apple M1 (`arm64`) |
| OS | Darwin 25.5.0 |
| rustc | `1.93.0` (`254b59607d4417e9dffbc307138ae5c86280fe4c`) |
| Cargo | `cargo 1.93.0 (083ac5135 2025-12-15)` |
| RUSTFLAGS | `-C target-cpu=native` |
| CARGO_INCREMENTAL | `0` |
| Benchmark scope | `full` |
| Criterion | `0.5.1`; quick=false |
| Criterion settings | `{"confidence_level": 0.95, "measurement_time_seconds": 5.0, "noise_threshold": 0.01, "sample_size": 100, "significance_level": 0.05, "warm_up_time_seconds": 3.0}` |

## Harness and feature matrix

| Harness | Target | Features | Filter |
|---|---|---|---|
| `fhp-simd/simd` | `fhp-simd/simd_bench` | `(none)` | `(all)` |
| `fhp-tokenizer/tokenizer` | `fhp-tokenizer/tokenizer_bench` | `entity-decode` | `(all)` |
| `fhp-tree/tree` | `fhp-tree/tree_bench` | `encoding, entity-decode` | `(all)` |
| `fhp-selector/selector` | `fhp-selector/selector_bench` | `(none)` | `(all)` |
| `fhp-selector/xpath` | `fhp-selector/xpath_bench` | `(none)` | `(all)` |
| `fast-html-parser/e2e` | `fast-html-parser/e2e_bench` | `css-selector, encoding, entity-decode` | `(all)` |
| `fast-html-parser/e2e-async-tokio` | `fast-html-parser/e2e_bench` | `css-selector, encoding, entity-decode, async-tokio` | `streaming/async` |
| `fast-html-parser/profile` | `fast-html-parser/profile_bench` | `css-selector, encoding, entity-decode` | `(all)` |
| `fast-html-parser/comparison` | `fast-html-parser/comparison_bench` | `css-selector, encoding, entity-decode` | `(all)` |
| `fast-html-parser/realworld` | `fast-html-parser/realworld_bench` | `css-selector, encoding, entity-decode` | `(all)` |

## Fixture integrity

| Fixture | Kind | Bytes | SHA-256 | Known source | Capture date |
|---|---|---:|---|---|---|
| `amazon.html` | Snapshot | 5,088 | `4af85243ae0939808462e294532a703ef20b876cde34f3ef630cbc4024676e06` | Amazon home page | Unknown |
| `github.html` | Snapshot | 301,093 | `8ca2eb2ec5663c0884bfb02c0681179e085889917dc9c3be31d1bba16e6f4484` | GitHub “Page not found” response; exact URL unavailable | Unknown |
| `hackernews.html` | Snapshot | 34,284 | `6e717995d1f65979a1a440a0d1d73d2a7e6d69c05454a48a98293b11b2d10456` | `https://news.ycombinator.com/` | Unknown |
| `large_5mb.html` | Synthetic | 5,395,593 | `0e8cb49ac877163247d010ea8ac1f5aaf39d4af3173273cbe63a5333d3c195d5` | Repository-generated HTML document | N/A |
| `medium_100kb.html` | Synthetic | 115,286 | `4b18d2e30adf0e65448df0fdbba0dba6a852e7296ab75beaf2eb8436eea0e427` | Repository-generated HTML document | N/A |
| `small_1kb.html` | Synthetic | 1,478 | `e19f021eca83dcf8b5c2051dc5eea145846304fcf51a16dee35410be0b9ff489` | Repository-generated HTML document | N/A |
| `stackoverflow.html` | Snapshot | 415,096 | `feb689ff4e3b62e27d70a999953b7da6a73e855d9d7645b4514b3bc1ea0ee6f2` | Stack Overflow newest questions tagged `rust` | Unknown |
| `wikipedia.html` | Snapshot | 589,673 | `cd1faa46cd5a9272424049af92204242df7b0dfcdb1e7d214c132c0c1308699f` | `https://en.wikipedia.org/wiki/Rust_(programming_language)` | Unknown |

## Absolute estimates

Single-run rows show Criterion's 95% confidence interval. The two order-sensitive comparison harnesses show the median of three run means and their min–max range. Lower time is better.

| Benchmark | Category | Correctness status | Estimate | 95% CI / run range | Throughput | Runs |
|---|---|---|---:|---:|---:|---:|
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 277.84 µs | run range: 275.12 µs–283.13 µs | 395.72 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 1.63 ms | run range: 1.60 ms–2.36 ms | 67.57 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 277.84 µs | run range: 277.32 µs–278.53 µs | 395.72 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 1.70 ms | run range: 1.67 ms–1.72 ms | 64.56 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 275.80 µs | run range: 274.17 µs–276.02 µs | 398.64 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 161.81 µs | run range: 159.86 µs–162.36 µs | 679.49 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 279.22 µs | run range: 276.34 µs–729.30 µs | 393.76 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 174.14 µs | run range: 173.43 µs–174.65 µs | 631.37 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 391.48 µs | run range: 389.35 µs–526.41 µs | 280.85 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 160.45 µs | run range: 160.26 µs–172.06 µs | 685.22 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 174.50 µs | run range: 173.43 µs–174.73 µs | 630.05 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/compile/fast_html_parser` | `comparison` | contract-equal | 120.36 ns | run range: 117.76 ns–130.18 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/compile/tl` | `comparison` | contract-equal | 10.36 ns | run range: 10.33 ns–10.75 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/evaluate_materialized/fast_html_parser` | `comparison` | contract-equal | 12.70 µs | run range: 12.63 µs–19.62 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/evaluate_materialized/tl` | `comparison` | contract-equal | 26.88 µs | run range: 26.63 µs–28.96 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 121.58 ns | run range: 116.52 ns–301.19 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 119.08 ns | run range: 118.56 ns–122.55 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 10.25 ns | run range: 10.09 ns–10.73 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 12.91 µs | run range: 12.63 µs–13.11 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 12.89 µs | run range: 12.46 µs–13.00 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 27.72 µs | run range: 26.60 µs–29.10 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 306.56 ns | run range: 296.75 ns–335.38 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 178.88 ns | run range: 177.34 ns–189.17 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 48.17 ns | run range: 46.47 ns–48.53 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 82.77 µs | run range: 79.76 µs–114.66 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 12.19 µs | run range: 11.43 µs–12.57 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 8.51 µs | run range: 8.45 µs–9.04 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/compile/fast_html_parser` | `comparison` | contract-equal | 131.54 ns | run range: 128.66 ns–132.87 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/compile/tl` | `comparison` | contract-equal | 8.09 ns | run range: 8.09 ns–8.62 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/evaluate_materialized/fast_html_parser` | `comparison` | contract-equal | 10.09 µs | run range: 9.91 µs–10.10 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/evaluate_materialized/tl` | `comparison` | contract-equal | 12.21 µs | run range: 12.16 µs–12.40 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 284.91 ns | run range: 127.88 ns–760.70 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 92.57 ns | run range: 92.53 ns–93.74 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 7.79 ns | run range: 7.58 ns–8.20 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 21.73 µs | run range: 9.90 µs–40.09 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 11.51 µs | run range: 11.46 µs–11.53 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 12.53 µs | run range: 12.32 µs–108.18 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/build/fast_html_parser` | `comparison` | contract-equal | 4.12 µs | run range: 4.11 µs–4.42 µs | 342.23 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/build/scraper` | `comparison` | contract-equal | 29.09 µs | run range: 28.28 µs–33.15 µs | 48.46 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/fast_html_parser` | `comparison` | contract-equal | 5.04 µs | run range: 4.12 µs–7.75 µs | 279.57 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/scraper` | `comparison` | contract-equal | 29.96 µs | run range: 29.18 µs–32.52 µs | 47.05 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.09 µs | run range: 4.04 µs–4.36 µs | 344.99 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 29.54 µs | run range: 28.30 µs–34.24 µs | 47.71 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.41 µs | run range: 4.12 µs–4.72 µs | 319.69 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 29.21 µs | run range: 29.11 µs–29.49 µs | 48.25 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.19 µs | run range: 4.03 µs–4.19 µs | 336.36 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 3.43 µs | run range: 3.41 µs–4.14 µs | 411.17 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.14 µs | run range: 4.06 µs–4.97 µs | 340.61 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 4.50 µs | run range: 3.76 µs–26.81 µs | 313.06 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 7.90 µs | run range: 7.35 µs–15.22 µs | 178.46 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 3.45 µs | run range: 3.39 µs–3.68 µs | 408.97 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 3.98 µs | run range: 3.74 µs–4.08 µs | 354.43 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 13.23 ms | run range: 12.86 ms–13.31 ms | 388.83 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 79.80 ms | run range: 77.78 ms–87.20 ms | 64.48 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 13.24 ms | run range: 12.67 ms–33.35 ms | 388.73 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 83.35 ms | run range: 82.20 ms–83.93 ms | 61.73 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 12.64 ms | run range: 12.57 ms–13.57 ms | 406.95 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 10.10 ms | run range: 9.15 ms–13.40 ms | 509.40 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 12.54 ms | run range: 12.46 ms–12.79 ms | 410.40 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 13.94 ms | run range: 11.71 ms–14.72 ms | 369.18 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 16.98 ms | run range: 16.81 ms–23.65 ms | 302.96 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 10.75 ms | run range: 9.09 ms–12.71 ms | 478.49 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 11.60 ms | run range: 11.53 ms–16.42 ms | 443.68 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/build/fast_html_parser` | `comparison` | contract-equal | 473.28 µs | run range: 466.63 µs–509.82 µs | 606.71 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/build/scraper` | `comparison` | contract-equal | 2.66 ms | run range: 2.61 ms–2.66 ms | 107.97 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/fast_html_parser` | `comparison` | contract-equal | 469.61 µs | run range: 465.71 µs–473.46 µs | 611.45 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/scraper` | `comparison` | contract-equal | 2.73 ms | run range: 2.69 ms–2.80 ms | 105.36 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 477.32 µs | run range: 465.45 µs–479.45 µs | 601.58 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 2.66 ms | run range: 2.62 ms–3.10 ms | 107.94 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 486.45 µs | run range: 465.64 µs–516.07 µs | 590.28 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 2.73 ms | run range: 2.72 ms–2.73 ms | 105.08 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 464.26 µs | run range: 458.60 µs–516.92 µs | 618.50 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 261.68 µs | run range: 260.30 µs–263.56 µs | 1.07 GiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 465.37 µs | run range: 459.40 µs–509.20 µs | 617.02 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 285.04 µs | run range: 282.98 µs–292.48 µs | 1007.38 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 375.35 µs | run range: 372.50 µs–378.24 µs | 765.01 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 264.92 µs | run range: 264.03 µs–265.04 µs | 1.06 GiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 286.27 µs | run range: 284.24 µs–316.36 µs | 1003.05 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/build/fast_html_parser` | `comparison` | contract-equal | 119.83 µs | run range: 112.66 µs–122.52 µs | 272.86 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/build/scraper` | `comparison` | contract-equal | 634.87 µs | run range: 632.67 µs–684.51 µs | 51.50 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/fast_html_parser` | `comparison` | contract-equal | 114.59 µs | run range: 113.92 µs–117.09 µs | 285.34 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/scraper` | `comparison` | contract-equal | 660.33 µs | run range: 649.71 µs–707.17 µs | 49.51 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 112.71 µs | run range: 112.63 µs–112.80 µs | 290.10 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 666.81 µs | run range: 627.85 µs–1.04 ms | 49.03 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 122.20 µs | run range: 113.30 µs–126.65 µs | 267.56 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 663.28 µs | run range: 655.52 µs–700.68 µs | 49.29 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 111.99 µs | run range: 111.81 µs–112.64 µs | 291.95 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 70.85 µs | run range: 67.14 µs–83.98 µs | 461.49 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 112.20 µs | run range: 111.88 µs–113.70 µs | 291.40 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 76.65 µs | run range: 73.22 µs–76.98 µs | 426.57 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 225.67 µs | run range: 216.24 µs–227.31 µs | 144.88 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 70.25 µs | run range: 67.71 µs–72.30 µs | 465.42 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 82.78 µs | run range: 75.41 µs–83.78 µs | 394.96 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 944.04 µs | run range: 935.75 µs–944.40 µs | 419.33 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 4.72 ms | run range: 4.64 ms–5.00 ms | 83.83 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 951.54 µs | run range: 932.08 µs–953.79 µs | 416.03 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 4.92 ms | run range: 4.90 ms–4.92 ms | 80.54 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 933.43 µs | run range: 924.28 µs–948.41 µs | 424.10 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 591.09 µs | run range: 575.65 µs–636.81 µs | 669.72 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 942.62 µs | run range: 925.10 µs–1.07 ms | 419.96 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 646.15 µs | run range: 626.62 µs–658.58 µs | 612.66 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 930.62 µs | run range: 913.64 µs–1.02 ms | 425.38 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 591.50 µs | run range: 580.09 µs–595.69 µs | 669.26 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 647.24 µs | run range: 628.56 µs–717.57 µs | 611.62 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.88 ms | run range: 1.79 ms–1.92 ms | 299.15 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 8.06 ms | run range: 7.90 ms–8.48 ms | 69.79 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.81 ms | run range: 1.80 ms–1.99 ms | 310.70 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 8.41 ms | run range: 8.23 ms–8.49 ms | 66.90 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.88 ms | run range: 1.77 ms–1.91 ms | 298.85 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 1.20 ms | run range: 1.16 ms–1.27 ms | 468.87 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.80 ms | run range: 1.77 ms–2.03 ms | 312.58 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 1.58 ms | run range: 1.36 ms–1.59 ms | 355.41 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 2.30 ms | run range: 2.30 ms–2.43 ms | 244.09 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 1.22 ms | run range: 1.16 ms–1.23 ms | 460.76 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 1.41 ms | run range: 1.31 ms–1.46 ms | 399.02 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 177.81 ns | run range: 163.53 ns–182.36 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 157.44 ns | run range: 154.20 ns–167.30 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 53.64 ns | run range: 53.13 ns–61.74 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 81.64 µs | run range: 80.15 µs–94.79 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 82.53 µs | run range: 77.31 µs–84.02 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 114.27 µs | run range: 106.08 µs–148.11 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 299.09 ns | run range: 294.05 ns–317.38 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 177.23 ns | run range: 176.25 ns–177.32 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 50.56 ns | run range: 49.34 ns–50.62 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 528.78 µs | run range: 518.13 µs–573.63 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 80.16 µs | run range: 78.35 µs–83.71 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 37.71 µs | run range: 37.45 µs–39.25 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 154.74 ns | run range: 151.73 ns–196.61 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 198.98 ns | run range: 198.66 ns–200.15 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 46.39 ns | run range: 45.83 ns–47.86 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 166.24 µs | run range: 164.24 µs–172.26 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 96.37 µs | run range: 94.96 µs–109.50 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 123.82 µs | run range: 122.28 µs–125.30 µs | — | 3 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/class` | `diagnostic` | diagnostic (no equality contract) | 12.01 µs | 95% CI: 11.76 µs–12.37 µs | — | 1 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/complex` | `diagnostic` | diagnostic (no equality contract) | 12.54 µs | 95% CI: 12.28 µs–12.88 µs | — | 1 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/descendant` | `diagnostic` | diagnostic (no equality contract) | 79.83 µs | 95% CI: 79.63 µs–80.07 µs | — | 1 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/tag_p` | `diagnostic` | diagnostic (no equality contract) | 10.39 µs | 95% CI: 10.10 µs–10.81 µs | — | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/01_simd_index` | `diagnostic` | diagnostic (no equality contract) | 56.54 µs | 95% CI: 55.08 µs–58.78 µs | 1.90 GiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/02_tokenize_vec` | `diagnostic` | diagnostic (no equality contract) | 301.93 µs | 95% CI: 297.22 µs–309.17 µs | 364.15 MiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/03_tokenize_with_noop` | `diagnostic` | diagnostic (no equality contract) | 286.63 µs | 95% CI: 272.77 µs–312.41 µs | 383.58 MiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/04_full_parse` | `diagnostic` | diagnostic (no equality contract) | 295.66 µs | 95% CI: 292.18 µs–299.62 µs | 371.86 MiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/05_tree_build_from_pretokenized` | `diagnostic` | diagnostic (no equality contract) | 91.24 µs | 95% CI: 89.25 µs–93.54 µs | 1.18 GiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/06_memcpy_100kb` | `diagnostic` | diagnostic (no equality contract) | 2.22 µs | 95% CI: 2.14 µs–2.32 µs | 48.35 GiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/07_tl_parse` | `diagnostic` | diagnostic (no equality contract) | 206.93 µs | 95% CI: 201.76 µs–215.94 µs | 531.33 MiB/s | 1 |
| `diagnostic/fhp-selector/selector_bench/string_convenience/string_class` | `diagnostic` | diagnostic (no equality contract) | 12.35 µs | 95% CI: 12.18 µs–12.53 µs | — | 1 |
| `diagnostic/fhp-selector/selector_bench/string_convenience/string_compound` | `diagnostic` | diagnostic (no equality contract) | 162.59 µs | 95% CI: 151.02 µs–174.95 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/absolute_path` | `diagnostic` | diagnostic (no equality contract) | 342.19 ns | 95% CI: 313.74 ns–380.91 ns | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/contains` | `diagnostic` | diagnostic (no equality contract) | 18.85 µs | 95% CI: 18.79 µs–18.92 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/descendant_attr` | `diagnostic` | diagnostic (no equality contract) | 17.26 µs | 95% CI: 17.00 µs–17.58 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/descendant_p` | `diagnostic` | diagnostic (no equality contract) | 13.67 µs | 95% CI: 13.57 µs–13.78 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/position` | `diagnostic` | diagnostic (no equality contract) | 14.34 µs | 95% CI: 13.88 µs–14.99 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/text_extract` | `diagnostic` | diagnostic (no equality contract) | 14.71 µs | 95% CI: 14.63 µs–14.79 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/wildcard_all` | `diagnostic` | diagnostic (no equality contract) | 11.91 µs | 95% CI: 11.89 µs–11.95 µs | — | 1 |
| `diagnostic/fhp-simd/simd_bench/dispatch_lookup/warm_once_lock` | `diagnostic` | diagnostic (no equality contract) | 0.84 ns | 95% CI: 0.81 ns–0.89 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/dispatch/1024` | `diagnostic` | diagnostic (no equality contract) | 3.59 ns | 95% CI: 3.58 ns–3.59 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/dispatch/64` | `diagnostic` | diagnostic (no equality contract) | 3.60 ns | 95% CI: 3.59 ns–3.61 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/dispatch/65536` | `diagnostic` | diagnostic (no equality contract) | 3.63 ns | 95% CI: 3.62 ns–3.64 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/scalar/1024` | `diagnostic` | diagnostic (no equality contract) | 1.82 ns | 95% CI: 1.81 ns–1.82 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/scalar/64` | `diagnostic` | diagnostic (no equality contract) | 1.84 ns | 95% CI: 1.82 ns–1.88 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/scalar/65536` | `diagnostic` | diagnostic (no equality contract) | 1.82 ns | 95% CI: 1.82 ns–1.83 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/parse/build/100kb` | `regression` | project-owned regression | 280.88 µs | 95% CI: 279.80 µs–282.14 µs | 391.44 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse/build/1kb` | `regression` | project-owned regression | 4.07 µs | 95% CI: 4.02 µs–4.14 µs | 346.64 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse/lifecycle/100kb` | `regression` | project-owned regression | 315.34 µs | 95% CI: 299.86 µs–334.85 µs | 348.65 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse/lifecycle/1kb` | `regression` | project-owned regression | 4.12 µs | 95% CI: 4.08 µs–4.20 µs | 341.74 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/build/100kb` | `regression` | project-owned regression | 297.98 µs | 95% CI: 294.70 µs–303.24 µs | 368.97 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/build/1kb` | `regression` | project-owned regression | 4.59 µs | 95% CI: 4.43 µs–4.81 µs | 306.79 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/lifecycle/100kb` | `regression` | project-owned regression | 304.39 µs | 95% CI: 297.85 µs–313.26 µs | 361.20 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/lifecycle/1kb` | `regression` | project-owned regression | 4.45 µs | 95% CI: 4.42 µs–4.48 µs | 316.71 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/build/100kb` | `regression` | project-owned regression | 296.58 µs | 95% CI: 291.97 µs–303.29 µs | 370.71 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/build/1kb` | `regression` | project-owned regression | 4.11 µs | 95% CI: 4.06 µs–4.20 µs | 343.22 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/lifecycle/100kb` | `regression` | project-owned regression | 362.49 µs | 95% CI: 338.65 µs–390.35 µs | 303.30 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/lifecycle/1kb` | `regression` | project-owned regression | 4.19 µs | 95% CI: 4.13 µs–4.29 µs | 336.69 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/build/100kb` | `regression` | project-owned regression | 309.70 µs | 95% CI: 293.20 µs–331.04 µs | 355.01 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/build/1kb` | `regression` | project-owned regression | 4.04 µs | 95% CI: 4.00 µs–4.12 µs | 348.71 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/lifecycle/100kb` | `regression` | project-owned regression | 295.68 µs | 95% CI: 285.02 µs–312.91 µs | 371.84 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/lifecycle/1kb` | `regression` | project-owned regression | 4.07 µs | 95% CI: 4.05 µs–4.09 µs | 346.27 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/class` | `regression` | project-owned regression | 118.54 ns | 95% CI: 116.40 ns–122.02 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/complex` | `regression` | project-owned regression | 394.35 ns | 95% CI: 393.48 ns–395.38 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/descendant` | `regression` | project-owned regression | 299.29 ns | 95% CI: 298.07 ns–300.64 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/tag_p` | `regression` | project-owned regression | 131.58 ns | 95% CI: 131.28 ns–131.90 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/class` | `regression` | project-owned regression | 11.83 µs | 95% CI: 11.72 µs–12.01 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/complex` | `regression` | project-owned regression | 11.99 µs | 95% CI: 11.94 µs–12.06 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/descendant` | `regression` | project-owned regression | 79.79 µs | 95% CI: 79.68 µs–79.92 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/tag_p` | `regression` | project-owned regression | 10.02 µs | 95% CI: 9.96 µs–10.09 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_1024` | `regression` | project-owned regression | 644.56 µs | 95% CI: 635.94 µs–655.35 µs | 170.57 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_64` | `regression` | project-owned regression | 649.84 µs | 95% CI: 636.90 µs–668.54 µs | 169.19 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_65536` | `regression` | project-owned regression | 631.29 µs | 95% CI: 624.70 µs–642.52 µs | 174.16 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_8192` | `regression` | project-owned regression | 630.65 µs | 95% CI: 623.90 µs–642.13 µs | 174.34 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_1024` | `regression` | project-owned regression | 628.07 µs | 95% CI: 622.88 µs–637.41 µs | 175.05 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_64` | `regression` | project-owned regression | 635.01 µs | 95% CI: 626.92 µs–646.25 µs | 173.14 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_65536` | `regression` | project-owned regression | 627.62 µs | 95% CI: 624.60 µs–631.48 µs | 175.18 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_8192` | `regression` | project-owned regression | 627.75 µs | 95% CI: 625.13 µs–630.76 µs | 175.14 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_1024` | `regression` | project-owned regression | 667.37 µs | 95% CI: 663.62 µs–671.61 µs | 164.74 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_64` | `regression` | project-owned regression | 1.02 ms | 95% CI: 997.65 µs–1.05 ms | 108.00 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_65536` | `regression` | project-owned regression | 621.05 µs | 95% CI: 619.70 µs–622.56 µs | 177.03 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_8192` | `regression` | project-owned regression | 644.88 µs | 95% CI: 634.87 µs–658.63 µs | 170.49 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_1024` | `regression` | project-owned regression | 668.07 µs | 95% CI: 663.25 µs–673.71 µs | 164.57 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_64` | `regression` | project-owned regression | 1.05 ms | 95% CI: 1.03 ms–1.08 ms | 104.59 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_65536` | `regression` | project-owned regression | 638.78 µs | 95% CI: 627.40 µs–656.83 µs | 172.12 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_8192` | `regression` | project-owned regression | 644.41 µs | 95% CI: 632.54 µs–664.84 µs | 170.61 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/traversal/depth_first` | `regression` | project-owned regression | 10.52 µs | 95% CI: 10.47 µs–10.58 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/traversal/text_content` | `regression` | project-owned regression | 47.99 µs | 95% CI: 46.83 µs–49.40 µs | — | 1 |
| `regression/fast-html-parser/profile_bench/entity_decode/dense_entities` | `regression` | project-owned regression | 18.73 µs | 95% CI: 17.65 µs–20.09 µs | 269.91 MiB/s | 1 |
| `regression/fast-html-parser/profile_bench/entity_decode/no_entities` | `regression` | project-owned regression | 285.72 ns | 95% CI: 283.26 ns–288.58 ns | 18.58 GiB/s | 1 |
| `regression/fast-html-parser/profile_bench/entity_decode/sparse_entities` | `regression` | project-owned regression | 13.31 µs | 95% CI: 13.15 µs–13.49 µs | 415.63 MiB/s | 1 |
| `regression/fhp-selector/selector_bench/chaining/compiled` | `regression` | project-owned regression | 26.94 µs | 95% CI: 26.34 µs–27.77 µs | — | 1 |
| `regression/fhp-selector/selector_bench/compile/class` | `regression` | project-owned regression | 173.98 ns | 95% CI: 165.47 ns–184.13 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/complex` | `regression` | project-owned regression | 467.93 ns | 95% CI: 464.23 ns–472.41 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/compound` | `regression` | project-owned regression | 279.57 ns | 95% CI: 218.98 ns–354.94 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/descendant` | `regression` | project-owned regression | 301.73 ns | 95% CI: 299.18 ns–304.80 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/id` | `regression` | project-owned regression | 108.84 ns | 95% CI: 108.01 ns–109.85 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/not` | `regression` | project-owned regression | 224.22 ns | 95% CI: 222.90 ns–225.79 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/nth_child` | `regression` | project-owned regression | 199.55 ns | 95% CI: 196.10 ns–203.83 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/tag` | `regression` | project-owned regression | 180.28 ns | 95% CI: 174.26 ns–186.73 ns | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/attr_equals` | `regression` | project-owned regression | 35.91 µs | 95% CI: 34.96 µs–37.57 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/attr_exists` | `regression` | project-owned regression | 27.07 µs | 95% CI: 25.73 µs–28.96 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/child` | `regression` | project-owned regression | 12.43 µs | 95% CI: 12.34 µs–12.54 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/class` | `regression` | project-owned regression | 11.87 µs | 95% CI: 11.78 µs–12.00 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/complex` | `regression` | project-owned regression | 77.41 µs | 95% CI: 76.63 µs–78.33 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/compound` | `regression` | project-owned regression | 13.48 µs | 95% CI: 13.11 µs–13.91 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/descendant` | `regression` | project-owned regression | 80.88 µs | 95% CI: 80.40 µs–81.46 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/first_child` | `regression` | project-owned regression | 13.44 µs | 95% CI: 12.43 µs–14.87 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/id` | `regression` | project-owned regression | 25.99 µs | 95% CI: 25.90 µs–26.10 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/not` | `regression` | project-owned regression | 12.82 µs | 95% CI: 12.68 µs–13.04 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/nth_child` | `regression` | project-owned regression | 13.23 µs | 95% CI: 12.79 µs–13.83 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/tag` | `regression` | project-owned regression | 10.09 µs | 95% CI: 9.98 µs–10.23 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/document_index_build` | `regression` | project-owned regression | 176.92 µs | 95% CI: 161.44 µs–193.62 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/find_by_class` | `regression` | project-owned regression | 73.34 µs | 95% CI: 65.41 µs–82.29 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/find_by_id` | `regression` | project-owned regression | 25.24 µs | 95% CI: 23.04 µs–27.71 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/find_by_tag` | `regression` | project-owned regression | 2.85 µs | 95% CI: 2.75 µs–2.97 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/absolute_path` | `regression` | project-owned regression | 196.46 ns | 95% CI: 190.94 ns–204.86 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/contains` | `regression` | project-owned regression | 117.80 ns | 95% CI: 117.07 ns–118.92 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/descendant_attr` | `regression` | project-owned regression | 91.25 ns | 95% CI: 85.50 ns–101.03 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/descendant_p` | `regression` | project-owned regression | 53.67 ns | 95% CI: 53.42 ns–54.02 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/position` | `regression` | project-owned regression | 71.79 ns | 95% CI: 71.07 ns–72.80 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/text_extract` | `regression` | project-owned regression | 71.51 ns | 95% CI: 70.65 ns–72.56 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/wildcard_all` | `regression` | project-owned regression | 7.60 ns | 95% CI: 7.57 ns–7.64 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/absolute_path` | `regression` | project-owned regression | 65.99 ns | 95% CI: 65.40 ns–66.71 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/contains` | `regression` | project-owned regression | 19.63 µs | 95% CI: 19.06 µs–20.35 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/descendant_attr` | `regression` | project-owned regression | 16.47 µs | 95% CI: 16.45 µs–16.48 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/descendant_p` | `regression` | project-owned regression | 13.44 µs | 95% CI: 13.37 µs–13.56 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/position` | `regression` | project-owned regression | 13.44 µs | 95% CI: 13.36 µs–13.53 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/text_extract` | `regression` | project-owned regression | 14.71 µs | 95% CI: 14.57 µs–14.87 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/wildcard_all` | `regression` | project-owned regression | 12.07 µs | 95% CI: 12.04 µs–12.10 µs | — | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/dispatch/1024` | `regression` | project-owned regression | 179.26 ns | 95% CI: 178.95 ns–179.61 ns | 5.32 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/dispatch/64` | `regression` | project-owned regression | 34.04 ns | 95% CI: 33.97 ns–34.14 ns | 1.75 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/dispatch/65536` | `regression` | project-owned regression | 9.69 µs | 95% CI: 9.67 µs–9.72 µs | 6.30 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/scalar/1024` | `regression` | project-owned regression | 819.12 ns | 95% CI: 815.63 ns–823.30 ns | 1.16 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/scalar/64` | `regression` | project-owned regression | 70.96 ns | 95% CI: 70.75 ns–71.25 ns | 860.13 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/scalar/65536` | `regression` | project-owned regression | 50.14 µs | 95% CI: 50.06 µs–50.23 µs | 1.22 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/dispatch/1024` | `regression` | project-owned regression | 383.89 ns | 95% CI: 364.06 ns–406.57 ns | 2.48 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/dispatch/64` | `regression` | project-owned regression | 20.69 ns | 95% CI: 20.63 ns–20.77 ns | 2.88 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/dispatch/65536` | `regression` | project-owned regression | 22.33 µs | 95% CI: 21.85 µs–23.01 µs | 2.73 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/scalar/1024` | `regression` | project-owned regression | 2.18 µs | 95% CI: 2.02 µs–2.39 µs | 448.12 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/scalar/64` | `regression` | project-owned regression | 133.25 ns | 95% CI: 130.34 ns–136.98 ns | 458.06 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/scalar/65536` | `regression` | project-owned regression | 123.06 µs | 95% CI: 122.60 µs–123.58 µs | 507.90 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/no_match/1024` | `regression` | project-owned regression | 107.57 ns | 95% CI: 106.14 ns–110.01 ns | 8.87 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/no_match/64` | `regression` | project-owned regression | 8.40 ns | 95% CI: 8.35 ns–8.47 ns | 7.09 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/no_match/65536` | `regression` | project-owned regression | 7.49 µs | 95% CI: 6.94 µs–8.20 µs | 8.15 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/tail_match/1024` | `regression` | project-owned regression | 108.70 ns | 95% CI: 108.11 ns–109.31 ns | 8.77 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/tail_match/64` | `regression` | project-owned regression | 8.22 ns | 95% CI: 8.21 ns–8.24 ns | 7.25 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/tail_match/65536` | `regression` | project-owned regression | 6.73 µs | 95% CI: 6.71 µs–6.75 µs | 9.07 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/no_match/1024` | `regression` | project-owned regression | 494.94 ns | 95% CI: 493.51 ns–496.57 ns | 1.93 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/no_match/64` | `regression` | project-owned regression | 30.25 ns | 95% CI: 29.45 ns–31.43 ns | 1.97 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/no_match/65536` | `regression` | project-owned regression | 31.26 µs | 95% CI: 31.11 µs–31.45 µs | 1.95 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/tail_match/1024` | `regression` | project-owned regression | 552.07 ns | 95% CI: 539.92 ns–566.29 ns | 1.73 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/tail_match/64` | `regression` | project-owned regression | 31.14 ns | 95% CI: 30.17 ns–32.39 ns | 1.91 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/tail_match/65536` | `regression` | project-owned regression | 31.09 µs | 95% CI: 30.75 µs–31.50 µs | 1.96 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/dispatch/1024` | `regression` | project-owned regression | 98.08 ns | 95% CI: 93.47 ns–105.35 ns | 9.72 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/dispatch/64` | `regression` | project-owned regression | 7.12 ns | 95% CI: 6.94 ns–7.34 ns | 8.37 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/dispatch/65536` | `regression` | project-owned regression | 6.16 µs | 95% CI: 6.05 µs–6.35 µs | 9.90 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/scalar/1024` | `regression` | project-owned regression | 498.47 ns | 95% CI: 497.20 ns–499.84 ns | 1.91 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/scalar/64` | `regression` | project-owned regression | 45.98 ns | 95% CI: 44.14 ns–48.15 ns | 1.30 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/scalar/65536` | `regression` | project-owned regression | 42.84 µs | 95% CI: 39.32 µs–46.74 µs | 1.42 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/extract_tokens/100000` | `regression` | project-owned regression | 415.96 µs | 95% CI: 401.52 µs–439.29 µs | 229.52 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/extract_tokens/1024` | `regression` | project-owned regression | 5.32 µs | 95% CI: 5.03 µs–5.69 µs | 215.02 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/extract_tokens/5000000` | `regression` | project-owned regression | 21.31 ms | 95% CI: 21.18 ms–21.48 ms | 223.75 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/structural_index/100000` | `regression` | project-owned regression | 48.76 µs | 95% CI: 47.39 µs–50.49 µs | 1.91 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/structural_index/1024` | `regression` | project-owned regression | 682.91 ns | 95% CI: 659.85 ns–710.28 ns | 1.64 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/structural_index/5000000` | `regression` | project-owned regression | 2.39 ms | 95% CI: 2.37 ms–2.42 ms | 1.95 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e/100000` | `regression` | project-owned regression | 461.25 µs | 95% CI: 451.97 µs–472.21 µs | 206.98 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e/1024` | `regression` | project-owned regression | 5.80 µs | 95% CI: 5.71 µs–5.90 µs | 197.35 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e/5000000` | `regression` | project-owned regression | 24.79 ms | 95% CI: 24.24 ms–25.59 ms | 192.36 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/build/large_5mb` | `regression` | project-owned regression | 112.70 ms | 95% CI: 102.65 ms–123.24 ms | 45.66 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/build/medium_100kb` | `regression` | project-owned regression | 2.47 ms | 95% CI: 2.05 ms–2.90 ms | 44.48 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/build/small_1kb` | `regression` | project-owned regression | 4.10 µs | 95% CI: 4.06 µs–4.18 µs | 343.54 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/lifecycle/large_5mb` | `regression` | project-owned regression | 12.88 ms | 95% CI: 12.83 ms–12.93 ms | 399.61 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/lifecycle/medium_100kb` | `regression` | project-owned regression | 2.54 ms | 95% CI: 2.01 ms–3.10 ms | 43.34 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/lifecycle/small_1kb` | `regression` | project-owned regression | 13.23 µs | 95% CI: 9.99 µs–16.72 µs | 106.50 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/traversal/breadth_first` | `regression` | project-owned regression | 18.41 µs | 95% CI: 17.66 µs–19.40 µs | — | 1 |
| `regression/fhp-tree/tree_bench/traversal/depth_first` | `regression` | project-owned regression | 12.33 µs | 95% CI: 11.58 µs–13.19 µs | — | 1 |
| `regression/fhp-tree/tree_bench/traversal/text_content` | `regression` | project-owned regression | 47.03 µs | 95% CI: 46.49 µs–47.64 µs | — | 1 |

## Contract-equal ratios

Ratios are emitted only when the benchmark ID contains the explicit `contract_equal` contract marker. Values above 1× mean FHP completed the same checked workload faster. Each value is formed inside one independent run before the three run-local ratios are summarized.

| Contract-equal group | Competitor | Median competitor/FHP | Run range | Runs |
|---|---|---:|---:|---:|
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/compile` | `tl` | 0.086× | 0.079×–0.091× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/evaluate_materialized` | `tl` | 2.096× | 1.476×–2.129× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/compile` | `tl` | 0.063× | 0.061×–0.065× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/evaluate_materialized` | `tl` | 1.228× | 1.206×–1.232× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/build` | `scraper` | 7.062× | 6.872×–7.498× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle` | `scraper` | 5.787× | 4.197×–7.275× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/build` | `scraper` | 5.603× | 5.217×–5.623× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle` | `scraper` | 5.775× | 5.756×–5.958× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/build` | `scraper` | 5.298× | 5.164×–6.076× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle` | `scraper` | 5.763× | 5.703×–6.040× | 3 |

## Commands

```text
$ cargo bench --locked -p fhp-simd --bench simd_bench --no-default-features -- --noplot --quiet
$ cargo bench --locked -p fhp-tokenizer --bench tokenizer_bench --no-default-features --features entity-decode -- --noplot --quiet
$ cargo bench --locked -p fhp-tree --bench tree_bench --no-default-features --features encoding,entity-decode -- --noplot --quiet
$ cargo bench --locked -p fhp-selector --bench selector_bench --no-default-features -- --noplot --quiet
$ cargo bench --locked -p fhp-selector --bench xpath_bench --no-default-features -- --noplot --quiet
$ cargo bench --locked -p fast-html-parser --bench e2e_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet
$ cargo bench --locked -p fast-html-parser --bench e2e_bench --no-default-features --features css-selector,encoding,entity-decode,async-tokio -- --noplot --quiet streaming/async
$ cargo bench --locked -p fast-html-parser --bench profile_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet
$ cargo bench --locked -p fast-html-parser --bench comparison_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet  # FHP_BENCH_ORDER=fhp-first
$ cargo bench --locked -p fast-html-parser --bench realworld_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet  # FHP_BENCH_ORDER=fhp-first
$ cargo bench --locked -p fast-html-parser --bench comparison_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet  # FHP_BENCH_ORDER=fhp-middle
$ cargo bench --locked -p fast-html-parser --bench realworld_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet  # FHP_BENCH_ORDER=fhp-middle
$ cargo bench --locked -p fast-html-parser --bench comparison_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet  # FHP_BENCH_ORDER=fhp-last
$ cargo bench --locked -p fast-html-parser --bench realworld_bench --no-default-features --features css-selector,encoding,entity-decode -- --noplot --quiet  # FHP_BENCH_ORDER=fhp-last
```

Raw Criterion samples and reports remain machine-local under `target/criterion/`.
