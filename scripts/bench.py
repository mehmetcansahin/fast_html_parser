#!/usr/bin/env python3
"""Reproducible, dependency-free benchmark orchestration for this workspace.

Raw Criterion data is deliberately machine-local.  This runner gives every
benchmark binary its own CRITERION_HOME, records the execution contract, and
only lets statistically supported regressions under ``regression/`` affect the
exit status.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime as datetime_module
import hashlib
import json
import math
import os
import platform
import re
import shlex
import shutil
import statistics
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Tuple


SCHEMA_VERSION = 2
RUSTFLAGS = "-C target-cpu=native"
CARGO_INCREMENTAL = "0"
README_START = "<!-- benchmark-summary:start -->"
README_END = "<!-- benchmark-summary:end -->"
ORDER_ROTATIONS = ("fhp-first", "fhp-middle", "fhp-last")
DEFAULT_ORDER = "fhp-middle"
QUICK_RUN_ARGS = ("--quick", "--noplot")
PUBLISH_RUN_ARGS = ("--noplot", "--quiet")
FAIL_THRESHOLD = 0.05
WARN_THRESHOLD = 0.02
EXPECTED_CONFIDENCE_LEVEL = 0.95
CRITERION_DEFAULT_SETTINGS = {
    "warm_up_time_seconds": 3.0,
    "measurement_time_seconds": 5.0,
    "sample_size": 100,
    "confidence_level": EXPECTED_CONFIDENCE_LEVEL,
    "significance_level": 0.05,
    "noise_threshold": 0.01,
}
BASELINE_NAME_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}\Z")
RESERVED_BASELINES = {"base", "change", "new", "report"}
FIXTURE_COLUMNS = (
    "File",
    "Kind",
    "Bytes",
    "SHA-256",
    "Known source",
    "Capture date",
)


class BenchError(RuntimeError):
    """A user-actionable benchmark configuration or execution error."""


@dataclasses.dataclass(frozen=True)
class Harness:
    id: str
    package: str
    bench: str
    features: Tuple[str, ...] = ()
    filter_pattern: Optional[str] = None
    order_sensitive: bool = False

    def as_metadata(self) -> Dict[str, Any]:
        return {
            "id": self.id,
            "package": self.package,
            "bench": self.bench,
            "default_features": False,
            "features": list(self.features),
            "filter": self.filter_pattern,
            "order_sensitive": self.order_sensitive,
        }


FACADE_FEATURES = ("css-selector", "encoding", "entity-decode")

# Each binary has an isolated Criterion output tree.  The async-only e2e slice
# is separate so enabling Tokio does not duplicate all synchronous samples.
FULL_MATRIX: Tuple[Harness, ...] = (
    Harness("fhp-simd/simd", "fhp-simd", "simd_bench"),
    Harness(
        "fhp-tokenizer/tokenizer",
        "fhp-tokenizer",
        "tokenizer_bench",
        ("entity-decode",),
    ),
    Harness(
        "fhp-tree/tree",
        "fhp-tree",
        "tree_bench",
        ("encoding", "entity-decode"),
    ),
    Harness("fhp-selector/selector", "fhp-selector", "selector_bench"),
    Harness("fhp-selector/xpath", "fhp-selector", "xpath_bench"),
    Harness(
        "fast-html-parser/e2e",
        "fast-html-parser",
        "e2e_bench",
        FACADE_FEATURES,
    ),
    Harness(
        "fast-html-parser/e2e-async-tokio",
        "fast-html-parser",
        "e2e_bench",
        FACADE_FEATURES + ("async-tokio",),
        "streaming/async",
    ),
    Harness(
        "fast-html-parser/profile",
        "fast-html-parser",
        "profile_bench",
        FACADE_FEATURES,
    ),
    Harness(
        "fast-html-parser/comparison",
        "fast-html-parser",
        "comparison_bench",
        FACADE_FEATURES,
        order_sensitive=True,
    ),
    Harness(
        "fast-html-parser/realworld",
        "fast-html-parser",
        "realworld_bench",
        FACADE_FEATURES,
        order_sensitive=True,
    ),
)


@dataclasses.dataclass(frozen=True)
class FixtureRecord:
    file: str
    kind: str
    bytes: int
    sha256: str
    known_source: str
    capture_date: str

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class Estimate:
    benchmark: str
    mean_ns: float
    lower_ns: float
    upper_ns: float
    throughput_kind: Optional[str]
    throughput_value: Optional[float]
    harness: str
    rotation: Optional[str] = None

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class ChangeEstimate:
    benchmark: str
    point: float
    lower: float
    upper: float
    harness: str

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class GateDecision:
    benchmark: str
    level: str
    reason: str
    point: float
    lower: float
    upper: float

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class PublishedEstimate:
    benchmark: str
    mean_ns: float
    lower_ns: float
    upper_ns: float
    throughput_kind: Optional[str]
    throughput_value: Optional[float]
    run_count: int
    rotations: Tuple[str, ...]

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class ContractRatio:
    group: str
    competitor: str
    ratio: float
    lower: float
    upper: float
    run_count: int

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


def repository_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _markdown_cells(line: str) -> Optional[List[str]]:
    stripped = line.strip()
    if not stripped.startswith("|") or not stripped.endswith("|"):
        return None
    return [cell.strip() for cell in stripped[1:-1].split("|")]


def _parse_fixture_manifest(root: Path) -> Dict[str, FixtureRecord]:
    manifest = root / "testdata" / "README.md"
    if not manifest.is_file():
        raise BenchError(f"missing fixture manifest: {manifest}")

    lines = manifest.read_text(encoding="utf-8").splitlines()
    headers = [
        index
        for index, line in enumerate(lines)
        if _markdown_cells(line) == list(FIXTURE_COLUMNS)
    ]
    if len(headers) != 1:
        raise BenchError(
            "testdata/README.md must contain exactly one fixture table with columns: "
            + " | ".join(FIXTURE_COLUMNS)
        )
    header_index = headers[0]
    if header_index + 1 >= len(lines):
        raise BenchError("fixture table is missing its alignment row")
    divider = _markdown_cells(lines[header_index + 1])
    if divider is None or len(divider) != len(FIXTURE_COLUMNS):
        raise BenchError("fixture table has an invalid alignment row")
    if any(re.fullmatch(r":?-{3,}:?", cell) is None for cell in divider):
        raise BenchError("fixture table alignment cells must contain Markdown dashes")

    records: Dict[str, FixtureRecord] = {}
    for line_number in range(header_index + 2, len(lines)):
        cells = _markdown_cells(lines[line_number])
        if cells is None:
            break
        if len(cells) != len(FIXTURE_COLUMNS):
            raise BenchError(f"invalid fixture row at testdata/README.md:{line_number + 1}")

        file_match = re.fullmatch(r"`([^`/]+\.html)`", cells[0])
        byte_match = re.fullmatch(r"(?:0|[1-9][0-9]{0,2})(?:,[0-9]{3})*", cells[2])
        hash_match = re.fullmatch(r"`([0-9a-f]{64})`", cells[3])
        if file_match is None:
            raise BenchError(
                f"fixture filename must be a backticked .html basename at line {line_number + 1}"
            )
        if byte_match is None:
            raise BenchError(
                f"fixture byte length must use comma grouping at line {line_number + 1}"
            )
        if hash_match is None:
            raise BenchError(
                f"fixture SHA-256 must be a backticked lowercase digest at line {line_number + 1}"
            )
        if any(not cells[index] for index in (1, 4, 5)):
            raise BenchError(f"fixture metadata cells cannot be empty at line {line_number + 1}")

        filename = file_match.group(1)
        if filename in records:
            raise BenchError(f"duplicate fixture manifest row: {filename}")
        byte_count = int(cells[2].replace(",", ""))
        if cells[2] != format(byte_count, ","):
            raise BenchError(f"non-canonical fixture byte count for {filename}: {cells[2]}")
        records[filename] = FixtureRecord(
            file=filename,
            kind=cells[1],
            bytes=byte_count,
            sha256=hash_match.group(1),
            known_source=cells[4],
            capture_date=cells[5],
        )

    if not records:
        raise BenchError("fixture manifest table has no rows")
    return records


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate_fixture_manifest(root: Path) -> List[FixtureRecord]:
    """Validate manifest coverage and return digests recomputed from current bytes."""

    declared = _parse_fixture_manifest(root)
    fixture_dir = root / "testdata"
    actual_names = {path.name for path in fixture_dir.glob("*.html") if path.is_file()}
    declared_names = set(declared)
    missing_rows = sorted(actual_names - declared_names)
    extra_rows = sorted(declared_names - actual_names)
    if missing_rows or extra_rows:
        details = []
        if missing_rows:
            details.append("missing manifest rows: " + ", ".join(missing_rows))
        if extra_rows:
            details.append("manifest rows without files: " + ", ".join(extra_rows))
        raise BenchError("; ".join(details))

    current: List[FixtureRecord] = []
    for filename in sorted(actual_names):
        path = fixture_dir / filename
        byte_count = path.stat().st_size
        sha256 = _sha256_file(path)
        expected = declared[filename]
        if byte_count != expected.bytes:
            raise BenchError(
                f"fixture byte length mismatch for {filename}: "
                f"manifest={expected.bytes}, current={byte_count}"
            )
        if sha256 != expected.sha256:
            raise BenchError(
                f"fixture SHA-256 mismatch for {filename}: "
                f"manifest={expected.sha256}, current={sha256}"
            )
        # These values are deliberately built from the file, not copied from
        # the manifest, so publish metadata always records current bytes.
        current.append(
            FixtureRecord(
                file=filename,
                kind=expected.kind,
                bytes=byte_count,
                sha256=sha256,
                known_source=expected.known_source,
                capture_date=expected.capture_date,
            )
        )
    return current


def _marker_span(text: str) -> Tuple[int, int]:
    if text.count(README_START) != 1 or text.count(README_END) != 1:
        raise BenchError("README.md must contain exactly one benchmark summary marker pair")
    start = text.index(README_START)
    end = text.index(README_END)
    if start >= end:
        raise BenchError("README.md benchmark summary markers are out of order")
    return start, end


def replace_marked_section(text: str, replacement: str) -> str:
    """Replace the README marker body deterministically and idempotently."""

    if README_START in replacement or README_END in replacement:
        raise BenchError("generated README summary must not contain marker comments")
    start, end = _marker_span(text)
    before = text[: start + len(README_START)]
    after = text[end:]
    body = replacement.strip("\n")
    return before + "\n" + body + "\n" + after


def update_readme_summary(path: Path, replacement: str) -> None:
    """Replace the generated section using the README's latest on-disk text."""

    current = path.read_text(encoding="utf-8")
    _atomic_write_text(path, replace_marked_section(current, replacement))


