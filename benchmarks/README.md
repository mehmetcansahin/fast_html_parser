# Benchmark results

Criterion's raw samples and HTML reports are machine-local and remain under
`target/criterion/`. This directory stores only compact, reviewable summaries
created by the repository benchmark runner.

```bash
python3 scripts/bench.py verify
python3 scripts/bench.py quick
python3 scripts/bench.py save main
python3 scripts/bench.py compare main
python3 scripts/bench.py publish
```

## Latest results

- [Full performance report](results/2026-07-13-fd2d1b6e846f-aarch64-apple-darwin.md)
  contains absolute estimates, three-run ranges, and contract-equal ratios for
  source digest `fd2d1b6e846f` on Apple M1.
- [Local baseline repeatability report](results/2026-07-13-local-baseline-repeatability.md)
  records a same-source `save`/`compare` experiment and targeted reruns. It
  documents measurement stability; it is not a cross-parser speed report.

The generated table in the repository [README](../README.md#performance) is a
compact view of the full performance report. Stack Overflow and Wikipedia DOM
results remain absolute-only because their observable signatures did not match
the competing parser; no speed ratio is inferred for those fixtures.

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
only those cross-parser ratios whose observable-result contract passed. Raw
Criterion data is deliberately not committed.
