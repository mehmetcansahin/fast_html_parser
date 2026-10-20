# Benchmark results

Criterion's raw samples and HTML reports are machine-local and remain under
`target/criterion/`. This directory stores only compact, reviewable summaries
created by the repository benchmark runner.

Official reports use metadata schema 3, explicit production SIMD features, a
hashed semantic contract from `contracts.json`, all six FHP/scraper/tl
permutations, and a tracked JSON provenance sidecar. Older schema-2 reports,
including the dirty 2026-07-15 snapshot, remain historical and provisional;
they are never eligible for the latest link, baseline compatibility, or a
release gate.

```bash
python3 scripts/bench.py verify
python3 scripts/bench.py quick
python3 scripts/bench.py save main       # clean worktree required
python3 scripts/bench.py compare main
python3 scripts/bench.py publish
```

## Latest results

<!-- latest-benchmark:start -->
No official schema-3 v0.2 result is available. Publishing requires a clean,
user-approved commit and updates this block together with the root README.
<!-- latest-benchmark:end -->

Historical reports remain available for diagnosis:

- The [dirty 2026-07-15 snapshot](results/2026-07-15-b4fcf640b253-aarch64-apple-darwin.md)
  is explicitly provisional and excluded from latest/official comparisons.
- The [2026-07-13 pre-schema-3 report](results/2026-07-13-fd2d1b6e846f-aarch64-apple-darwin.md)
  predates the current semantic-contract and six-permutation requirements.
- The [local baseline repeatability report](results/2026-07-13-local-baseline-repeatability.md)
  documents same-source measurement stability rather than cross-parser speed.

To smoke-test baseline persistence and report parsing without a full run:

```bash
python3 scripts/bench.py save smoke --quick
python3 scripts/bench.py compare smoke --quick
```

Criterion quick mode uses very few samples and can report large changes even
on unchanged code. Its exit status verifies that the normal regression policy
is wired correctly; use the default full `save`/`compare` commands for an
actual performance decision.

`save` and `compare` use the same machine-local Criterion baseline. A compare
run is rejected when CPU, OS, Rust toolchain, target, feature matrix, or build
flags do not match the saved environment. Source and lockfile changes are
reported but are expected between a baseline and the code being evaluated.

Only benchmark IDs below `regression/` participate in the local regression
policy. A statistically significant slowdown of at least 5% fails the compare
command; significant changes from 2% to 5%, and noisy changes of at least 5%,
are warnings. Comparison and diagnostic benchmarks never affect the exit code.

A local failure is a regression candidate, not by itself proof of a code
regression. Same-source repeatability testing showed that sub-microsecond
compile benchmarks, async runtime measurements, and lifecycle/drop variants
can cross the threshold without an input change. Confirm failures with targeted
reruns and consult the repeatability report before making a performance claim.

Published summaries live in `results/`. They contain the source digest,
fixture digests, commands, machine/toolchain metadata, absolute estimates, and
only those cross-parser ratios whose observable-result contract and stability
checks passed. Each official Markdown report has a same-stem JSON provenance
sidecar. Raw Criterion data is deliberately not committed.

## Tokenizer comparison

The `fhp-tokenizer/comparison` harness
(`crates/fhp-tokenizer/benches/tokenizer_comparison_bench.rs`) measures
`fhp-tokenizer` against other Rust HTML tokenizers on the Hacker News, GitHub,
Stack Overflow and Wikipedia snapshots and on `large_5mb.html`. Its IDs live
under `comparison/fhp-tokenizer/tokenizer_comparison_bench/`, so they never
gate a regression compare.

The libraries do different amounts of work, so the raw throughput of all rows
is **not** a ranking. Each row is labelled by work class:

