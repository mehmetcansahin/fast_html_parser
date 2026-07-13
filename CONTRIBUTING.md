# Contributing to fast-html-parser

Thank you for your interest in contributing to fast-html-parser! This document provides guidelines and instructions for contributing.

## Prerequisites

- **Rust toolchain**: Minimum supported Rust version (MSRV) is **1.85** (edition 2024)
- **cargo-deny** (optional): For license and advisory auditing — `cargo install cargo-deny`

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

## Testing

Run the full test suite:

```bash
cargo test --workspace
```

Run tests including async/streaming features:

```bash
cargo test --workspace --features async-tokio
```

## Code Quality

All contributions must pass the following checks before merging:

### Clippy (zero warnings)

```bash
cargo clippy --workspace -- -D warnings
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

The compare command gates only `regression/` benchmark IDs. Published results
must be generated with `python3 scripts/bench.py publish`; do not copy numbers
from an ad hoc Criterion run into the README.

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
- `chore:` — maintenance tasks (CI, dependencies, tooling)

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
6. Ensure all CI checks pass (tests, clippy, formatting).
7. A maintainer will review your PR and may request changes.

## Reporting Issues

- Use the **Bug Report** template for defects.
- Use the **Feature Request** template for enhancements.
- Search existing issues before creating a new one to avoid duplicates.

## License

By contributing to this project, you agree that your contributions will be dual-licensed under the **MIT License** and the **Apache License 2.0**, at the user's option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE) for details.
