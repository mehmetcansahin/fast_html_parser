#!/usr/bin/env python3
"""Local quality and release gates for fast-html-parser.

The script never installs toolchains, targets, or Cargo subcommands. Missing
prerequisites fail with an actionable message so a release cannot silently run
with reduced coverage.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tarfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
CRATES = (
    "fhp-core",
    "fhp-simd",
    "fhp-tokenizer",
    "fhp-tree",
    "fhp-selector",
    "fhp-encoding",
    "fast-html-parser",
)
FUZZ_TARGETS = (
    "tokenizer_equivalence",
    "tree_equivalence",
    "entity_decode",
    "selector",
)
CROSS_TARGETS = (
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)


class ReleaseError(RuntimeError):
    """A release prerequisite or validation failed."""


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+ " + " ".join(command), flush=True)
    merged = os.environ.copy()
    if env:
        merged.update(env)
    result = subprocess.run(command, cwd=ROOT, env=merged, check=False)
    if result.returncode != 0:
        raise ReleaseError(f"command failed with exit code {result.returncode}: {' '.join(command)}")


def capture(command: list[str]) -> str:
    result = subprocess.run(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise ReleaseError(f"command failed ({' '.join(command)}): {detail}")
    return result.stdout.strip()


def require_command(command: list[str], install_hint: str) -> None:
    try:
        capture(command)
    except (FileNotFoundError, ReleaseError) as error:
        raise ReleaseError(f"missing release prerequisite; {install_hint}") from error


def ensure_clean_worktree() -> None:
    status = capture(["git", "status", "--porcelain=v1", "--untracked-files=normal"])
    if status:
        raise ReleaseError("release requires a clean Git worktree")


def package_manifests() -> list[Path]:
    return [ROOT / "crates" / crate / "Cargo.toml" for crate in CRATES]


def validate_versions(expected: str) -> None:
    internal = set(CRATES)
    for manifest in package_manifests():
        with manifest.open("rb") as handle:
            document = tomllib.load(handle)
        actual = document["package"]["version"]
        if actual != expected:
            raise ReleaseError(f"{manifest.relative_to(ROOT)} has version {actual}, expected {expected}")
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, declaration in document.get(section, {}).items():
                if name not in internal or not isinstance(declaration, dict):
                    continue
                version = declaration.get("version")
                if version is not None and version != expected:
                    raise ReleaseError(
                        f"{manifest.relative_to(ROOT)} requires {name} {version}, expected {expected}"
                    )


def validate_tools_and_targets() -> None:
    require_command(["cargo", "deny", "--version"], "install cargo-deny")
    require_command(["cargo", "fuzz", "--version"], "install cargo-fuzz")

    toolchains = capture(["rustup", "toolchain", "list"])
    if not any(line.split()[0].startswith("1.85.0") for line in toolchains.splitlines()):
        raise ReleaseError("missing Rust 1.85.0; install it with `rustup toolchain install 1.85.0`")
    if not any(line.split()[0].startswith("nightly") for line in toolchains.splitlines()):
        raise ReleaseError("missing Rust nightly; install it with `rustup toolchain install nightly`")
    require_command(
        ["cargo", "+nightly", "fuzz", "--version"],
        "install cargo-fuzz for the nightly toolchain",
    )

    installed_targets = set(capture(["rustup", "target", "list", "--installed"]).splitlines())
    required = set(CROSS_TARGETS)
    if platform.system() == "Darwin" and platform.machine() == "arm64":
        required.add("x86_64-apple-darwin")
    missing = sorted(required - installed_targets)
    if missing:
        joined = " ".join(missing)
        raise ReleaseError(f"missing Rust targets: {joined}; install with `rustup target add {joined}`")


def audit_package_archives(version: str) -> None:
    package_dir = ROOT / "target" / "package"
    expected_archives = {f"{crate}-{version}.crate" for crate in CRATES}
    actual_archives = {
        archive.name
        for archive in package_dir.glob(f"*-{version}.crate")
        if archive.is_file()
    }
    missing = sorted(expected_archives - actual_archives)
    if missing:
        raise ReleaseError(f"missing package archives: {', '.join(missing)}")

    expected_licenses = {
        license_name: (ROOT / license_name).read_bytes()
        for license_name in ("LICENSE-MIT", "LICENSE-APACHE")
    }
    for crate in CRATES:
        archive = package_dir / f"{crate}-{version}.crate"
        with tarfile.open(archive, "r:gz") as packaged:
            members = {member.name: member for member in packaged.getmembers() if member.isfile()}
            prefix = f"{crate}-{version}/"
            if not members or any(not name.startswith(prefix) for name in members):
                raise ReleaseError(f"{archive.name} contains a path outside {prefix}")
            if f"{prefix}Cargo.toml" not in members:
                raise ReleaseError(f"{archive.name} does not contain Cargo.toml")
            for license_name, expected in expected_licenses.items():
                member = members.get(f"{prefix}{license_name}")
                if member is None:
                    raise ReleaseError(f"{archive.name} does not contain {license_name}")
                handle = packaged.extractfile(member)
                actual = b"" if handle is None else handle.read()
                if hashlib.sha256(actual).digest() != hashlib.sha256(expected).digest():
                    raise ReleaseError(f"{archive.name} contains a stale {license_name}")
        if crate == "fast-html-parser" and f"{crate}-{version}/README.md" not in members:
            raise ReleaseError(f"{archive.name} does not contain README.md")


def validate_clean_benchmark_report() -> None:
    index = (ROOT / "benchmarks" / "README.md").read_text(encoding="utf-8")
    match = re.search(r"\[Full performance report\]\((results/[^)]+\.md)\)", index)
    if match is None:
        raise ReleaseError("benchmarks/README.md has no generated latest report link")
    report = ROOT / "benchmarks" / match.group(1)
    if not report.is_file():
        raise ReleaseError(f"latest benchmark report is missing: {report.relative_to(ROOT)}")
    text = report.read_text(encoding="utf-8")
    sidecar = report.with_suffix(".json")
    if not sidecar.is_file():
        raise ReleaseError(
            f"latest benchmark provenance sidecar is missing: {sidecar.relative_to(ROOT)}"
        )
    try:
        provenance = json.loads(sidecar.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ReleaseError(f"invalid benchmark provenance sidecar: {error}") from error
    metadata = provenance.get("metadata") if isinstance(provenance, dict) else None
    if not isinstance(metadata, dict) or metadata.get("schema_version") != 3:
        raise ReleaseError("latest benchmark provenance must use metadata schema 3")
    git_metadata = metadata.get("git")
    if not isinstance(git_metadata, dict):
        raise ReleaseError("latest benchmark provenance has invalid Git metadata")
    if metadata.get("official") is not True or git_metadata.get("dirty") is not False:
        raise ReleaseError("latest benchmark provenance is not an official clean-tree result")
    expected_report = report.relative_to(ROOT).as_posix()
    if metadata.get("report") != expected_report:
        raise ReleaseError("benchmark provenance report path does not match the latest report")
    expected_report_digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    if provenance.get("report_sha256") != expected_report_digest:
        raise ReleaseError("latest benchmark report digest does not match its provenance sidecar")

    head = capture(["git", "rev-parse", "HEAD"])
    benchmark_commit = git_metadata.get("commit")
    if not isinstance(benchmark_commit, str) or not benchmark_commit:
        raise ReleaseError("benchmark provenance has no Git commit")
    ancestor = subprocess.run(
        ["git", "merge-base", "--is-ancestor", benchmark_commit, head],
        cwd=ROOT,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if ancestor.returncode != 0:
        raise ReleaseError("latest benchmark commit is not an ancestor of the release HEAD")
    if f"| Git commit | `{benchmark_commit}` (clean) |" not in text:
        raise ReleaseError("latest benchmark report commit does not match its provenance sidecar")

    sys.path.insert(0, str(ROOT / "scripts"))
    import bench  # noqa: PLC0415

    digest = bench.source_digest(ROOT)
    if metadata.get("source_digest") != digest or f"| Source digest | `{digest}` |" not in text:
        raise ReleaseError("latest benchmark report source digest does not match the release source")
    semantic_digest = bench.semantic_contract_sha256(ROOT)
    if metadata.get("semantic_contract_sha256") != semantic_digest:
        raise ReleaseError("latest benchmark semantic contract digest is stale")


def command_check() -> None:
    run(["python3", "scripts/generate_entities.py", "--check"])
    run(["python3", "scripts/sync_licenses.py", "--check"])
    run(["cargo", "fmt", "--all", "--", "--check"])
    run(["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"])
    run(["cargo", "test", "--workspace", "--all-features"])
    run(["cargo", "test", "--workspace", "--no-default-features"])
    run(["cargo", "test", "-p", "fast-html-parser", "--no-default-features"])
    run(["cargo", "test", "-p", "fast-html-parser", "--no-default-features", "--features", "simd"])
    run(
        ["cargo", "doc", "--workspace", "--all-features", "--no-deps"],
        env={"RUSTDOCFLAGS": "-D warnings"},
    )
    run(["python3", "-m", "unittest", "discover", "-s", "scripts/tests", "-v"])
    run(["python3", "scripts/bench.py", "verify"])


def command_release(version: str) -> None:
    ensure_clean_worktree()
    validate_versions(version)
    validate_tools_and_targets()
    command_check()
    run(["cargo", "+1.85.0", "check", "--workspace", "--all-targets", "--all-features"])
    run(["cargo", "deny", "check"])
    for target in CROSS_TARGETS:
        run(["cargo", "check", "--workspace", "--all-features", "--target", target])
    if platform.system() == "Darwin" and platform.machine() == "arm64":
        run(["cargo", "test", "-p", "fhp-simd", "--target", "x86_64-apple-darwin"])
    for target in FUZZ_TARGETS:
        run(["cargo", "+nightly", "fuzz", "run", target, "--", "-max_total_time=60"])
    run(["cargo", "package", "--workspace", "--locked", "--no-verify"])
    audit_package_archives(version)
    validate_clean_benchmark_report()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Run local fast-html-parser quality gates")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check", help="run the deterministic development quality gate")
    release = commands.add_parser("release", help="run the complete clean-tree release gate")
    release.add_argument("--version", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "check":
            command_check()
        elif args.command == "release":
            command_release(args.version)
        else:
            raise ReleaseError(f"unknown command: {args.command}")
    except (FileNotFoundError, ReleaseError) as error:
        print(f"release error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