| Implementation | Work class | Count returned | What it does |
|---|---|---|---|
| `fhp_tokenize` | `full_token` | tokens | `tokenize`: every token in a `Vec` |
| `fhp_tokenize_with` | `full_token` | tokens | `tokenize_with`: one-shot callback |
| `fhp_stream_feed_64k` | `full_token` | tokens | `StreamTokenizer::feed`, 64 KiB chunks |
| `fhp_stream_feed_str_with_64k` | `full_token` | tokens | `StreamTokenizer::feed_str_with`, 64 KiB chunks |
| `html5gum_iterator` | `full_token` | items, including parse errors | html5gum 0.8 default iterator of owned tokens |
| `html5gum_callback` | `full_token` | events | html5gum 0.8 `CallbackEmitter`; start tags arrive as name, attribute and close events |
| `html5ever_tokenizer` | `full_token` | tokens, including parse errors and EOF | html5ever 0.40 tokenizer: a `StrTendril` copy of the input in a `BufferQueue`, fed to a counting `TokenSink` |
| `lol_html_noop_passthrough` | `lazy_rewriter` | output bytes | lol_html 2 `HtmlRewriter` with no handlers. **Less work:** a lazy pass-through that never tokenizes content |
| `lol_html_element_text_comment_handlers` | `lazy_rewriter` | handler calls | lol_html 2 with `element!("*")`, `doc_text!` and `doc_comments!`. **Less work:** attributes are parsed only on access and text is never entity-decoded |
| `tl_parse` | `nonspec_dom` | DOM nodes | tl 0.7 `parse`. A DOM-level reference whose parser **is not WHATWG-compliant** |

Standalone html5gum and html5ever tokenizers leave raw-text switching to a
tree builder. Both adapters therefore apply fhp-tokenizer's rules: `script` is
script data; `style`, `iframe`, `xmp`, `noembed` and `noframes` are raw text;
`title` and `textarea` are RCDATA; `plaintext` never ends; `noscript` stays
markup. html5gum's own `naively_switch_states` treats `noscript` as raw text and
would tokenize different content on the Stack Overflow and Wikipedia fixtures.

### Contract

The `tokenizer` section of `contracts.json` records, for every fixture and
implementation, the native count that the timed function returns. Full-token
implementations also record a canonical token-stream digest
(`fnv1a-128-length-prefixed-v1`). The stream contains lowercase start tags with
their attributes (first of each duplicate kept, sorted by name because html5gum
does not keep source order) and self-closing flag, lowercase end tags, decoded
text with adjacent runs merged, comments, and one marker per doctype. Parse
errors, EOF tokens and doctype identifiers are excluded. The digest comes from
the same adapter the benchmark times, called with a recording callback instead
of a counting one. lol_html runs must also reproduce the fixture byte for byte.
The harness checks all of this before any timing, as does
`cargo test -p fhp-tokenizer --test tokenizer_benchmark_contract`.

After an intended behavior or dependency change, print the observed entries
and update the contract in the same change:

```bash
cargo test -p fhp-tokenizer --test tokenizer_benchmark_contract -- --ignored --nocapture
```

### Benchmark groups

Each fixture registers two kinds of group:

- `…/tokenize/semantic_reference/<work class>/<implementation>` holds every
  implementation as an absolute estimate. These rows never form ratios.
- `…/tokenize/contract_equal/<api shape>/<implementation>` pairs one fhp entry
  point, named `fhp`, with each competitor that has the same API shape and
  the same checked-in token-stream digest. `push_callback` compares
  `tokenize_with` with html5gum's `CallbackEmitter` and html5ever's
  `TokenSink`. `token_values` compares `tokenize` with html5gum's iterator.
  A competitor whose digest differs drops out of the group automatically. These
  groups use flat sampling with a 10 s measurement time.

Only `contract_equal` groups produce competitor/FHP ratios in a published
report. lol_html, tl and the streaming fhp entry points are absolute only. The
root README summary keeps showing DOM results; tokenizer rows and ratios
appear in the full report.

### Running

`python3 scripts/bench.py verify` executes every tokenizer benchmark once.
`python3 scripts/bench.py quick` runs the full harness in Criterion quick mode
as a comparison smoke slice, in the default order, next to the regression
slices. Its estimates are local, unrecorded sanity checks. `save --quick` and
`compare --quick` stay regression-only.

`publish` treats the harness like the DOM comparisons: it runs all six
`FHP_BENCH_ORDER` permutations, with html5gum in the scraper position and
html5ever in the tl position, and keeps lol_html and tl after them. Each
permutation measures 75 IDs, 25 of them with the 10 s contract-equal
measurement time. By Criterion's settings this harness therefore adds roughly
75 minutes to each publication attempt.
