# Benchmark report

Generated at `2026-07-13T08:22:44+00:00`.

## Run continuity

Measurement began at `2026-07-11T12:58:47+00:00`. After an interrupted local session, the eight complete non-order-sensitive harness results were retained and both comparison harnesses were rerun for all three parser order rotations. Source, lockfile, fixture, target, and environment contracts were revalidated before publication.

## Reproducibility metadata

| Field | Value |
|---|---|
| Source digest | `fd2d1b6e846fc21d23dfb84e3a4de0cbf6c2286804309ea922dfc231a915819c` |
| Fixture manifest digest | `273aaf25eb2d36b5fcefb89d507a4cff68cb6030093df4d7eca7adad171710c8` |
| Git commit | `d88aa0a6a7367f179d649f9ebb2a401019a51b60` (dirty) |
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
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 298.94 µs | run range: 292.55 µs–316.22 µs | 367.78 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 1.72 ms | run range: 1.69 ms–2.78 ms | 63.87 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 332.12 µs | run range: 277.27 µs–427.06 µs | 331.04 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 1.94 ms | run range: 1.67 ms–2.86 ms | 56.70 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 317.24 µs | run range: 273.47 µs–622.25 µs | 346.57 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 177.07 µs | run range: 163.54 µs–279.93 µs | 620.92 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 310.12 µs | run range: 273.54 µs–572.82 µs | 354.53 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 185.72 µs | run range: 176.73 µs–186.51 µs | 592.00 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 409.81 µs | run range: 408.56 µs–476.11 µs | 268.29 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 175.50 µs | run range: 175.29 µs–279.27 µs | 626.48 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 183.74 µs | run range: 178.80 µs–242.36 µs | 598.39 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/compile/fast_html_parser` | `comparison` | contract-equal | 111.73 ns | run range: 109.90 ns–114.19 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/compile/tl` | `comparison` | contract-equal | 10.19 ns | run range: 9.93 ns–10.45 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/evaluate_materialized/fast_html_parser` | `comparison` | contract-equal | 12.51 µs | run range: 12.50 µs–12.96 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/evaluate_materialized/tl` | `comparison` | contract-equal | 26.22 µs | run range: 26.00 µs–27.78 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 124.10 ns | run range: 111.56 ns–152.02 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 127.65 ns | run range: 119.51 ns–157.10 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 10.72 ns | run range: 10.14 ns–16.08 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 12.97 µs | run range: 12.74 µs–16.30 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 13.63 µs | run range: 13.09 µs–20.38 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 26.87 µs | run range: 26.38 µs–27.58 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 292.84 ns | run range: 285.00 ns–301.77 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 182.47 ns | run range: 170.53 ns–190.70 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 45.98 ns | run range: 45.32 ns–47.33 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 86.37 µs | run range: 79.63 µs–89.83 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 11.47 µs | run range: 11.40 µs–11.48 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/descendant_div_p/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 8.74 µs | run range: 8.66 µs–8.91 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/compile/fast_html_parser` | `comparison` | contract-equal | 140.36 ns | run range: 126.02 ns–146.09 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/compile/tl` | `comparison` | contract-equal | 8.23 ns | run range: 8.13 ns–8.71 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/evaluate_materialized/fast_html_parser` | `comparison` | contract-equal | 10.61 µs | run range: 9.85 µs–11.97 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/evaluate_materialized/tl` | `comparison` | contract-equal | 12.90 µs | run range: 12.56 µs–13.11 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 130.47 ns | run range: 129.57 ns–137.93 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 95.09 ns | run range: 89.45 ns–115.58 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 8.28 ns | run range: 8.10 ns–8.67 ns | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 10.83 µs | run range: 10.20 µs–10.95 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 11.51 µs | run range: 11.40 µs–11.63 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 13.08 µs | run range: 12.57 µs–13.35 µs | — | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/build/fast_html_parser` | `comparison` | contract-equal | 4.31 µs | run range: 4.01 µs–4.40 µs | 327.18 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/build/scraper` | `comparison` | contract-equal | 32.35 µs | run range: 28.98 µs–32.55 µs | 43.57 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/fast_html_parser` | `comparison` | contract-equal | 4.16 µs | run range: 4.08 µs–4.45 µs | 338.60 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/scraper` | `comparison` | contract-equal | 33.10 µs | run range: 29.85 µs–33.11 µs | 42.59 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.05 µs | run range: 3.98 µs–4.28 µs | 348.44 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 28.29 µs | run range: 27.92 µs–28.68 µs | 49.82 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.23 µs | run range: 4.11 µs–4.40 µs | 333.08 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 29.62 µs | run range: 28.99 µs–31.79 µs | 47.58 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 3.99 µs | run range: 3.99 µs–4.10 µs | 353.31 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 3.44 µs | run range: 3.37 µs–8.00 µs | 410.28 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 4.27 µs | run range: 4.02 µs–4.31 µs | 330.23 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 3.84 µs | run range: 3.82 µs–6.31 µs | 366.79 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 7.62 µs | run range: 7.45 µs–7.66 µs | 185.10 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 3.51 µs | run range: 3.37 µs–4.39 µs | 401.61 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 4.57 µs | run range: 3.69 µs–4.76 µs | 308.71 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 13.88 ms | run range: 12.85 ms–15.42 ms | 370.70 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 84.08 ms | run range: 75.93 ms–86.56 ms | 61.20 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 13.09 ms | run range: 12.74 ms–14.01 ms | 393.17 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 83.53 ms | run range: 80.11 ms–92.51 ms | 61.60 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 12.81 ms | run range: 12.51 ms–14.61 ms | 401.80 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 10.08 ms | run range: 8.75 ms–10.71 ms | 510.63 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 12.28 ms | run range: 12.26 ms–13.25 ms | 419.10 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 11.71 ms | run range: 11.22 ms–12.42 ms | 439.25 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 17.09 ms | run range: 16.86 ms–17.91 ms | 301.06 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 11.28 ms | run range: 9.16 ms–12.69 ms | 456.10 MiB/s | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/5mb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 12.09 ms | run range: 11.02 ms–13.53 ms | 425.53 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/build/fast_html_parser` | `comparison` | contract-equal | 477.74 µs | run range: 467.62 µs–482.67 µs | 601.04 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/build/scraper` | `comparison` | contract-equal | 2.67 ms | run range: 2.65 ms–2.81 ms | 107.40 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/fast_html_parser` | `comparison` | contract-equal | 467.27 µs | run range: 466.51 µs–487.25 µs | 614.51 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/scraper` | `comparison` | contract-equal | 2.71 ms | run range: 2.70 ms–3.21 ms | 105.83 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 506.15 µs | run range: 485.71 µs–524.25 µs | 567.31 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 2.68 ms | run range: 2.63 ms–2.68 ms | 107.14 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 469.82 µs | run range: 465.54 µs–477.41 µs | 611.17 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 2.71 ms | run range: 2.71 ms–2.80 ms | 105.77 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 457.62 µs | run range: 456.54 µs–525.96 µs | 627.47 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 262.83 µs | run range: 255.87 µs–265.47 µs | 1.07 GiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 476.56 µs | run range: 457.87 µs–477.88 µs | 602.53 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 277.19 µs | run range: 276.74 µs–281.77 µs | 1.01 GiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 373.13 µs | run range: 373.06 µs–389.24 µs | 769.57 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 267.37 µs | run range: 259.76 µs–271.02 µs | 1.05 GiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 284.05 µs | run range: 280.99 µs–286.25 µs | 1010.89 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/build/fast_html_parser` | `comparison` | contract-equal | 116.22 µs | run range: 115.31 µs–126.80 µs | 281.34 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/build/scraper` | `comparison` | contract-equal | 651.46 µs | run range: 632.73 µs–674.29 µs | 50.19 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/fast_html_parser` | `comparison` | contract-equal | 116.37 µs | run range: 114.28 µs–119.96 µs | 280.97 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle/scraper` | `comparison` | contract-equal | 686.55 µs | run range: 675.96 µs–690.05 µs | 47.62 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 114.33 µs | run range: 112.90 µs–125.26 µs | 285.98 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 644.13 µs | run range: 643.41 µs–658.85 µs | 50.76 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 117.00 µs | run range: 113.75 µs–135.12 µs | 279.45 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 688.67 µs | run range: 683.07 µs–728.75 µs | 47.48 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 112.50 µs | run range: 111.71 µs–133.30 µs | 290.62 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 81.01 µs | run range: 71.66 µs–87.11 µs | 403.58 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 114.42 µs | run range: 112.36 µs–125.46 µs | 285.76 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 81.90 µs | run range: 78.72 µs–91.54 µs | 399.20 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 225.29 µs | run range: 219.75 µs–238.22 µs | 145.13 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 80.32 µs | run range: 76.84 µs–87.64 µs | 407.09 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 80.33 µs | run range: 78.82 µs–84.78 µs | 407.01 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 928.81 µs | run range: 926.67 µs–986.47 µs | 426.21 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 4.79 ms | run range: 4.71 ms–5.11 ms | 82.56 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 928.01 µs | run range: 927.59 µs–978.73 µs | 426.58 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 4.88 ms | run range: 4.85 ms–4.88 ms | 81.13 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 916.44 µs | run range: 915.74 µs–948.33 µs | 431.96 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 612.98 µs | run range: 592.88 µs–622.37 µs | 645.81 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 944.66 µs | run range: 936.39 µs–1.11 ms | 419.06 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 679.19 µs | run range: 658.04 µs–688.77 µs | 582.85 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 905.28 µs | run range: 897.17 µs–1.15 ms | 437.29 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 606.04 µs | run range: 598.81 µs–610.00 µs | 653.20 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/stackoverflow_415kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 665.69 µs | run range: 657.45 µs–1.20 ms | 594.67 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.80 ms | run range: 1.79 ms–2.04 ms | 312.19 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/build/scraper` | `comparison` | semantic-reference (absolute) | 8.08 ms | run range: 7.92 ms–10.59 ms | 69.62 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.79 ms | run range: 1.79 ms–1.83 ms | 313.86 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/dom/lifecycle/scraper` | `comparison` | semantic-reference (absolute) | 8.52 ms | run range: 8.27 ms–9.10 ms | 66.03 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/build/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.76 ms | run range: 1.76 ms–1.87 ms | 319.69 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/build/tl` | `comparison` | semantic-reference (absolute) | 1.19 ms | run range: 1.19 ms–1.31 ms | 471.58 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/lifecycle/fast_html_parser` | `comparison` | semantic-reference (absolute) | 1.77 ms | run range: 1.76 ms–2.00 ms | 317.53 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/owned/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 1.37 ms | run range: 1.34 ms–1.40 ms | 410.60 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/streaming/lifecycle/lol_html_noop_rewrite` | `comparison` | semantic-reference (absolute) | 2.28 ms | run range: 2.28 ms–2.38 ms | 246.22 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/zero_copy/build/tl` | `comparison` | semantic-reference (absolute) | 1.21 ms | run range: 1.21 ms–1.22 ms | 464.12 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/parse/semantic_reference/zero_copy/lifecycle/tl` | `comparison` | semantic-reference (absolute) | 1.41 ms | run range: 1.36 ms–1.41 ms | 399.68 MiB/s | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 163.55 ns | run range: 161.54 ns–163.62 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 157.26 ns | run range: 155.80 ns–158.44 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 54.64 ns | run range: 53.26 ns–55.58 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 84.90 µs | run range: 82.49 µs–92.80 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 82.87 µs | run range: 79.99 µs–87.28 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/class_mw_body/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 113.57 µs | run range: 105.28 µs–114.76 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 297.45 ns | run range: 292.37 ns–301.65 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 168.19 ns | run range: 168.08 ns–177.16 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 50.72 ns | run range: 50.27 ns–51.52 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 546.25 µs | run range: 518.77 µs–810.98 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 81.33 µs | run range: 79.15 µs–86.04 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/descendant_table_td/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 38.68 µs | run range: 37.42 µs–41.71 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/compile/fast_html_parser` | `comparison` | semantic-reference (absolute) | 151.40 ns | run range: 151.19 ns–158.73 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/compile/scraper` | `comparison` | semantic-reference (absolute) | 197.71 ns | run range: 197.46 ns–199.91 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/compile/tl` | `comparison` | semantic-reference (absolute) | 49.10 ns | run range: 47.13 ns–53.39 ns | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/evaluate_materialized/fast_html_parser` | `comparison` | semantic-reference (absolute) | 168.39 µs | run range: 167.79 µs–173.42 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/evaluate_materialized/scraper` | `comparison` | semantic-reference (absolute) | 99.63 µs | run range: 94.50 µs–103.75 µs | — | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/selector/link_with_href/semantic_reference/evaluate_materialized/tl` | `comparison` | semantic-reference (absolute) | 121.62 µs | run range: 121.59 µs–187.38 µs | — | 3 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/class` | `diagnostic` | diagnostic (no equality contract) | 11.84 µs | 95% CI: 11.72 µs–12.05 µs | — | 1 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/complex` | `diagnostic` | diagnostic (no equality contract) | 12.89 µs | 95% CI: 11.93 µs–14.32 µs | — | 1 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/descendant` | `diagnostic` | diagnostic (no equality contract) | 81.46 µs | 95% CI: 79.76 µs–84.54 µs | — | 1 |
| `diagnostic/fast-html-parser/e2e_bench/select/string_convenience/tag_p` | `diagnostic` | diagnostic (no equality contract) | 9.88 µs | 95% CI: 9.86 µs–9.90 µs | — | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/01_simd_index` | `diagnostic` | diagnostic (no equality contract) | 54.27 µs | 95% CI: 52.89 µs–56.70 µs | 1.98 GiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/02_tokenize_vec` | `diagnostic` | diagnostic (no equality contract) | 313.64 µs | 95% CI: 304.49 µs–326.30 µs | 350.55 MiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/03_tokenize_with_noop` | `diagnostic` | diagnostic (no equality contract) | 263.44 µs | 95% CI: 257.57 µs–274.18 µs | 417.35 MiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/04_full_parse` | `diagnostic` | diagnostic (no equality contract) | 323.80 µs | 95% CI: 317.98 µs–329.59 µs | 339.54 MiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/05_tree_build_from_pretokenized` | `diagnostic` | diagnostic (no equality contract) | 85.34 µs | 95% CI: 83.09 µs–89.40 µs | 1.26 GiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/06_memcpy_100kb` | `diagnostic` | diagnostic (no equality contract) | 2.99 µs | 95% CI: 2.73 µs–3.37 µs | 35.92 GiB/s | 1 |
| `diagnostic/fast-html-parser/profile_bench/cost_100kb/07_tl_parse` | `diagnostic` | diagnostic (no equality contract) | 198.27 µs | 95% CI: 196.15 µs–200.88 µs | 554.53 MiB/s | 1 |
| `diagnostic/fhp-selector/selector_bench/string_convenience/string_class` | `diagnostic` | diagnostic (no equality contract) | 11.72 µs | 95% CI: 11.70 µs–11.75 µs | — | 1 |
| `diagnostic/fhp-selector/selector_bench/string_convenience/string_compound` | `diagnostic` | diagnostic (no equality contract) | 75.74 µs | 95% CI: 75.62 µs–75.88 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/absolute_path` | `diagnostic` | diagnostic (no equality contract) | 276.08 ns | 95% CI: 274.45 ns–278.45 ns | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/contains` | `diagnostic` | diagnostic (no equality contract) | 19.99 µs | 95% CI: 18.82 µs–22.11 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/descendant_attr` | `diagnostic` | diagnostic (no equality contract) | 16.63 µs | 95% CI: 16.41 µs–16.96 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/descendant_p` | `diagnostic` | diagnostic (no equality contract) | 13.41 µs | 95% CI: 13.32 µs–13.59 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/position` | `diagnostic` | diagnostic (no equality contract) | 13.58 µs | 95% CI: 13.36 µs–14.00 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/text_extract` | `diagnostic` | diagnostic (no equality contract) | 14.47 µs | 95% CI: 14.44 µs–14.51 µs | — | 1 |
| `diagnostic/fhp-selector/xpath_bench/string_convenience/wildcard_all` | `diagnostic` | diagnostic (no equality contract) | 11.99 µs | 95% CI: 11.94 µs–12.08 µs | — | 1 |
| `diagnostic/fhp-simd/simd_bench/dispatch_lookup/warm_once_lock` | `diagnostic` | diagnostic (no equality contract) | 0.77 ns | 95% CI: 0.77 ns–0.78 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/dispatch/1024` | `diagnostic` | diagnostic (no equality contract) | 3.62 ns | 95% CI: 3.61 ns–3.63 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/dispatch/64` | `diagnostic` | diagnostic (no equality contract) | 3.62 ns | 95% CI: 3.61 ns–3.63 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/dispatch/65536` | `diagnostic` | diagnostic (no equality contract) | 3.59 ns | 95% CI: 3.58 ns–3.60 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/scalar/1024` | `diagnostic` | diagnostic (no equality contract) | 1.83 ns | 95% CI: 1.83 ns–1.84 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/scalar/64` | `diagnostic` | diagnostic (no equality contract) | 1.83 ns | 95% CI: 1.82 ns–1.83 ns | — | 1 |
| `diagnostic/fhp-simd/simd_bench/find_delimiters_early_match/scalar/65536` | `diagnostic` | diagnostic (no equality contract) | 1.82 ns | 95% CI: 1.82 ns–1.83 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/parse/build/100kb` | `regression` | project-owned regression | 278.57 µs | 95% CI: 278.22 µs–278.95 µs | 394.68 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse/build/1kb` | `regression` | project-owned regression | 4.03 µs | 95% CI: 4.02 µs–4.04 µs | 349.99 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse/lifecycle/100kb` | `regression` | project-owned regression | 284.29 µs | 95% CI: 278.54 µs–295.57 µs | 386.74 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse/lifecycle/1kb` | `regression` | project-owned regression | 4.08 µs | 95% CI: 4.07 µs–4.09 µs | 345.50 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/build/100kb` | `regression` | project-owned regression | 285.33 µs | 95% CI: 285.13 µs–285.54 µs | 385.33 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/build/1kb` | `regression` | project-owned regression | 4.18 µs | 95% CI: 4.18 µs–4.19 µs | 336.83 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/lifecycle/100kb` | `regression` | project-owned regression | 286.39 µs | 95% CI: 286.21 µs–286.58 µs | 383.91 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_bytes/auto_encoding/lifecycle/1kb` | `regression` | project-owned regression | 4.23 µs | 95% CI: 4.23 µs–4.23 µs | 333.18 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/build/100kb` | `regression` | project-owned regression | 279.76 µs | 95% CI: 279.32 µs–280.28 µs | 392.99 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/build/1kb` | `regression` | project-owned regression | 3.99 µs | 95% CI: 3.99 µs–4.00 µs | 352.96 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/lifecycle/100kb` | `regression` | project-owned regression | 279.81 µs | 95% CI: 279.45 µs–280.20 µs | 392.93 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/borrow/lifecycle/1kb` | `regression` | project-owned regression | 4.04 µs | 95% CI: 4.04 µs–4.04 µs | 348.84 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/build/100kb` | `regression` | project-owned regression | 277.20 µs | 95% CI: 276.94 µs–277.48 µs | 396.63 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/build/1kb` | `regression` | project-owned regression | 3.96 µs | 95% CI: 3.95 µs–3.96 µs | 356.24 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/lifecycle/100kb` | `regression` | project-owned regression | 277.78 µs | 95% CI: 277.28 µs–278.29 µs | 395.80 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/parse_owned/owned/lifecycle/1kb` | `regression` | project-owned regression | 4.01 µs | 95% CI: 4.01 µs–4.02 µs | 351.18 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/class` | `regression` | project-owned regression | 116.67 ns | 95% CI: 115.89 ns–117.78 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/complex` | `regression` | project-owned regression | 401.43 ns | 95% CI: 400.67 ns–402.28 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/descendant` | `regression` | project-owned regression | 306.86 ns | 95% CI: 303.01 ns–311.90 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/compile/tag_p` | `regression` | project-owned regression | 128.44 ns | 95% CI: 128.11 ns–128.80 ns | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/class` | `regression` | project-owned regression | 12.01 µs | 95% CI: 11.85 µs–12.23 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/complex` | `regression` | project-owned regression | 12.07 µs | 95% CI: 11.95 µs–12.27 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/descendant` | `regression` | project-owned regression | 80.71 µs | 95% CI: 80.04 µs–81.84 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/select/evaluate/tag_p` | `regression` | project-owned regression | 10.00 µs | 95% CI: 9.93 µs–10.11 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_1024` | `regression` | project-owned regression | 671.58 µs | 95% CI: 650.93 µs–697.19 µs | 163.71 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_64` | `regression` | project-owned regression | 654.42 µs | 95% CI: 612.06 µs–728.38 µs | 168.01 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_65536` | `regression` | project-owned regression | 759.08 µs | 95% CI: 755.50 µs–762.64 µs | 144.84 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/build/chunk_8192` | `regression` | project-owned regression | 665.33 µs | 95% CI: 644.67 µs–693.51 µs | 165.25 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_1024` | `regression` | project-owned regression | 628.45 µs | 95% CI: 610.53 µs–659.10 µs | 174.95 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_64` | `regression` | project-owned regression | 713.61 µs | 95% CI: 693.41 µs–736.09 µs | 154.07 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_65536` | `regression` | project-owned regression | 620.14 µs | 95% CI: 607.50 µs–643.27 µs | 177.29 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/async/lifecycle/chunk_8192` | `regression` | project-owned regression | 633.66 µs | 95% CI: 615.58 µs–659.45 µs | 173.51 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_1024` | `regression` | project-owned regression | 654.63 µs | 95% CI: 653.29 µs–656.21 µs | 167.95 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_64` | `regression` | project-owned regression | 964.79 µs | 95% CI: 964.22 µs–965.40 µs | 113.96 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_65536` | `regression` | project-owned regression | 616.16 µs | 95% CI: 614.93 µs–617.59 µs | 178.44 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/build/chunk_8192` | `regression` | project-owned regression | 621.25 µs | 95% CI: 619.88 µs–623.04 µs | 176.97 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_1024` | `regression` | project-owned regression | 654.50 µs | 95% CI: 653.68 µs–655.45 µs | 167.98 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_64` | `regression` | project-owned regression | 964.04 µs | 95% CI: 963.38 µs–964.77 µs | 114.05 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_65536` | `regression` | project-owned regression | 630.86 µs | 95% CI: 624.51 µs–638.88 µs | 174.28 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/streaming/sync/lifecycle/chunk_8192` | `regression` | project-owned regression | 620.79 µs | 95% CI: 619.80 µs–622.04 µs | 177.11 MiB/s | 1 |
| `regression/fast-html-parser/e2e_bench/traversal/depth_first` | `regression` | project-owned regression | 10.38 µs | 95% CI: 10.17 µs–10.76 µs | — | 1 |
| `regression/fast-html-parser/e2e_bench/traversal/text_content` | `regression` | project-owned regression | 45.06 µs | 95% CI: 44.11 µs–46.72 µs | — | 1 |
| `regression/fast-html-parser/profile_bench/entity_decode/dense_entities` | `regression` | project-owned regression | 16.48 µs | 95% CI: 16.13 µs–17.06 µs | 306.72 MiB/s | 1 |
| `regression/fast-html-parser/profile_bench/entity_decode/no_entities` | `regression` | project-owned regression | 329.14 ns | 95% CI: 321.39 ns–338.38 ns | 16.13 GiB/s | 1 |
| `regression/fast-html-parser/profile_bench/entity_decode/sparse_entities` | `regression` | project-owned regression | 12.73 µs | 95% CI: 12.40 µs–13.30 µs | 434.49 MiB/s | 1 |
| `regression/fhp-selector/selector_bench/chaining/compiled` | `regression` | project-owned regression | 25.90 µs | 95% CI: 25.69 µs–26.21 µs | — | 1 |
| `regression/fhp-selector/selector_bench/compile/class` | `regression` | project-owned regression | 117.88 ns | 95% CI: 116.16 ns–120.69 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/complex` | `regression` | project-owned regression | 471.37 ns | 95% CI: 461.97 ns–484.29 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/compound` | `regression` | project-owned regression | 186.68 ns | 95% CI: 186.34 ns–187.07 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/descendant` | `regression` | project-owned regression | 303.42 ns | 95% CI: 299.55 ns–309.17 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/id` | `regression` | project-owned regression | 109.67 ns | 95% CI: 109.34 ns–110.06 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/not` | `regression` | project-owned regression | 223.31 ns | 95% CI: 222.84 ns–223.88 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/nth_child` | `regression` | project-owned regression | 191.03 ns | 95% CI: 189.79 ns–192.75 ns | — | 1 |
| `regression/fhp-selector/selector_bench/compile/tag` | `regression` | project-owned regression | 127.59 ns | 95% CI: 126.58 ns–129.39 ns | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/attr_equals` | `regression` | project-owned regression | 36.06 µs | 95% CI: 35.40 µs–36.79 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/attr_exists` | `regression` | project-owned regression | 25.66 µs | 95% CI: 25.48 µs–25.86 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/child` | `regression` | project-owned regression | 12.44 µs | 95% CI: 12.34 µs–12.58 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/class` | `regression` | project-owned regression | 11.77 µs | 95% CI: 11.74 µs–11.81 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/complex` | `regression` | project-owned regression | 76.59 µs | 95% CI: 76.25 µs–76.97 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/compound` | `regression` | project-owned regression | 12.50 µs | 95% CI: 12.38 µs–12.68 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/descendant` | `regression` | project-owned regression | 83.16 µs | 95% CI: 80.16 µs–88.56 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/first_child` | `regression` | project-owned regression | 12.55 µs | 95% CI: 12.45 µs–12.67 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/id` | `regression` | project-owned regression | 26.74 µs | 95% CI: 25.76 µs–28.55 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/not` | `regression` | project-owned regression | 12.76 µs | 95% CI: 12.66 µs–12.92 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/nth_child` | `regression` | project-owned regression | 12.41 µs | 95% CI: 12.34 µs–12.50 µs | — | 1 |
| `regression/fhp-selector/selector_bench/evaluate/tag` | `regression` | project-owned regression | 10.04 µs | 95% CI: 9.97 µs–10.11 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/document_index_build` | `regression` | project-owned regression | 85.72 µs | 95% CI: 82.62 µs–90.24 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/find_by_class` | `regression` | project-owned regression | 31.95 µs | 95% CI: 31.71 µs–32.31 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/find_by_id` | `regression` | project-owned regression | 11.23 µs | 95% CI: 11.06 µs–11.48 µs | — | 1 |
| `regression/fhp-selector/selector_bench/find/find_by_tag` | `regression` | project-owned regression | 2.54 µs | 95% CI: 2.52 µs–2.55 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/absolute_path` | `regression` | project-owned regression | 201.55 ns | 95% CI: 195.17 ns–210.87 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/contains` | `regression` | project-owned regression | 118.40 ns | 95% CI: 117.26 ns–120.25 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/descendant_attr` | `regression` | project-owned regression | 82.32 ns | 95% CI: 82.00 ns–82.74 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/descendant_p` | `regression` | project-owned regression | 53.42 ns | 95% CI: 53.35 ns–53.51 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/position` | `regression` | project-owned regression | 71.17 ns | 95% CI: 70.97 ns–71.38 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/text_extract` | `regression` | project-owned regression | 69.93 ns | 95% CI: 69.38 ns–70.65 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/compile/wildcard_all` | `regression` | project-owned regression | 7.68 ns | 95% CI: 7.60 ns–7.81 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/absolute_path` | `regression` | project-owned regression | 64.43 ns | 95% CI: 64.24 ns–64.66 ns | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/contains` | `regression` | project-owned regression | 18.79 µs | 95% CI: 18.52 µs–19.21 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/descendant_attr` | `regression` | project-owned regression | 16.33 µs | 95% CI: 16.32 µs–16.35 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/descendant_p` | `regression` | project-owned regression | 16.40 µs | 95% CI: 14.09 µs–20.71 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/position` | `regression` | project-owned regression | 13.21 µs | 95% CI: 13.20 µs–13.23 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/text_extract` | `regression` | project-owned regression | 14.36 µs | 95% CI: 14.26 µs–14.51 µs | — | 1 |
| `regression/fhp-selector/xpath_bench/evaluate/wildcard_all` | `regression` | project-owned regression | 11.89 µs | 95% CI: 11.87 µs–11.91 µs | — | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/dispatch/1024` | `regression` | project-owned regression | 180.14 ns | 95% CI: 179.68 ns–180.66 ns | 5.29 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/dispatch/64` | `regression` | project-owned regression | 31.26 ns | 95% CI: 31.23 ns–31.30 ns | 1.91 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/dispatch/65536` | `regression` | project-owned regression | 9.98 µs | 95% CI: 9.86 µs–10.12 µs | 6.12 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/scalar/1024` | `regression` | project-owned regression | 816.86 ns | 95% CI: 810.90 ns–827.77 ns | 1.17 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/scalar/64` | `regression` | project-owned regression | 68.57 ns | 95% CI: 68.51 ns–68.64 ns | 890.07 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/classify_bytes/scalar/65536` | `regression` | project-owned regression | 50.48 µs | 95% CI: 50.24 µs–50.81 µs | 1.21 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/dispatch/1024` | `regression` | project-owned regression | 341.98 ns | 95% CI: 336.98 ns–348.15 ns | 2.79 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/dispatch/64` | `regression` | project-owned regression | 20.55 ns | 95% CI: 20.49 ns–20.61 ns | 2.90 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/dispatch/65536` | `regression` | project-owned regression | 20.69 µs | 95% CI: 20.65 µs–20.73 µs | 2.95 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/scalar/1024` | `regression` | project-owned regression | 1.93 µs | 95% CI: 1.91 µs–1.96 µs | 505.55 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/scalar/64` | `regression` | project-owned regression | 126.16 ns | 95% CI: 125.82 ns–126.52 ns | 483.80 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/compute_all_masks/scalar/65536` | `regression` | project-owned regression | 129.68 µs | 95% CI: 125.00 µs–136.66 µs | 481.95 MiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/no_match/1024` | `regression` | project-owned regression | 112.64 ns | 95% CI: 106.52 ns–122.18 ns | 8.47 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/no_match/64` | `regression` | project-owned regression | 8.19 ns | 95% CI: 8.16 ns–8.22 ns | 7.28 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/no_match/65536` | `regression` | project-owned regression | 6.62 µs | 95% CI: 6.60 µs–6.64 µs | 9.23 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/tail_match/1024` | `regression` | project-owned regression | 107.53 ns | 95% CI: 104.92 ns–112.12 ns | 8.87 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/tail_match/64` | `regression` | project-owned regression | 8.12 ns | 95% CI: 8.10 ns–8.14 ns | 7.34 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/dispatch/tail_match/65536` | `regression` | project-owned regression | 6.62 µs | 95% CI: 6.61 µs–6.64 µs | 9.22 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/no_match/1024` | `regression` | project-owned regression | 487.92 ns | 95% CI: 487.11 ns–488.81 ns | 1.95 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/no_match/64` | `regression` | project-owned regression | 29.10 ns | 95% CI: 29.02 ns–29.19 ns | 2.05 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/no_match/65536` | `regression` | project-owned regression | 33.58 µs | 95% CI: 30.86 µs–38.26 µs | 1.82 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/tail_match/1024` | `regression` | project-owned regression | 497.99 ns | 95% CI: 481.80 ns–525.25 ns | 1.92 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/tail_match/64` | `regression` | project-owned regression | 31.80 ns | 95% CI: 30.25 ns–34.06 ns | 1.87 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/find_delimiters/scalar/tail_match/65536` | `regression` | project-owned regression | 31.75 µs | 95% CI: 31.35 µs–32.21 µs | 1.92 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/dispatch/1024` | `regression` | project-owned regression | 92.84 ns | 95% CI: 92.73 ns–92.97 ns | 10.27 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/dispatch/64` | `regression` | project-owned regression | 6.71 ns | 95% CI: 6.44 ns–7.19 ns | 8.89 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/dispatch/65536` | `regression` | project-owned regression | 6.00 µs | 95% CI: 5.96 µs–6.06 µs | 10.17 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/scalar/1024` | `regression` | project-owned regression | 493.51 ns | 95% CI: 492.84 ns–494.28 ns | 1.93 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/scalar/64` | `regression` | project-owned regression | 38.00 ns | 95% CI: 37.92 ns–38.10 ns | 1.57 GiB/s | 1 |
| `regression/fhp-simd/simd_bench/skip_whitespace/scalar/65536` | `regression` | project-owned regression | 31.33 µs | 95% CI: 30.11 µs–33.57 µs | 1.95 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/extract_tokens/100000` | `regression` | project-owned regression | 380.52 µs | 95% CI: 380.01 µs–381.09 µs | 250.89 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/extract_tokens/1024` | `regression` | project-owned regression | 4.66 µs | 95% CI: 4.64 µs–4.69 µs | 245.44 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/extract_tokens/5000000` | `regression` | project-owned regression | 20.61 ms | 95% CI: 20.55 ms–20.69 ms | 231.34 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/structural_index/100000` | `regression` | project-owned regression | 46.55 µs | 95% CI: 46.46 µs–46.66 µs | 2.00 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/structural_index/1024` | `regression` | project-owned regression | 581.97 ns | 95% CI: 580.28 ns–583.92 ns | 1.92 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/structural_index/5000000` | `regression` | project-owned regression | 2.32 ms | 95% CI: 2.32 ms–2.33 ms | 2.00 GiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e/100000` | `regression` | project-owned regression | 427.56 µs | 95% CI: 424.98 µs–432.18 µs | 223.29 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e/1024` | `regression` | project-owned regression | 5.22 µs | 95% CI: 5.22 µs–5.23 µs | 219.11 MiB/s | 1 |
| `regression/fhp-tokenizer/tokenizer_bench/tokenize_e2e/5000000` | `regression` | project-owned regression | 23.55 ms | 95% CI: 23.43 ms–23.68 ms | 202.50 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/build/large_5mb` | `regression` | project-owned regression | 12.62 ms | 95% CI: 12.60 ms–12.64 ms | 407.81 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/build/medium_100kb` | `regression` | project-owned regression | 282.03 µs | 95% CI: 281.26 µs–282.79 µs | 389.84 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/build/small_1kb` | `regression` | project-owned regression | 4.01 µs | 95% CI: 4.01 µs–4.02 µs | 351.17 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/lifecycle/large_5mb` | `regression` | project-owned regression | 12.60 ms | 95% CI: 12.58 ms–12.62 ms | 408.53 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/lifecycle/medium_100kb` | `regression` | project-owned regression | 283.48 µs | 95% CI: 282.09 µs–285.07 µs | 387.84 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/parse/lifecycle/small_1kb` | `regression` | project-owned regression | 4.13 µs | 95% CI: 4.09 µs–4.20 µs | 341.05 MiB/s | 1 |
| `regression/fhp-tree/tree_bench/traversal/breadth_first` | `regression` | project-owned regression | 16.08 µs | 95% CI: 16.05 µs–16.12 µs | — | 1 |
| `regression/fhp-tree/tree_bench/traversal/depth_first` | `regression` | project-owned regression | 10.39 µs | 95% CI: 10.32 µs–10.52 µs | — | 1 |
| `regression/fhp-tree/tree_bench/traversal/text_content` | `regression` | project-owned regression | 45.66 µs | 95% CI: 45.38 µs–46.03 µs | — | 1 |

## Contract-equal ratios

Ratios are emitted only when the benchmark ID contains the explicit `contract_equal` contract marker. Values above 1× mean FHP completed the same checked workload faster. Each value is formed inside one independent run before the three run-local ratios are summarized.

| Contract-equal group | Competitor | Median competitor/FHP | Run range | Runs |
|---|---|---:|---:|---:|
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/compile` | `tl` | 0.092× | 0.089×–0.093× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/class_card/contract_equal/fhp_tl/evaluate_materialized` | `tl` | 2.078× | 2.024×–2.223× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/compile` | `tl` | 0.060× | 0.059×–0.065× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/tag_p/contract_equal/fhp_tl/evaluate_materialized` | `tl` | 1.185× | 1.096×–1.310× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/build` | `scraper` | 7.352× | 7.225×–7.555× | 3 |
| `comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle` | `scraper` | 7.435× | 7.170×–8.107× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/build` | `scraper` | 5.669× | 5.596×–5.826× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/github_301kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle` | `scraper` | 5.816× | 5.776×–6.597× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/build` | `scraper` | 5.444× | 5.318×–5.649× | 3 |
| `comparison/fast-html-parser/realworld_bench/realworld/hackernews_34kb/parse/contract_equal/fhp_scraper_dom/dom/lifecycle` | `scraper` | 5.915× | 5.723×–5.930× | 3 |

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
