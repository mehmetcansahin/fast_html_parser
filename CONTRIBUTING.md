# Contributing to fast-html-parser

Thank you for your interest in contributing to fast-html-parser! This document provides guidelines and instructions for contributing.

## Prerequisites

- **Rust toolchain**: Minimum supported Rust version (MSRV) is **1.85** (edition 2024)
- **cargo-deny**: Required only for the maintainer release gate — `cargo install cargo-deny`
- **cargo-fuzz** and Rust nightly: Required only for the maintainer release gate
- Additional Rust targets: The release command reports the exact missing target
  installation command; it never installs prerequisites automatically

## Getting Started

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/<your-username>/fast_html_parser.git
   cd fast_html_parser
   ```
3. Create a new branch for your work:
   ```bash
   git checkout -b feat/my-feature
   ```

## Building

Build the entire workspace:

```bash
cargo build --workspace
```

Build with all features enabled:

```bash
cargo build --workspace --all-features
```

## Local Development Gate

Before opening or updating a pull request, run the repository-owned local gate:

```bash
python3 scripts/release.py check
```

It verifies formatting, strict Clippy, all-feature and scalar/SIMD feature
matrices, strict rustdoc, Python tooling tests, vendored entity generation,
license copies, and benchmark contracts. The repository intentionally does not
use GitHub Actions, so a successful local gate is required evidence.

For a focused iteration, the underlying commands can still be run directly.

## Focused Testing

Run the full test suite:

```bash
cargo test --workspace
```

Run tests including async/streaming features:

```bash
cargo test --workspace --all-features
```

## Code Quality

The local gate runs the following checks; use these commands when narrowing a
failure:

### Clippy (zero warnings)

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Formatting

```bash
cargo fmt --all
```

Verify formatting without modifying files:

```bash
cargo fmt --all -- --check
```

### Benchmarks

Validate the benchmark contracts and compile every harness:

```bash
python3 scripts/bench.py verify
```

Run a short local measurement while iterating:

```bash
python3 scripts/bench.py quick
```

For a performance-sensitive change, save a baseline before editing and compare
against it afterward on the same machine and toolchain:

```bash
python3 scripts/bench.py save before-change
python3 scripts/bench.py compare before-change
```

The compare command gates only `regression/` benchmark IDs, using the existing
2% warning and 5% failure thresholds. A fixture whose canonical DOM contract
changed must not reuse its old result as a performance baseline.

Published results must be generated with `python3 scripts/bench.py publish`;
do not copy numbers from an ad hoc Criterion run into the README. Publication
requires a clean worktree and updates both the root README and benchmark index.
The dirty 2026-07-15 report is provisional historical data, not an official
v0.2 result.

Treat a failed local comparison as a regression candidate. Before attributing
it to a code change, rerun the exact failing benchmark at least twice against
the same immutable baseline. Short selector/XPath compilation measurements,
Tokio benchmarks, and lifecycle/drop variants are especially sensitive to
machine state and workload order. If the result remains material, repeat the
full save/compare experiment under controlled conditions. See the
[local baseline repeatability report](benchmarks/results/2026-07-13-local-baseline-repeatability.md)
for a same-source validation example.

## Coding Standards

These coding standards must be followed in all contributions:

- **Rust edition 2024** — all crates use edition 2024.
- **Doc comments on public items** — every public function, struct, enum, and trait must have `///` documentation.
- **SAFETY comments for unsafe blocks** — every `unsafe {}` block must be preceded by a `// SAFETY:` comment explaining why the operation is sound.
- **`#[inline]` and `#[inline(always)]`** — use these only on hot paths. Do not add them speculatively.
- **Error handling** — use `thiserror` for custom error types.
- **Naming** — `snake_case` for functions and variables, `PascalCase` for types.
- **SIMD code** — comment every intrinsic to explain what it does.
- **Unit tests** — every new module must include unit tests.

## Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/) format:

- `feat:` — a new feature
- `fix:` — a bug fix
- `refactor:` — code restructuring without behavior change
- `docs:` — documentation-only changes
- `test:` — adding or updating tests
- `chore:` — maintenance tasks (local gates, dependencies, tooling)

Examples:

```
feat: add streaming HTML parser with encoding detection
fix: resolve incorrect implicit tag closing for <td> elements
refactor: consolidate SIMD movemask implementations
docs: add usage examples for selector engine
test: add edge-case tests for malformed attribute parsing
```

## Pull Request Process

1. **Fork** the repository and create a feature branch from `main`.
2. **Implement** your changes following the coding standards above.
3. **Commit** using conventional commit messages.
4. **Push** your branch and open a Pull Request against `main`.
5. In the PR description, clearly describe **what** changed and **why**.
6. Run `python3 scripts/release.py check` and include any relevant local
   verification details in the pull request.
7. A maintainer will review your PR and may request changes.

## Maintainer Release Gate

After the release source and benchmark report are committed and the worktree is
clean, run:

```bash
python3 scripts/release.py release --version 0.2.0
```

In addition to the development gate, this checks Rust 1.85, cargo-deny, native
and x86/Rosetta SIMD execution where applicable, Linux and Windows target
checks, every fuzz target for 60 seconds, all seven package archives, copied
license hashes, and clean benchmark metadata. Missing toolchains, targets,
`cargo-deny`, or `cargo-fuzz` are reported as explicit failures and are never
installed by the script.

## Reporting Issues

- Use the **Bug Report** template for defects.
- Use the **Feature Request** template for enhancements.
- Search existing issues before creating a new one to avoid duplicates.

## License

By contributing to this project, you agree that your contributions will be dual-licensed under the **MIT License** and the **Apache License 2.0**, at the user's option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE) for details.
