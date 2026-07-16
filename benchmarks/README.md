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