def _atomic_write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_text(text, encoding="utf-8")
    os.replace(str(temporary), str(path))


def _atomic_write_json(path: Path, value: Any) -> None:
    _atomic_write_text(path, json.dumps(value, indent=2, sort_keys=True) + "\n")


def _capture(command: Sequence[str], root: Path, required: bool = True) -> str:
    result = subprocess.run(
        list(command),
        cwd=str(root),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0 and required:
        detail = result.stderr.strip() or result.stdout.strip()
        raise BenchError(f"command failed ({shlex.join(command)}): {detail}")
    return result.stdout.strip() if result.returncode == 0 else ""


def _parse_key_value_lines(text: str) -> Dict[str, str]:
    values: Dict[str, str] = {}
    for line in text.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            values[key.strip()] = value.strip()
    return values


def _cpu_model(root: Path) -> str:
    system = platform.system()
    if system == "Darwin":
        value = _capture(("sysctl", "-n", "machdep.cpu.brand_string"), root, required=False)
        if value:
            return value
        # Apple Silicon may deny machdep sysctls in a sandbox.  Parse only the
        # non-sensitive Chip field; never persist the command's serial/UUID
        # fields in benchmark metadata.
        hardware = _capture(
            ("system_profiler", "SPHardwareDataType", "-detailLevel", "mini"),
            root,
            required=False,
        )
        for line in hardware.splitlines():
            if line.strip().startswith("Chip:"):
                return line.split(":", 1)[1].strip()
    if system == "Linux":
        cpuinfo = Path("/proc/cpuinfo")
        if cpuinfo.is_file():
            for line in cpuinfo.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.lower().startswith(("model name", "hardware")) and ":" in line:
                    return line.split(":", 1)[1].strip()
    return platform.processor() or platform.machine() or "unknown"


def capture_environment(root: Path) -> Dict[str, Any]:
    rustc_text = _capture(("rustc", "--version", "--verbose"), root)
    rustc_values = _parse_key_value_lines(rustc_text)
    rustc_first = rustc_text.splitlines()[0] if rustc_text else ""
    release = rustc_values.get("release")
    if release is None and rustc_first.startswith("rustc "):
        release = rustc_first.split()[1]
    cfg = _capture(("rustc", "--print", "cfg", "-C", "target-cpu=native"), root)
    native_features = sorted(
        match.group(1)
        for line in cfg.splitlines()
        for match in [re.fullmatch(r'target_feature="([^"]+)"', line.strip())]
        if match is not None
    )
    uname = platform.uname()
    return {
        "cpu": {
            "architecture": platform.machine(),
            "model": _cpu_model(root),
            "native_target_features": native_features,
        },
        "os": {
            "system": uname.system,
            "release": uname.release,
            "version": uname.version,
        },
        "rustc": {
            "release": release or "unknown",
            "commit_hash": rustc_values.get("commit-hash", "unknown"),
            "host": rustc_values.get("host", "unknown"),
            "llvm_version": rustc_values.get("LLVM version", "unknown"),
        },
        "cargo": _capture(("cargo", "--version"), root),
        "target": rustc_values.get("host", "unknown"),
        "python": platform.python_version(),
    }


def _source_files(root: Path) -> List[Path]:
    files: List[Path] = []
    for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml"):
        candidate = root / name
        if candidate.is_file():
            files.append(candidate)
    cargo_dir = root / ".cargo"
    if cargo_dir.is_dir():
        files.extend(path for path in cargo_dir.rglob("*") if path.is_file())
    crates = root / "crates"
    if crates.is_dir():
        files.extend(
            path
            for path in crates.rglob("*")
            if path.is_file() and (path.suffix == ".rs" or path.name == "Cargo.toml")
        )
    testdata = root / "testdata"
    if testdata.is_dir():
        files.extend(path for path in testdata.glob("*.html") if path.is_file())
    runner = root / "scripts" / "bench.py"
    if runner.is_file():
        files.append(runner)
    return sorted(set(files), key=lambda path: path.relative_to(root).as_posix())


def source_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for path in _source_files(root):
        relative = path.relative_to(root).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        digest.update(path.stat().st_size.to_bytes(8, "big"))
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def locked_package_version(lockfile: Path, package_name: str) -> str:
    """Read one exact package version from Cargo.lock without a TOML dependency."""

    if not lockfile.is_file():
        raise BenchError(f"missing Cargo lockfile: {lockfile}")
    matches: List[str] = []
    current_name: Optional[str] = None
    current_version: Optional[str] = None

    def finish_package() -> None:
        if current_name == package_name and current_version is not None:
            matches.append(current_version)

    for raw_line in lockfile.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line == "[[package]]":
            finish_package()
            current_name = None
            current_version = None
            continue
        name_match = re.fullmatch(r'name = "([^"]+)"', line)
        if name_match is not None:
            current_name = name_match.group(1)
            continue
        version_match = re.fullmatch(r'version = "([^"]+)"', line)
        if version_match is not None:
            current_version = version_match.group(1)
    finish_package()

    unique = sorted(set(matches))
    if len(unique) != 1:
        raise BenchError(
            f"expected exactly one locked {package_name!r} version in {lockfile}; got {unique}"
        )
    return unique[0]


def criterion_contract(root: Path, scope: str) -> Dict[str, Any]:
    return {
        "version": locked_package_version(root / "Cargo.lock", "criterion"),
        "quick_mode": scope == "quick",
        "settings": dict(CRITERION_DEFAULT_SETTINGS),
    }


def _fixture_metadata(records: Sequence[FixtureRecord]) -> List[Dict[str, Any]]:
    return [record.as_metadata() for record in records]


def capture_run_inputs(
    root: Path, fixtures: Optional[Sequence[FixtureRecord]] = None
) -> Dict[str, Any]:
    current_fixtures = list(fixtures) if fixtures is not None else validate_fixture_manifest(root)
    lockfile = root / "Cargo.lock"
    fixture_manifest = root / "testdata" / "README.md"
    return {
        "source_digest": source_digest(root),
        "lockfile_sha256": _sha256_file(lockfile) if lockfile.is_file() else None,
        "fixture_manifest_sha256": (
            _sha256_file(fixture_manifest) if fixture_manifest.is_file() else None
        ),
        "fixtures": _fixture_metadata(current_fixtures),
    }


def assert_run_inputs_unchanged(root: Path, initial: Mapping[str, Any]) -> None:
    """Abort before committing metadata when source/lock/fixtures drift mid-run."""

    current = capture_run_inputs(root)
    changed = [
        key
        for key in (
            "source_digest",
            "lockfile_sha256",
            "fixture_manifest_sha256",
            "fixtures",
        )
        if current.get(key) != initial.get(key)
    ]
    if changed:
        raise BenchError(
            "benchmark inputs changed during the run ({}); refusing to write "
            "metadata or published summaries".format(", ".join(changed))
        )


def _git_metadata(root: Path) -> Dict[str, Any]:
    commit = _capture(("git", "rev-parse", "HEAD"), root, required=False) or "unknown"
    branch = _capture(("git", "branch", "--show-current"), root, required=False) or "detached"
    status = _capture(
        ("git", "status", "--porcelain=v1", "--untracked-files=normal"),
        root,
        required=False,
    )
    return {"commit": commit, "branch": branch, "dirty": bool(status)}


def _matrix_metadata(matrix: Sequence[Harness]) -> List[Dict[str, Any]]:
    return [harness.as_metadata() for harness in matrix]


def capture_metadata(
    root: Path,
    mode: str,
    matrix: Sequence[Harness],
    fixtures: Optional[Sequence[FixtureRecord]] = None,
    scope: str = "full",
) -> Dict[str, Any]:
    if scope not in {"full", "quick"}:
        raise BenchError(f"unknown benchmark scope: {scope}")
    inputs = capture_run_inputs(root, fixtures)
    metadata = {
        "schema_version": SCHEMA_VERSION,
        "captured_at_utc": datetime_module.datetime.now(datetime_module.timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "mode": mode,
        "git": _git_metadata(root),
        "environment": capture_environment(root),
        "build_contract": {
            "scope": scope,
            "cargo_locked": True,
            "cargo_incremental": CARGO_INCREMENTAL,
            "rustflags": RUSTFLAGS,
            "criterion": criterion_contract(root, scope),
            "matrix": _matrix_metadata(matrix),
        },
    }
    metadata.update(inputs)
    return metadata


def _deep_get(value: Mapping[str, Any], path: str) -> Any:
    current: Any = value
    for component in path.split("."):
        if not isinstance(current, Mapping) or component not in current:
            return None
        current = current[component]
    return current


COMPATIBILITY_PATHS = (
    "schema_version",
    "environment.cpu.architecture",
    "environment.cpu.model",
    "environment.cpu.native_target_features",
    "environment.os.system",
    "environment.os.release",
    "environment.os.version",
    "environment.rustc.release",
    "environment.rustc.commit_hash",
    "environment.rustc.host",
    "environment.rustc.llvm_version",
    "environment.cargo",
    "environment.target",
    "build_contract.scope",
    "build_contract.cargo_locked",
    "build_contract.cargo_incremental",
    "build_contract.rustflags",
    "build_contract.criterion",
    "build_contract.matrix",
    "fixtures",
)


def compatibility_errors(
    baseline: Mapping[str, Any], current: Mapping[str, Any]
) -> List[str]:
    errors: List[str] = []
    for path in COMPATIBILITY_PATHS:
        old = _deep_get(baseline, path)
        new = _deep_get(current, path)
        if old != new:
            errors.append(f"{path}: baseline={old!r}, current={new!r}")
    return errors


def source_change_messages(
    baseline: Mapping[str, Any], current: Mapping[str, Any]
) -> List[str]:
    messages: List[str] = []
    baseline_commit = _deep_get(baseline, "git.commit")
    current_commit = _deep_get(current, "git.commit")
    if baseline_commit != current_commit:
        messages.append(
            "git commit changed: "
            f"{str(baseline_commit)[:12]} -> {str(current_commit)[:12]}"
        )
    if baseline.get("source_digest") != current.get("source_digest"):
        messages.append(
            "source changed: "
            f"{str(baseline.get('source_digest'))[:12]} -> "
            f"{str(current.get('source_digest'))[:12]}"
        )
    if baseline.get("lockfile_sha256") != current.get("lockfile_sha256"):
        messages.append("Cargo.lock changed since the saved baseline")
    return messages


def validate_baseline_name(name: str) -> None:
    if BASELINE_NAME_RE.fullmatch(name) is None or name in RESERVED_BASELINES:
        raise BenchError(
            "baseline name must be 1-64 safe characters, start alphanumeric, "
            "and not be one of: " + ", ".join(sorted(RESERVED_BASELINES))
        )


def _safe_component(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "-", value).strip("-.")
    return cleaned or "unknown"


def _criterion_home(root: Path, namespace: str, harness: Harness) -> Path:
    harness_component = _safe_component(harness.id.replace("/", "__"))
    return root / "target" / "criterion" / namespace / harness_component


def build_cargo_command(harness: Harness, criterion_args: Sequence[str]) -> List[str]:
    command = [
        "cargo",
        "bench",
        "--locked",
        "-p",
        harness.package,
        "--bench",
        harness.bench,
        "--no-default-features",
    ]
    if harness.features:
        command.extend(("--features", ",".join(harness.features)))
    command.append("--")
    command.extend(criterion_args)
    if harness.filter_pattern:
        command.append(harness.filter_pattern)
    return command


def _benchmark_environment(home: Path, order: Optional[str]) -> Dict[str, str]:
    environment = os.environ.copy()
    environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
    environment["CARGO_INCREMENTAL"] = CARGO_INCREMENTAL
    environment["RUSTFLAGS"] = RUSTFLAGS
    environment["CRITERION_HOME"] = str(home.resolve())
    if order is None:
        environment.pop("FHP_BENCH_ORDER", None)
    else:
        environment["FHP_BENCH_ORDER"] = order
    return environment


def run_harness(
    root: Path,
    harness: Harness,
    home: Path,
    criterion_args: Sequence[str],
    order: Optional[str] = None,
) -> Dict[str, Any]:
    if order is not None and order not in ORDER_ROTATIONS:
        raise BenchError(f"invalid FHP_BENCH_ORDER value: {order}")
    home.mkdir(parents=True, exist_ok=True)
    command = build_cargo_command(harness, criterion_args)
    print(f"\n[{harness.id}] CRITERION_HOME={home}")
    if order:
        print(f"FHP_BENCH_ORDER={order}")
    print("$ " + shlex.join(command), flush=True)
    completed = subprocess.run(
        command,
        cwd=str(root),
        env=_benchmark_environment(home, order),
        check=False,
    )
    if completed.returncode != 0:
        raise BenchError(
            f"benchmark harness {harness.id} failed with exit code {completed.returncode}"
        )
    return {
        "harness": harness.id,
        "command": command,
        "criterion_home": str(home.relative_to(root)),
        "environment": {
            "CARGO_INCREMENTAL": CARGO_INCREMENTAL,
            "RUSTFLAGS": RUSTFLAGS,
            "FHP_BENCH_ORDER": order,
        },
    }


def _load_json_object(path: Path) -> Dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BenchError(f"could not read Criterion JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise BenchError(f"Criterion JSON is not an object: {path}")
    return value


def _finite_number(value: Any, path: Path, label: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise BenchError(f"{label} is not numeric in {path}")
    number = float(value)
    if not math.isfinite(number):
        raise BenchError(f"{label} is not finite in {path}")
    return number


def _mean_values(document: Mapping[str, Any], path: Path) -> Tuple[float, float, float]:
    mean = document.get("mean")
    if not isinstance(mean, Mapping):
        raise BenchError(f"missing mean estimate in {path}")
    interval = mean.get("confidence_interval")
    if not isinstance(interval, Mapping):
        raise BenchError(f"missing mean confidence interval in {path}")
    confidence_level = _finite_number(
        interval.get("confidence_level"), path, "mean CI confidence_level"
    )
    if not math.isclose(
        confidence_level,
        EXPECTED_CONFIDENCE_LEVEL,
        rel_tol=0.0,
        abs_tol=1e-12,
    ):
        raise BenchError(
            f"unexpected Criterion confidence level in {path}: "
            f"expected {EXPECTED_CONFIDENCE_LEVEL}, got {confidence_level}"
        )
    return (
        _finite_number(mean.get("point_estimate"), path, "mean.point_estimate"),
        _finite_number(interval.get("lower_bound"), path, "mean CI lower_bound"),
        _finite_number(interval.get("upper_bound"), path, "mean CI upper_bound"),
    )


def _benchmark_identity(benchmark_path: Path) -> Tuple[str, Optional[str], Optional[float]]:
    benchmark = _load_json_object(benchmark_path)
    full_id = benchmark.get("full_id")
    if not isinstance(full_id, str) or not full_id:
        raise BenchError(f"missing full_id in {benchmark_path}")
    throughput = benchmark.get("throughput")
    if throughput is None:
        return full_id, None, None
    if not isinstance(throughput, Mapping) or len(throughput) != 1:
        raise BenchError(f"invalid throughput object in {benchmark_path}")
    kind, raw_value = next(iter(throughput.items()))
    if not isinstance(kind, str):
        raise BenchError(f"invalid throughput kind in {benchmark_path}")
    return full_id, kind, _finite_number(raw_value, benchmark_path, "throughput")


def parse_new_estimates(
    home: Path, harness: Harness, rotation: Optional[str] = None
) -> List[Estimate]:
    estimates: List[Estimate] = []
    for path in sorted(home.rglob("new/estimates.json")):
        benchmark_path = path.parent / "benchmark.json"
        if not benchmark_path.is_file():
            raise BenchError(f"missing Criterion benchmark metadata beside {path}")
        document = _load_json_object(path)
        mean, lower, upper = _mean_values(document, path)
        full_id, throughput_kind, throughput_value = _benchmark_identity(benchmark_path)
        estimates.append(
            Estimate(
                benchmark=full_id,
                mean_ns=mean,
                lower_ns=lower,
                upper_ns=upper,
                throughput_kind=throughput_kind,
                throughput_value=throughput_value,
                harness=harness.id,
                rotation=rotation,
            )
        )
    return estimates


def parse_change_estimates(home: Path, harness: Harness) -> List[ChangeEstimate]:
    estimates: List[ChangeEstimate] = []
    for path in sorted(home.rglob("change/estimates.json")):
        benchmark_path = path.parent.parent / "new" / "benchmark.json"
        if not benchmark_path.is_file():
            raise BenchError(f"missing new/benchmark.json for Criterion change {path}")
        document = _load_json_object(path)
        point, lower, upper = _mean_values(document, path)
        full_id, _, _ = _benchmark_identity(benchmark_path)
        estimates.append(
            ChangeEstimate(
                benchmark=full_id,
                point=point,
                lower=lower,
                upper=upper,
                harness=harness.id,
            )
        )
    return estimates


def classify_change(change: ChangeEstimate) -> GateDecision:
    if not change.benchmark.startswith("regression/"):
        return GateDecision(
            change.benchmark,
            "info",
            "non-regression namespace; reported but never gated",
            change.point,
            change.lower,
            change.upper,
        )

    significant_slowdown = change.lower > 0.0
    if change.point >= FAIL_THRESHOLD and significant_slowdown:
        level = "fail"
        reason = "statistically significant slowdown of at least 5%"
    elif WARN_THRESHOLD <= change.point < FAIL_THRESHOLD and significant_slowdown:
        level = "warn"
        reason = "statistically significant slowdown between 2% and 5%"
    elif change.point >= FAIL_THRESHOLD:
        level = "warn"
        reason = "noisy slowdown of at least 5%; confidence interval includes zero"
    else:
        level = "pass"
        reason = "below the regression warning threshold"
    return GateDecision(
        change.benchmark,
        level,
        reason,
        change.point,
        change.lower,
        change.upper,
    )


def classify_changes(changes: Iterable[ChangeEstimate]) -> List[GateDecision]:
    return sorted((classify_change(change) for change in changes), key=lambda item: item.benchmark)


def _percent(value: float) -> str:
    return f"{value * 100:+.2f}%"


def _print_gate_summary(decisions: Sequence[GateDecision]) -> None:
    for decision in decisions:
        if decision.level in {"fail", "warn"}:
            print(
                f"{decision.level.upper():4} {decision.benchmark}: "
                f"{_percent(decision.point)} "
                f"(95% CI {_percent(decision.lower)}..{_percent(decision.upper)}) — "
                f"{decision.reason}"
            )
    counts = {
        level: sum(decision.level == level for decision in decisions)
        for level in ("fail", "warn", "pass", "info")
    }
    print(
        "comparison summary: "
        + ", ".join(f"{level}={counts[level]}" for level in ("fail", "warn", "pass", "info"))
    )


def _quick_matrix() -> Tuple[Harness, ...]:
    quick: List[Harness] = []
    for harness in FULL_MATRIX:
        if harness.order_sensitive:
            continue
        pattern = harness.filter_pattern or "regression/"
        quick.append(dataclasses.replace(harness, filter_pattern=pattern))
    return tuple(quick)


def _run_namespace() -> str:
    timestamp = datetime_module.datetime.now(datetime_module.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"{timestamp}-{os.getpid()}"


def command_verify(root: Path) -> int:
    fixtures = validate_fixture_manifest(root)
    readme = root / "README.md"
    if not readme.is_file():
        raise BenchError("missing README.md")
    _marker_span(readme.read_text(encoding="utf-8"))
    metadata = capture_metadata(root, "verify", FULL_MATRIX, fixtures)
    namespace = "verify/" + _run_namespace()
    for harness in FULL_MATRIX:
        home = _criterion_home(root, namespace, harness)
        order = DEFAULT_ORDER if harness.order_sensitive else None
        run_harness(root, harness, home, ("--test", "--noplot"), order)
    print(
        f"verified {len(FULL_MATRIX)} targeted harnesses, {len(fixtures)} fixtures, "
        f"target {metadata['environment']['target']}"
    )
    return 0


def command_quick(root: Path) -> int:
    validate_fixture_manifest(root)
    matrix = _quick_matrix()
    namespace = "quick/" + _run_namespace()
    result_count = 0
    for harness in matrix:
        home = _criterion_home(root, namespace, harness)
        run_harness(
            root,
            harness,
            home,
            QUICK_RUN_ARGS,
        )
        parsed = parse_new_estimates(home, harness)
        if not parsed:
            raise BenchError(f"quick harness produced no Criterion estimates: {harness.id}")
        result_count += len(parsed)
    print(f"quick suite completed: {result_count} regression estimates")
    return 0


def _baseline_metadata_path(root: Path, name: str) -> Path:
    return root / "target" / "criterion" / "baselines" / name / "metadata.json"


def _normal_harness_home(root: Path, harness: Harness) -> Path:
    return _criterion_home(root, "harnesses", harness)


def clear_transient_comparison_data(root: Path, home: Path) -> None:
    """Remove only stale Criterion ``new``/``change`` trees from a guarded home.

    Named baseline directories, reports, and every path outside the runner's
    harness root are preserved.  This keeps a removed/renamed benchmark from a
    prior compare run from leaking into the current gate decision.
    """

    guard = (root / "target" / "criterion" / "harnesses").resolve()
    resolved_home = home.resolve()
    try:
        resolved_home.relative_to(guard)
    except ValueError as error:
        raise BenchError(f"refusing to clean unguarded Criterion home: {home}") from error
    if resolved_home == guard:
        raise BenchError("refusing to clean the Criterion harness root itself")
    if not resolved_home.exists():
        return

    # Detect all leaves before deleting either sibling.  A benchmark ID may
    # itself contain a component named `new` or `change`; only Criterion result
    # leaves with their direct files qualify for removal.
    transient: List[Path] = []
    for path in resolved_home.rglob("*"):
        if not path.is_dir() or not (path / "estimates.json").is_file():
            continue
        if path.name == "new" and (path / "benchmark.json").is_file():
            transient.append(path)
        elif path.name == "change" and (path.parent / "new" / "benchmark.json").is_file():
            transient.append(path)
    transient.sort(key=lambda path: len(path.parts), reverse=True)
    for path in transient:
        shutil.rmtree(path)


def _named_baseline_dirs(home: Path, name: str) -> List[Path]:
    if not home.exists():
        return []
    return sorted(
        path
        for path in home.rglob(name)
        if path.is_dir()
        and any(
            (path / filename).is_file()
            for filename in (
                "estimates.json",
                "benchmark.json",
                "sample.json",
                "tukey.json",
                "raw.csv",
            )
        )
    )


def parse_saved_baseline_ids(home: Path, name: str) -> List[str]:
    ids: List[str] = []
    for directory in _named_baseline_dirs(home, name):
        full_id, _, _ = _benchmark_identity(directory / "benchmark.json")
        ids.append(full_id)
    if len(ids) != len(set(ids)):
        raise BenchError(f"duplicate Criterion IDs in baseline {name!r} under {home}")
    return sorted(ids)


def ensure_baseline_name_available(root: Path, name: str) -> None:
    metadata_path = _baseline_metadata_path(root, name)
    existing_dirs: List[Path] = []
    for harness in FULL_MATRIX:
        existing_dirs.extend(_named_baseline_dirs(_normal_harness_home(root, harness), name))
    if metadata_path.exists() or existing_dirs:
        detail = (
            str(metadata_path.relative_to(root))
            if metadata_path.exists()
            else str(existing_dirs[0].relative_to(root))
        )
        raise BenchError(
            f"baseline {name!r} already exists or has partial artifacts at {detail}; "
            "baselines are immutable. Choose a new name, or inspect and remove the "
            "partial target/criterion artifacts before retrying."
        )


def _baseline_index_from_metadata(
    metadata: Mapping[str, Any], matrix: Sequence[Harness]
) -> Dict[str, List[str]]:
    raw_index = metadata.get("baseline_index")
    if not isinstance(raw_index, Mapping):
        raise BenchError("baseline metadata has no per-harness baseline_index")
    expected_harnesses = {harness.id for harness in matrix}
    if set(raw_index) != expected_harnesses:
        raise BenchError(
            "baseline metadata harness index does not match the requested matrix"
        )
    index: Dict[str, List[str]] = {}
    for harness_id in sorted(expected_harnesses):
        values = raw_index.get(harness_id)
        if not isinstance(values, list) or not values or not all(
            isinstance(value, str) and value for value in values
        ):
            raise BenchError(f"invalid baseline ID index for harness {harness_id}")
        if len(values) != len(set(values)):
            raise BenchError(f"duplicate baseline IDs for harness {harness_id}")
        index[harness_id] = sorted(values)
    return index


def _save_criterion_args(name: str, quick: bool) -> List[str]:
    arguments = ["--save-baseline", name, "--noplot"]
    if quick:
        arguments.insert(0, "--quick")
    return arguments


def _compare_criterion_args(name: str, quick: bool) -> List[str]:
    arguments = ["--baseline-lenient", name, "--noplot"]
    if quick:
        arguments.insert(0, "--quick")
    return arguments


def assess_compare_coverage(
    harness_id: str,
    current_ids: Iterable[str],
    saved_ids: Iterable[str],
    change_ids: Iterable[str],
) -> Dict[str, List[str]]:
    current = set(current_ids)
    saved = set(saved_ids)
    changed = set(change_ids)
    current_regression = {
        benchmark for benchmark in current if benchmark.startswith("regression/")
    }
    missing_regression_baselines = sorted(current_regression - saved)
    missing_regression_changes = sorted(current_regression - changed)
    if missing_regression_baselines or missing_regression_changes:
        details = []
        if missing_regression_baselines:
            details.append(
                "missing saved regression baselines: "
                + ", ".join(missing_regression_baselines)
            )
        if missing_regression_changes:
            details.append(
                "missing regression change estimates: "
                + ", ".join(missing_regression_changes)
            )
        raise BenchError(f"regression coverage failure for {harness_id}: " + "; ".join(details))
    unexpected_changes = sorted(changed - current)
    if unexpected_changes:
        raise BenchError(
            f"stale Criterion changes remained for {harness_id}: "
            + ", ".join(unexpected_changes)
        )
    return {
        "current_ids": sorted(current),
        "saved_ids": sorted(saved),
        "change_ids": sorted(changed),
        "uncompared_non_regression_ids": sorted(
            benchmark
            for benchmark in current - saved
            if not benchmark.startswith("regression/")
        ),
        "removed_baseline_ids": sorted(saved - current),
    }


def command_save(root: Path, name: str, quick: bool = False) -> int:
    validate_baseline_name(name)
    ensure_baseline_name_available(root, name)
    fixtures = validate_fixture_manifest(root)
    matrix = _quick_matrix() if quick else FULL_MATRIX
    scope = "quick" if quick else "full"
    metadata = capture_metadata(root, "save", matrix, fixtures, scope)
    commands: List[Dict[str, Any]] = []
    baseline_index: Dict[str, List[str]] = {}
    criterion_args = _save_criterion_args(name, quick)
    try:
        for harness in matrix:
            home = _normal_harness_home(root, harness)
            order = DEFAULT_ORDER if harness.order_sensitive else None
            commands.append(
                run_harness(
                    root,
                    harness,
                    home,
                    criterion_args,
                    order,
                )
            )
            saved_ids = parse_saved_baseline_ids(home, name)
            if not saved_ids:
                raise BenchError(
                    f"Criterion did not write baseline {name!r} for {harness.id}"
                )
            baseline_index[harness.id] = saved_ids
        assert_run_inputs_unchanged(root, metadata)
        metadata["baseline"] = name
        metadata["baseline_index"] = baseline_index
        metadata["commands"] = commands
        path = _baseline_metadata_path(root, name)
        _atomic_write_json(path, metadata)
    except (BenchError, OSError) as error:
        raise BenchError(
            f"baseline save {name!r} did not commit metadata: {error}. "
            "Choose a new baseline name, or inspect and remove any partial "
            "target/criterion named-baseline directories before retrying."
        ) from error
    print(f"saved baseline {name!r} and compatibility metadata at {path.relative_to(root)}")
    return 0


def command_compare(root: Path, name: str, quick: bool = False) -> int:
    validate_baseline_name(name)
    metadata_path = _baseline_metadata_path(root, name)
    if not metadata_path.is_file():
        raise BenchError(f"missing baseline metadata: {metadata_path.relative_to(root)}")
    baseline = _load_json_object(metadata_path)
    fixtures = validate_fixture_manifest(root)
    matrix = _quick_matrix() if quick else FULL_MATRIX
    scope = "quick" if quick else "full"
    current = capture_metadata(root, "compare", matrix, fixtures, scope)
    errors = compatibility_errors(baseline, current)
    if errors:
        raise BenchError("incompatible benchmark environment:\n  - " + "\n  - ".join(errors))

    for message in source_change_messages(baseline, current):
        print(message)

    baseline_index = _baseline_index_from_metadata(baseline, matrix)
    for harness in matrix:
        actual_ids = parse_saved_baseline_ids(_normal_harness_home(root, harness), name)
        if actual_ids != baseline_index[harness.id]:
            raise BenchError(
                f"saved Criterion baseline IDs for {harness.id} do not match metadata; "
                "the baseline is incomplete or has been modified"
            )

    commands: List[Dict[str, Any]] = []
    changes: List[ChangeEstimate] = []
    coverage: Dict[str, Dict[str, List[str]]] = {}
    criterion_args = _compare_criterion_args(name, quick)
    for harness in matrix:
        home = _normal_harness_home(root, harness)
        clear_transient_comparison_data(root, home)
        order = DEFAULT_ORDER if harness.order_sensitive else None
        commands.append(
            run_harness(
                root,
                harness,
                home,
                criterion_args,
                order,
            )
        )
        current_estimates = parse_new_estimates(home, harness)
        if not current_estimates:
            raise BenchError(f"comparison produced no current estimates for {harness.id}")
        parsed_changes = parse_change_estimates(home, harness)
        current_ids = {estimate.benchmark for estimate in current_estimates}
        saved_ids = set(baseline_index[harness.id])
        change_ids = {change.benchmark for change in parsed_changes}
        harness_coverage = assess_compare_coverage(
            harness.id, current_ids, saved_ids, change_ids
        )
        uncompared_non_regression = harness_coverage["uncompared_non_regression_ids"]
        if uncompared_non_regression:
            print(
                f"INFO {harness.id}: {len(uncompared_non_regression)} new non-regression "
                "benchmark(s) reported without a baseline"
            )
        coverage[harness.id] = harness_coverage
        changes.extend(parsed_changes)

    assert_run_inputs_unchanged(root, current)
    decisions = classify_changes(changes)
    _print_gate_summary(decisions)
    current["baseline"] = name
    current["commands"] = commands
    current["coverage"] = coverage
    current["changes"] = [change.as_metadata() for change in changes]
    current["decisions"] = [decision.as_metadata() for decision in decisions]
    comparison_dir = root / "target" / "criterion" / "comparisons"
    output_name = f"{_run_namespace()}-vs-{_safe_component(name)}.json"
    _atomic_write_json(comparison_dir / output_name, current)
    return 1 if any(decision.level == "fail" for decision in decisions) else 0


def aggregate_estimates(estimates: Sequence[Estimate]) -> List[PublishedEstimate]:
    grouped: Dict[str, List[Estimate]] = {}
    for estimate in estimates:
        grouped.setdefault(estimate.benchmark, []).append(estimate)

    published: List[PublishedEstimate] = []
    for benchmark in sorted(grouped):
        runs = grouped[benchmark]
        kinds = {run.throughput_kind for run in runs}
        values = {run.throughput_value for run in runs}
        if len(kinds) != 1 or len(values) != 1:
            raise BenchError(f"inconsistent throughput metadata across runs for {benchmark}")
        means = [run.mean_ns for run in runs]
        if len(runs) == 1:
            lower = runs[0].lower_ns
            upper = runs[0].upper_ns
        else:
            # Criterion confidence intervals are per run.  Across order
            # rotations, publish the range of run means rather than inventing
            # a combined confidence interval.
            lower = min(means)
            upper = max(means)
        published.append(
            PublishedEstimate(
                benchmark=benchmark,
                mean_ns=statistics.median(means),
                lower_ns=lower,
                upper_ns=upper,
                throughput_kind=next(iter(kinds)),
                throughput_value=next(iter(values)),
                run_count=len(runs),
                rotations=tuple(
                    sorted(
                        (run.rotation for run in runs if run.rotation is not None),
                        key=lambda value: ORDER_ROTATIONS.index(value),
                    )
                ),
            )
        )
    return published


def validate_publish_run_cardinality(
    estimates: Sequence[Estimate], matrix: Sequence[Harness]
) -> None:
    """Require one complete run, or all three order rotations, for every ID."""

    harnesses = {harness.id: harness for harness in matrix}
    grouped: Dict[Tuple[str, str], List[Estimate]] = {}
    for estimate in estimates:
        if estimate.harness not in harnesses:
            raise BenchError(f"estimate references unknown harness: {estimate.harness}")
        grouped.setdefault((estimate.harness, estimate.benchmark), []).append(estimate)

    for harness in matrix:
        harness_rows = [key for key in grouped if key[0] == harness.id]
        if not harness_rows:
            raise BenchError(f"publish produced no benchmark IDs for harness {harness.id}")

    for (harness_id, benchmark), runs in sorted(grouped.items()):
        harness = harnesses[harness_id]
        rotations = [run.rotation for run in runs]
        if harness.order_sensitive:
            if len(runs) != len(ORDER_ROTATIONS) or sorted(
                rotations, key=lambda value: ORDER_ROTATIONS.index(value) if value in ORDER_ROTATIONS else -1
            ) != list(ORDER_ROTATIONS):
                raise BenchError(
                    f"order-sensitive benchmark {benchmark} must have exactly one "
                    f"run for each rotation {ORDER_ROTATIONS}; got {rotations}"
                )
        elif len(runs) != 1 or rotations != [None]:
            raise BenchError(
                f"non-order-sensitive benchmark {benchmark} must have exactly one "
                f"unrotated run; got {rotations}"
            )


def contract_equal_ratios(estimates: Sequence[Estimate]) -> List[ContractRatio]:
    """Median paired ratios for IDs carrying ``contract_equal`` explicitly.

    Every ratio is formed within one independent publication run, then the
    three run-local ratios are summarized.  This avoids dividing separately
    aggregated parser estimates and preserves the publication run cardinality.
    """

    per_run: Dict[Tuple[str, Optional[str]], Dict[str, Estimate]] = {}
    for estimate in estimates:
        if "/contract_equal/" not in estimate.benchmark:
            continue
        if "/" not in estimate.benchmark:
            continue
        group, implementation = estimate.benchmark.rsplit("/", 1)
        key = (group, estimate.rotation)
        implementations = per_run.setdefault(key, {})
        if implementation in implementations:
            raise BenchError(
                f"duplicate {implementation} estimate for {group} rotation {estimate.rotation}"
            )
        implementations[implementation] = estimate

    samples: Dict[Tuple[str, str], List[float]] = {}
    for (group, rotation), implementations in sorted(
        per_run.items(), key=lambda item: (item[0][0], item[0][1] or "")
    ):
        fhp = implementations.get("fast_html_parser") or implementations.get("fhp")
        if fhp is None or fhp.mean_ns <= 0:
            continue
        for competitor in sorted(implementations):
            if competitor in {"fast_html_parser", "fhp"}:
                continue
            candidate = implementations[competitor]
            samples.setdefault((group, competitor), []).append(
                candidate.mean_ns / fhp.mean_ns
            )

    ratios: List[ContractRatio] = []
    for (group, competitor), numeric_values in sorted(samples.items()):
        ratios.append(
            ContractRatio(
                group=group,
                competitor=competitor,
                ratio=statistics.median(numeric_values),
                lower=min(numeric_values),
                upper=max(numeric_values),
                run_count=len(numeric_values),
            )
        )
    return ratios


def _format_duration(nanoseconds: float) -> str:
    absolute = abs(nanoseconds)
    if absolute < 1_000:
        return f"{nanoseconds:.2f} ns"
    if absolute < 1_000_000:
        return f"{nanoseconds / 1_000:.2f} µs"
    if absolute < 1_000_000_000:
        return f"{nanoseconds / 1_000_000:.2f} ms"
    return f"{nanoseconds / 1_000_000_000:.2f} s"


def _format_throughput(estimate: PublishedEstimate) -> str:
    if estimate.throughput_kind is None or estimate.throughput_value is None:
        return "—"
    if estimate.mean_ns <= 0:
        return "—"
    units_per_second = estimate.throughput_value * 1_000_000_000 / estimate.mean_ns
    if estimate.throughput_kind == "Bytes":
        for divisor, suffix in (
            (1024.0**3, "GiB/s"),
            (1024.0**2, "MiB/s"),
            (1024.0, "KiB/s"),
        ):
            if units_per_second >= divisor:
                return f"{units_per_second / divisor:.2f} {suffix}"
        return f"{units_per_second:.2f} B/s"
    return f"{units_per_second:.2f} {estimate.throughput_kind}/s"


def _escape_markdown(value: Any) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ")


def _benchmark_category(benchmark: str) -> str:
    return benchmark.split("/", 1)[0] if "/" in benchmark else "uncategorized"


def _correctness_status(benchmark: str) -> str:
    if "/contract_equal/" in benchmark:
        return "contract-equal"
    if "/semantic_reference/" in benchmark:
        return "semantic-reference (absolute)"
    if benchmark.startswith("regression/"):
        return "project-owned regression"
    if benchmark.startswith("diagnostic/"):
        return "diagnostic (no equality contract)"
    return "absolute only"


def _result_table(estimates: Sequence[PublishedEstimate]) -> str:
    lines = [
        "| Benchmark | Category | Correctness status | Estimate | 95% CI / run range | Throughput | Runs |",
        "|---|---|---|---:|---:|---:|---:|",
    ]
    for estimate in estimates:
        interval_label = "95% CI" if estimate.run_count == 1 else "run range"
        interval = (
            f"{interval_label}: {_format_duration(estimate.lower_ns)}–"
            f"{_format_duration(estimate.upper_ns)}"
        )
        lines.append(
            "| `{}` | `{}` | {} | {} | {} | {} | {} |".format(
                _escape_markdown(estimate.benchmark),
                _escape_markdown(_benchmark_category(estimate.benchmark)),
                _escape_markdown(_correctness_status(estimate.benchmark)),
                _format_duration(estimate.mean_ns),
                interval,
                _format_throughput(estimate),
                estimate.run_count,
            )
        )
    return "\n".join(lines)


def _ratio_table(ratios: Sequence[ContractRatio]) -> str:
    if not ratios:
        return "No explicit `contract_equal` ratio was available in this run."
    lines = [
        "| Contract-equal group | Competitor | Median competitor/FHP | Run range | Runs |",
        "|---|---|---:|---:|---:|",
    ]
    for ratio in ratios:
        lines.append(
            f"| `{_escape_markdown(ratio.group)}` | `{_escape_markdown(ratio.competitor)}` "
            f"| {ratio.ratio:.3f}× | {ratio.lower:.3f}×–{ratio.upper:.3f}× "
            f"| {ratio.run_count} |"
        )
    return "\n".join(lines)


def generate_report_markdown(
    metadata: Mapping[str, Any],
    estimates: Sequence[PublishedEstimate],
    ratios: Sequence[ContractRatio],
) -> str:
    environment = metadata["environment"]
    cpu = environment["cpu"]
    rustc = environment["rustc"]
    git = metadata["git"]
    criterion = metadata["build_contract"]["criterion"]
    fixture_lines = [
        "| Fixture | Kind | Bytes | SHA-256 | Known source | Capture date |",
        "|---|---|---:|---|---|---|",
    ]
    for fixture in metadata["fixtures"]:
        fixture_lines.append(
            f"| `{_escape_markdown(fixture['file'])}` | "
            f"{_escape_markdown(fixture['kind'])} | {fixture['bytes']:,} "
            f"| `{fixture['sha256']}` | "
            f"{_escape_markdown(fixture['known_source'])} | "
            f"{_escape_markdown(fixture['capture_date'])} |"
        )
    matrix_lines = [
        "| Harness | Target | Features | Filter |",
        "|---|---|---|---|",
    ]
    for harness in metadata["build_contract"]["matrix"]:
        features = ", ".join(harness["features"]) or "(none)"
        filter_pattern = harness["filter"] or "(all)"
        matrix_lines.append(
            f"| `{_escape_markdown(harness['id'])}` | "
            f"`{_escape_markdown(harness['package'])}/{_escape_markdown(harness['bench'])}` | "
            f"`{_escape_markdown(features)}` | `{_escape_markdown(filter_pattern)}` |"
        )
    commands = metadata.get("commands", [])
    command_lines = [
        "```text",
        *(
            "$ " + shlex.join(command["command"])
            + (
                f"  # FHP_BENCH_ORDER={command['environment']['FHP_BENCH_ORDER']}"
                if command["environment"].get("FHP_BENCH_ORDER")
                else ""
            )
            for command in commands
        ),
        "```",
    ]
    range_note = (
        "Single-run rows show Criterion's 95% confidence interval. The two "
        "order-sensitive comparison harnesses show the median of three run means "
        "and their min–max range. Lower time is better."
    )
    return "\n".join(
        [
            "# Benchmark report",
            "",
            f"Generated at `{metadata['captured_at_utc']}`.",
            "",
            "## Reproducibility metadata",
            "",
            "| Field | Value |",
            "|---|---|",
            f"| Source digest | `{metadata['source_digest']}` |",
            f"| Fixture manifest digest | `{metadata['fixture_manifest_sha256']}` |",
            f"| Git commit | `{git['commit']}` ({'dirty' if git['dirty'] else 'clean'}) |",
            f"| Target | `{environment['target']}` |",
            f"| CPU | {_escape_markdown(cpu['model'])} (`{cpu['architecture']}`) |",
            f"| OS | {_escape_markdown(environment['os']['system'])} "
            f"{_escape_markdown(environment['os']['release'])} |",
            f"| rustc | `{rustc['release']}` (`{rustc['commit_hash']}`) |",
            f"| Cargo | `{_escape_markdown(environment['cargo'])}` |",
            f"| RUSTFLAGS | `{RUSTFLAGS}` |",
            f"| CARGO_INCREMENTAL | `{CARGO_INCREMENTAL}` |",
            f"| Benchmark scope | `{metadata['build_contract']['scope']}` |",
            f"| Criterion | `{criterion['version']}`; quick={str(criterion['quick_mode']).lower()} |",
            f"| Criterion settings | "
            f"`{_escape_markdown(json.dumps(criterion['settings'], sort_keys=True))}` |",
            "",
            "## Harness and feature matrix",
            "",
            *matrix_lines,
            "",
            "## Fixture integrity",
            "",
            *fixture_lines,
            "",
            "## Absolute estimates",
            "",
            range_note,
            "",
            _result_table(estimates),
            "",
            "## Contract-equal ratios",
            "",
            "Ratios are emitted only when the benchmark ID contains the explicit "
            "`contract_equal` contract marker. Values above 1× mean FHP completed "
            "the same checked workload faster. Each value is formed inside one "
            "independent run before the three run-local ratios are summarized.",
            "",
            _ratio_table(ratios),
            "",
            "## Commands",
            "",
            *command_lines,
            "",
            "Raw Criterion samples and reports remain machine-local under `target/criterion/`.",
            "",
        ]
    )


def generate_readme_summary(
    metadata: Mapping[str, Any],
    estimates: Sequence[PublishedEstimate],
    ratios: Sequence[ContractRatio],
    report_path: Path,
) -> str:
    environment = metadata["environment"]
    absolute = [
        estimate
        for estimate in estimates
        if estimate.benchmark.startswith(("comparison/", "realworld/"))
        and "/semantic_reference/" in estimate.benchmark
        and estimate.benchmark.endswith(
            ("/dom/build/fast_html_parser", "/evaluate_materialized/fast_html_parser")
        )
    ]
    compact_ratios = [
        ratio
        for ratio in ratios
        if ratio.group.endswith(("/dom/build", "/evaluate_materialized"))
    ]
    lines = [
        "_Generated by `python3 scripts/bench.py publish`; lower time is better._",
        "",
        f"Full report: [{report_path.name}]({report_path.as_posix()})",
        "",
        f"Environment: `{environment['target']}`, "
        f"`rustc {environment['rustc']['release']}`, "
        f"source `{str(metadata['source_digest'])[:12]}`.",
        "",
        "This compact table shows FHP semantic-reference DOM-build and "
        "materialized selector-evaluation rows. Owned, zero-copy, streaming, "
        "and all other absolute estimates remain in the full report; none is "
        "converted into a ratio unless its ID contains `contract_equal`.",
        "",
        _result_table(absolute),
        "",
        "Contract-equal ratios:",
        "",
        _ratio_table(compact_ratios),
    ]
    return "\n".join(lines)


def _report_stem(metadata: Mapping[str, Any]) -> str:
    captured = datetime_module.datetime.fromisoformat(str(metadata["captured_at_utc"]))
    date = captured.astimezone(datetime_module.timezone.utc).date().isoformat()
    target = _safe_component(str(metadata["environment"]["target"]))
    return f"{date}-{str(metadata['source_digest'])[:12]}-{target}"


def command_publish(root: Path) -> int:
    fixtures = validate_fixture_manifest(root)
    readme_path = root / "README.md"
    if not readme_path.is_file():
        raise BenchError("missing README.md")
    _marker_span(readme_path.read_text(encoding="utf-8"))

    metadata = capture_metadata(root, "publish", FULL_MATRIX, fixtures)
    stem = _report_stem(metadata)
    raw_root = (
        root
        / "target"
        / "criterion"
        / "publish"
        / stem
        / _run_namespace()
    )
    commands: List[Dict[str, Any]] = []
    estimates: List[Estimate] = []

    # Run the complete targeted suite once.  FHP is registered first for the
    # initial order-sensitive samples, then those two harnesses are repeated in
    # the middle and last positions to expose registration/thermal bias.
    for harness in FULL_MATRIX:
        rotation = ORDER_ROTATIONS[0] if harness.order_sensitive else None
        home = raw_root / "criterion" / _safe_component(harness.id.replace("/", "__"))
        if rotation:
            home = home / rotation
        commands.append(
            run_harness(
                root,
                harness,
                home,
                PUBLISH_RUN_ARGS,
                rotation,
            )
        )
        parsed = parse_new_estimates(home, harness, rotation)
        if not parsed:
            raise BenchError(f"publish harness produced no estimates: {harness.id}")
        estimates.extend(parsed)

    for rotation in ORDER_ROTATIONS[1:]:
        for harness in (item for item in FULL_MATRIX if item.order_sensitive):
            home = (
                raw_root
                / "criterion"
                / _safe_component(harness.id.replace("/", "__"))
                / rotation
            )
            commands.append(
                run_harness(
                    root,
                    harness,
                    home,
                    PUBLISH_RUN_ARGS,
                    rotation,
                )
            )
            parsed = parse_new_estimates(home, harness, rotation)
            if not parsed:
                raise BenchError(
                    f"publish rotation {rotation} produced no estimates: {harness.id}"
                )
            estimates.extend(parsed)

    assert_run_inputs_unchanged(root, metadata)
    metadata["commands"] = commands
    metadata["publish_rotations"] = list(ORDER_ROTATIONS)
    validate_publish_run_cardinality(estimates, FULL_MATRIX)
    published = aggregate_estimates(estimates)
    ratios = contract_equal_ratios(estimates)
    report_relative = Path("benchmarks") / "results" / f"{stem}.md"
    report_path = root / report_relative
    metadata["report"] = report_relative.as_posix()

    raw_document = {
        "metadata": metadata,
        "runs": [estimate.as_metadata() for estimate in estimates],
        "summary": [estimate.as_metadata() for estimate in published],
        "contract_equal_ratios": [ratio.as_metadata() for ratio in ratios],
    }
    _atomic_write_json(raw_root / "metadata-and-results.json", raw_document)
    _atomic_write_text(
        report_path,
        generate_report_markdown(metadata, published, ratios),
    )
    readme_summary = generate_readme_summary(
        metadata,
        published,
        ratios,
        report_relative,
    )
    # Re-read at write time so unrelated README edits made during the long
    # benchmark run are preserved around the generated marker section.
    update_readme_summary(readme_path, readme_summary)
    print(f"published benchmark summary: {report_relative}")
    print(f"raw machine-local data: {raw_root.relative_to(root)}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run reproducible Criterion benchmark workflows for this workspace."
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("verify", help="validate fixtures and execute every harness once")
    commands.add_parser("quick", help="run the regression namespace in Criterion quick mode")
    save = commands.add_parser("save", help="save a named machine-local Criterion baseline")
    save.add_argument("name")
    save.add_argument(
        "--quick",
        action="store_true",
        help="save only the reduced regression matrix using Criterion quick mode",
    )
    compare = commands.add_parser("compare", help="compare against a compatible named baseline")
    compare.add_argument("name")
    compare.add_argument(
        "--quick",
        action="store_true",
        help="compare the reduced regression matrix using Criterion quick mode",
    )
    commands.add_parser("publish", help="run the publication matrix and update marked docs")
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    arguments = build_parser().parse_args(argv)
    root = repository_root()
    try:
        if arguments.command == "verify":
            return command_verify(root)
        if arguments.command == "quick":
            return command_quick(root)
        if arguments.command == "save":
            return command_save(root, arguments.name, arguments.quick)
        if arguments.command == "compare":
            return command_compare(root, arguments.name, arguments.quick)
        if arguments.command == "publish":
            return command_publish(root)
        raise BenchError(f"unknown command: {arguments.command}")
    except BenchError as error:
        print(f"benchmark error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
