# Local baseline repeatability report

This report evaluates the repeatability of the machine-local Criterion
`save`/`compare` workflow. It is not a new cross-parser performance
publication. The published absolute estimates and contract-equal ratios remain
in [the full benchmark report](2026-07-13-fd2d1b6e846f-aarch64-apple-darwin.md).

## Reproducibility metadata

| Field | Value |
|---|---|
| Baseline | `detailed-20260713-1430` |
| Baseline captured | `2026-07-13T11:29:46+00:00` |
| Comparison captured | `2026-07-13T12:18:42+00:00` |
| Source digest | `fd2d1b6e846fc21d23dfb84e3a4de0cbf6c2286804309ea922dfc231a915819c` |
| Fixture manifest digest | `273aaf25eb2d36b5fcefb89d507a4cff68cb6030093df4d7eca7adad171710c8` |
| Cargo.lock SHA-256 | `e0d9ba342be8f0be16d90832427762e55e291ce7be92d7dbbbee4d3796ac549b` |
| Git commit | `d88aa0a6a7367f179d649f9ebb2a401019a51b60` (dirty) |
| Target | `aarch64-apple-darwin` |
| CPU | Apple M1 (`arm64`) |
| OS | Darwin 25.5.0 |
| rustc / Cargo | `1.93.0` / `1.93.0` |
| Criterion | `0.5.1`; 100 samples, 3 s warm-up, 5 s measurement, 95% confidence |
| Build environment | `CARGO_INCREMENTAL=0`; `RUSTFLAGS="-C target-cpu=native"` |

The baseline and comparison used the same source, lockfile, fixtures, target,
feature matrix, compiler, and build flags. All 10 harnesses were covered: 292
saved benchmark IDs, 292 current IDs, and 292 change estimates, with no missing
regression baseline or change record.

## Commands

The full back-to-back run used:

```bash
python3 scripts/bench.py save detailed-20260713-1430
python3 scripts/bench.py compare detailed-20260713-1430
```

Failing clusters were then rerun against the same immutable baseline with the
same environment. Targeted runs used this command shape, with the matching
package, harness, features, `CRITERION_HOME`, and benchmark filter substituted:

```bash
env CARGO_INCREMENTAL=0 \
  RUSTFLAGS="-C target-cpu=native" \
  CRITERION_HOME="target/criterion/harnesses/<harness>" \
  cargo bench --locked -p <package> --bench <bench> \
  --no-default-features <features> -- \
  --baseline-lenient detailed-20260713-1430 --noplot <filter>
```

The targeted filters intentionally changed workload order. They test whether a
full-run failure survives isolation; they do not form a second publish result.

## Full comparison outcome

| Decision | Count | Meaning |
|---|---:|---|
| Fail | 14 | `regression/**`, mean slowdown at least 5%, CI lower bound above zero |
| Warn | 23 | Significant 2–5% slowdown or noisy 5%+ slowdown |
| Pass | 95 | Below the local regression warning policy |
| Info | 160 | Comparison or diagnostic namespace; never gated |

The command exited with status 1. Because the source and build inputs were
unchanged, the 14 failures are same-source false positives rather than code
regressions.

## Targeted repeat results

Percentages are Criterion mean-change estimates against the original saved
baseline. “Pass” means the project policy no longer classified the result as a
failure.

| Benchmark or cluster | Full compare | Targeted repeat | Third measurement | Interpretation |
|---|---:|---:|---:|---|
| Selector `compile/nth_child` | +6.15% fail | +7.09% fail | +10.13% fail | Persistent baseline offset in a ~190 ns microbenchmark |
| Tree `parse/build/small_1kb` | +11.19% fail | +7.94% fail | +5.04% fail | Persistent baseline offset for the smallest tree-build workload |
| Tokenizer `tokenize_e2e/5000000` | +5.58% fail | +11.51% fail | +0.84% pass | Sensitive to run order or machine state |
| Auto-encoding `lifecycle/100kb` | +15.33% fail | +8.54% fail | -0.03% pass | Lifecycle/deallocation result was not repeatable |
| Async streaming 64 KiB, build | +15.47% fail | +4.28% pass | — | Tokio/runtime scheduling noise |
| Async streaming 64 KiB, lifecycle | +50.66% fail | +3.99% pass | — | Initial confidence interval was extremely wide |
| SIMD classify dispatch, 1 KiB | +6.38% fail | +1.56% pass | — | Initial failure did not repeat |
| XPath compile/evaluate | 6 initial failures | All passed | — | Nanosecond compile and small evaluation results were order-sensitive |

Only two of the 14 initial failures remained failures through three
measurements. Since those two also used identical source and build inputs, they
show that a saved baseline can retain a systematic offset for a micro-workload;
they do not establish a code regression.

## Interpretation policy

- `compare` continues to return a nonzero exit status when its documented
  threshold is crossed. Treat that result as a regression candidate.
- Confirm a candidate with at least two targeted reruns before attributing it
  to a code change. Very short compile benchmarks, async runtime measurements,
  and lifecycle/drop variants require particular caution.
- If a candidate persists, repeat the full save/compare experiment under a
  controlled machine state or aggregate multiple independent baseline and
  comparison runs before making a performance claim.
- Do not copy local compare percentages into public performance tables.
  Published ratios must come from the three-run, rotated, contract-equal
  publication workflow.

Raw Criterion samples and comparison JSON remain machine-local under
`target/criterion/`.
