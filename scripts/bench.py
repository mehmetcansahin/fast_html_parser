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


SCHEMA_VERSION = 3
RUSTFLAGS = "-C target-cpu=native"
CARGO_INCREMENTAL = "0"
README_START = "<!-- benchmark-summary:start -->"
README_END = "<!-- benchmark-summary:end -->"
BENCH_INDEX_START = "<!-- latest-benchmark:start -->"
BENCH_INDEX_END = "<!-- latest-benchmark:end -->"
ORDER_ROTATIONS = (
    "fhp-scraper-tl",
    "fhp-tl-scraper",
    "scraper-fhp-tl",
    "scraper-tl-fhp",
    "tl-fhp-scraper",
    "tl-scraper-fhp",
)
DEFAULT_ORDER = ORDER_ROTATIONS[0]
QUICK_RUN_ARGS = ("--quick", "--noplot")
PUBLISH_RUN_ARGS = ("--noplot", "--quiet")
FAIL_THRESHOLD = 0.05
WARN_THRESHOLD = 0.02
EXPECTED_CONFIDENCE_LEVEL = 0.95
P95_P50_LIMIT = 2.0
FAR_OUTLIER_LIMIT = 0.05
ROTATION_SPREAD_LIMIT = 1.10
CALIBRATION_MIN = 0.90
CALIBRATION_MAX = 1.10
CALIBRATION_IDS = (
    "diagnostic/calibration/memcpy_100kb",
    "diagnostic/calibration/tl_parse_100kb",
)
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


FACADE_FEATURES = ("css-selector", "encoding", "entity-decode", "simd")

# Each binary has an isolated Criterion output tree.  The async-only e2e slice
# is separate so enabling Tokio does not duplicate all synchronous samples.
FULL_MATRIX: Tuple[Harness, ...] = (
    Harness("fhp-simd/simd", "fhp-simd", "simd_bench", ("simd",)),
    Harness(
        "fhp-tokenizer/tokenizer",
        "fhp-tokenizer",
        "tokenizer_bench",
        ("entity-decode", "simd"),
    ),
    Harness(
        "fhp-tree/tree",
        "fhp-tree",
        "tree_bench",
        ("encoding", "entity-decode", "simd"),
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
    criterion_median_ns: Optional[float] = None
    normalized_p50_ns: Optional[float] = None
    normalized_p95_ns: Optional[float] = None
    far_outlier_ratio: Optional[float] = None
    attempt: int = 1

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
    criterion_median_ns: Optional[float] = None
    normalized_p50_ns: Optional[float] = None
    normalized_p95_ns: Optional[float] = None
    far_outlier_ratio: Optional[float] = None
    stable: bool = True

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
    stable: bool = True

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class StabilityDecision:
    benchmark: str
    stable: bool
    p95_p50: float
    far_outlier_ratio: float
    rotation_spread: float
    reasons: Tuple[str, ...]

    def as_metadata(self) -> Dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class CalibrationDecision:
    status: str
    median_baseline_current_ratio: Optional[float]
    ratios: Mapping[str, float]
    reason: str

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


def semantic_contract_sha256(root: Path) -> str:
    """Validate and hash the single benchmark semantic contract source."""

    path = root / "benchmarks" / "contracts.json"
    if not path.is_file():
        raise BenchError(f"missing semantic benchmark contract: {path}")
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BenchError(f"invalid semantic benchmark contract {path}: {error}") from error
    if not isinstance(document, dict) or document.get("schema_version") != 1:
        raise BenchError("benchmarks/contracts.json must use semantic contract schema 1")
    canonical = document.get("canonical_dom")
    selectors = document.get("selectors")
    if not isinstance(canonical, dict) or not isinstance(canonical.get("fixtures"), list):
        raise BenchError("benchmarks/contracts.json has no canonical_dom.fixtures array")
    if not canonical["fixtures"] or not isinstance(selectors, list) or not selectors:
        raise BenchError("benchmarks/contracts.json contract arrays cannot be empty")
    return _sha256_file(path)


def _marker_span(
    text: str,
    start_marker: str = README_START,
    end_marker: str = README_END,
) -> Tuple[int, int]:
    if text.count(start_marker) != 1 or text.count(end_marker) != 1:
        raise BenchError("document must contain exactly one generated marker pair")
    start = text.index(start_marker)
    end = text.index(end_marker)
    if start >= end:
        raise BenchError("generated document markers are out of order")
    return start, end


def replace_marked_section(
    text: str,
    replacement: str,
    start_marker: str = README_START,
    end_marker: str = README_END,
) -> str:
    """Replace the README marker body deterministically and idempotently."""

    if start_marker in replacement or end_marker in replacement:
        raise BenchError("generated summary must not contain marker comments")
    start, end = _marker_span(text, start_marker, end_marker)
    before = text[: start + len(start_marker)]
    after = text[end:]
    body = replacement.strip("\n")
    return before + "\n" + body + "\n" + after


def update_readme_summary(
    path: Path,
    replacement: str,
    start_marker: str = README_START,
    end_marker: str = README_END,
) -> None:
    """Replace the generated section using the README's latest on-disk text."""

    current = path.read_text(encoding="utf-8")
    _atomic_write_text(
        path,
        replace_marked_section(current, replacement, start_marker, end_marker),
    )


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
    semantic_contract = root / "benchmarks" / "contracts.json"
    if semantic_contract.is_file():
        files.append(semantic_contract)
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
        "semantic_contract_sha256": semantic_contract_sha256(root),
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
            "semantic_contract_sha256",
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


def ensure_clean_publish_worktree(root: Path) -> None:
    """Reject public benchmark generation from an unreconstructable tree."""

    status = _capture(
        ("git", "status", "--porcelain=v1", "--untracked-files=normal"),
        root,
    )
    if status:
        paths = [line[3:] if len(line) > 3 else line for line in status.splitlines()]
        preview = ", ".join(paths[:5])
        suffix = "" if len(paths) <= 5 else f" (+{len(paths) - 5} more)"
        raise BenchError(
            "official full save/publish requires a clean Git worktree; "
            "commit or otherwise resolve: "
            f"{preview}{suffix}"
        )


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
    metadata["official"] = bool(
        scope == "full"
        and mode in {"save", "compare", "publish"}
        and not metadata["git"]["dirty"]
    )
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
    "semantic_contract_sha256",
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


def _estimate_values(
    document: Mapping[str, Any], path: Path, label: str
) -> Tuple[float, float, float]:
    estimate = document.get(label)
    if not isinstance(estimate, Mapping):
        raise BenchError(f"missing {label} estimate in {path}")
    interval = estimate.get("confidence_interval")
    if not isinstance(interval, Mapping):
        raise BenchError(f"missing {label} confidence interval in {path}")
    confidence_level = _finite_number(
        interval.get("confidence_level"), path, f"{label} CI confidence_level"
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
        _finite_number(estimate.get("point_estimate"), path, f"{label}.point_estimate"),
        _finite_number(interval.get("lower_bound"), path, f"{label} CI lower_bound"),
        _finite_number(interval.get("upper_bound"), path, f"{label} CI upper_bound"),
    )


def _mean_values(document: Mapping[str, Any], path: Path) -> Tuple[float, float, float]:
    return _estimate_values(document, path, "mean")


def _percentile(values: Sequence[float], percentile: float) -> float:
    if not values:
        raise BenchError("cannot calculate a percentile from an empty sample")
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * percentile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] + (ordered[upper] - ordered[lower]) * fraction


def _sample_statistics(
    estimate_path: Path, criterion_median: float
) -> Tuple[float, float, float]:
    sample_path = estimate_path.parent / "sample.json"
    tukey_path = estimate_path.parent / "tukey.json"
    if not sample_path.is_file():
        return criterion_median, criterion_median, 0.0
    sample = _load_json_object(sample_path)
    iterations = sample.get("iters")
    times = sample.get("times")
    if not isinstance(iterations, list) or not isinstance(times, list):
        raise BenchError(f"invalid Criterion sample arrays in {sample_path}")
    if not iterations or len(iterations) != len(times):
        raise BenchError(f"mismatched Criterion sample arrays in {sample_path}")
    normalized = []
    for iteration, elapsed in zip(iterations, times):
        count = _finite_number(iteration, sample_path, "sample iteration count")
        duration = _finite_number(elapsed, sample_path, "sample elapsed time")
        if count <= 0:
            raise BenchError(f"non-positive Criterion iteration count in {sample_path}")
        normalized.append(duration / count)
    p50 = _percentile(normalized, 0.50)
    p95 = _percentile(normalized, 0.95)
    far_ratio = 0.0
    if tukey_path.is_file():
        try:
            tukey = json.loads(tukey_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise BenchError(f"invalid Criterion Tukey data {tukey_path}: {error}") from error
        if not isinstance(tukey, list) or len(tukey) != 4:
            raise BenchError(f"Criterion Tukey data must have four fences: {tukey_path}")
        low_far = _finite_number(tukey[0], tukey_path, "low far-outlier fence")
        high_far = _finite_number(tukey[3], tukey_path, "high far-outlier fence")
        far_ratio = sum(value < low_far or value > high_far for value in normalized) / len(
            normalized
        )
    return p50, p95, far_ratio


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
        median, _, _ = _estimate_values(document, path, "median") if "median" in document else (
            mean,
            lower,
            upper,
        )
        p50, p95, far_outlier_ratio = _sample_statistics(path, median)
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
                criterion_median_ns=median,
                normalized_p50_ns=p50,
                normalized_p95_ns=p95,
                far_outlier_ratio=far_outlier_ratio,
            )
        )
    return estimates


def parse_named_estimates(home: Path, harness: Harness, name: str) -> List[Estimate]:
    estimates: List[Estimate] = []
    for directory in _named_baseline_dirs(home, name):
        path = directory / "estimates.json"
        document = _load_json_object(path)
        mean, lower, upper = _mean_values(document, path)
        median, _, _ = _estimate_values(document, path, "median") if "median" in document else (
            mean,
            lower,
            upper,
        )
        full_id, throughput_kind, throughput_value = _benchmark_identity(
            directory / "benchmark.json"
        )
        estimates.append(
            Estimate(
                benchmark=full_id,
                mean_ns=mean,
                lower_ns=lower,
                upper_ns=upper,
                throughput_kind=throughput_kind,
                throughput_value=throughput_value,
                harness=harness.id,
                criterion_median_ns=median,
                normalized_p50_ns=median,
                normalized_p95_ns=median,
                far_outlier_ratio=0.0,
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


def majority_gate_decisions(
    attempts: Sequence[Sequence[GateDecision]],
) -> List[GateDecision]:
    """Select the median severity across three complete comparison attempts."""

    by_benchmark: Dict[str, List[GateDecision]] = {}
    for attempt in attempts:
        for decision in attempt:
            by_benchmark.setdefault(decision.benchmark, []).append(decision)
    severity = {"info": 0, "pass": 0, "warn": 1, "fail": 2}
    final = []
    for benchmark, decisions in sorted(by_benchmark.items()):
        if len(decisions) == 1:
            final.append(decisions[0])
            continue
        if len(decisions) != 3:
            raise BenchError(
                f"majority gate requires one or three decisions for {benchmark}; "
                f"got {len(decisions)}"
            )
        chosen = sorted(decisions, key=lambda item: severity[item.level])[1]
        final.append(
            GateDecision(
                benchmark=benchmark,
                level=chosen.level,
                reason=f"three-attempt majority: {chosen.reason}",
                point=statistics.median(item.point for item in decisions),
                lower=statistics.median(item.lower for item in decisions),
                upper=statistics.median(item.upper for item in decisions),
            )
        )
    return final


def attempt_requires_rerun(
    gates: Sequence[GateDecision], stability: Sequence[StabilityDecision]
) -> bool:
    """Repeat a full attempt after the first warning, failure, or unstable run."""

    return any(decision.level in {"warn", "fail"} for decision in gates) or any(
        not decision.stable for decision in stability
    )


def assess_stability(estimates: Sequence[Estimate]) -> List[StabilityDecision]:
    grouped: Dict[str, List[Estimate]] = {}
    for estimate in estimates:
        if "/contract_equal/" in estimate.benchmark:
            grouped.setdefault(estimate.benchmark, []).append(estimate)
    decisions = []
    for benchmark, runs in sorted(grouped.items()):
        p50_values = [
            run.normalized_p50_ns or run.criterion_median_ns or run.mean_ns for run in runs
        ]
        p95_values = [
            run.normalized_p95_ns or run.criterion_median_ns or run.mean_ns for run in runs
        ]
        median_values = [run.criterion_median_ns or run.mean_ns for run in runs]
        far_ratio = max((run.far_outlier_ratio or 0.0) for run in runs)
        p95_p50 = max(
            p95 / p50 if p50 > 0 else math.inf
            for p50, p95 in zip(p50_values, p95_values)
        )
        minimum = min(median_values)
        rotation_spread = max(median_values) / minimum if minimum > 0 else math.inf
        reasons = []
        if p95_p50 > P95_P50_LIMIT:
            reasons.append(f"p95/p50 {p95_p50:.3f} exceeds {P95_P50_LIMIT:.2f}")
        if far_ratio > FAR_OUTLIER_LIMIT:
            reasons.append(
                f"far-outlier ratio {far_ratio:.3%} exceeds {FAR_OUTLIER_LIMIT:.0%}"
            )
        if rotation_spread > ROTATION_SPREAD_LIMIT:
            reasons.append(
                f"rotation max/min {rotation_spread:.3f} exceeds {ROTATION_SPREAD_LIMIT:.2f}"
            )
        decisions.append(
            StabilityDecision(
                benchmark=benchmark,
                stable=not reasons,
                p95_p50=p95_p50,
                far_outlier_ratio=far_ratio,
                rotation_spread=rotation_spread,
                reasons=tuple(reasons),
            )
        )
    return decisions


def majority_stability(
    attempts: Sequence[Sequence[StabilityDecision]],
) -> List[StabilityDecision]:
    by_benchmark: Dict[str, List[StabilityDecision]] = {}
    for attempt in attempts:
        for decision in attempt:
            by_benchmark.setdefault(decision.benchmark, []).append(decision)
    final = []
    for benchmark, decisions in sorted(by_benchmark.items()):
        if len(decisions) not in {1, 3}:
            raise BenchError(
                f"stability majority requires one or three decisions for {benchmark}"
            )
        stable = sum(item.stable for item in decisions) >= (2 if len(decisions) == 3 else 1)
        reasons = tuple(
            sorted({reason for item in decisions if not item.stable for reason in item.reasons})
        )
        final.append(
            StabilityDecision(
                benchmark=benchmark,
                stable=stable,
                p95_p50=statistics.median(item.p95_p50 for item in decisions),
                far_outlier_ratio=statistics.median(
                    item.far_outlier_ratio for item in decisions
                ),
                rotation_spread=statistics.median(item.rotation_spread for item in decisions),
                reasons=() if stable else reasons,
            )
        )
    return final


def assess_calibration(
    baseline: Sequence[Estimate], current: Sequence[Estimate]
) -> CalibrationDecision:
    baseline_by_id = {estimate.benchmark: estimate for estimate in baseline}
    current_by_id = {estimate.benchmark: estimate for estimate in current}
    missing = [
        benchmark
        for benchmark in CALIBRATION_IDS
        if benchmark not in baseline_by_id or benchmark not in current_by_id
    ]
    if missing:
        return CalibrationDecision(
            status="unavailable",
            median_baseline_current_ratio=None,
            ratios={},
            reason="missing calibration workload(s): " + ", ".join(missing),
        )
    ratios = {}
    for benchmark in CALIBRATION_IDS:
        old = baseline_by_id[benchmark].criterion_median_ns or baseline_by_id[benchmark].mean_ns
        new = current_by_id[benchmark].criterion_median_ns or current_by_id[benchmark].mean_ns
        if old <= 0 or new <= 0:
            raise BenchError(f"non-positive calibration median for {benchmark}")
        ratios[benchmark] = old / new
    median_ratio = statistics.median(ratios.values())
    status = "pass" if CALIBRATION_MIN <= median_ratio <= CALIBRATION_MAX else "inconclusive"
    reason = (
        "calibration is within the accepted environment band"
        if status == "pass"
        else "inconclusive environment drift; regression results were not normalized"
    )
    return CalibrationDecision(status, median_ratio, ratios, reason)


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
    fixtures = validate_fixture_manifest(root)
    matrix = _quick_matrix()
    namespace = "quick/" + _run_namespace()
    result_count = 0
    commands = []
    estimates = []
    for harness in matrix:
        home = _criterion_home(root, namespace, harness)
        commands.append(run_harness(root, harness, home, QUICK_RUN_ARGS))
        parsed = parse_new_estimates(home, harness)
        if not parsed:
            raise BenchError(f"quick harness produced no Criterion estimates: {harness.id}")
        result_count += len(parsed)
        estimates.extend(parsed)
    metadata = capture_metadata(root, "quick", matrix, fixtures, "quick")
    metadata["commands"] = commands
    metadata["estimates"] = [estimate.as_metadata() for estimate in estimates]
    _atomic_write_json(root / "target" / "criterion" / namespace / "metadata.json", metadata)
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
    if not quick:
        ensure_clean_publish_worktree(root)
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
    if not quick and baseline.get("official") is not True:
        raise BenchError("full comparison requires an official clean-tree schema-3 baseline")

    for message in source_change_messages(baseline, current):
        print(message)

    baseline_index = _baseline_index_from_metadata(baseline, matrix)
    baseline_estimates: List[Estimate] = []
    for harness in matrix:
        home = _normal_harness_home(root, harness)
        actual_ids = parse_saved_baseline_ids(home, name)
        if actual_ids != baseline_index[harness.id]:
            raise BenchError(
                f"saved Criterion baseline IDs for {harness.id} do not match metadata; "
                "the baseline is incomplete or has been modified"
            )
        baseline_estimates.extend(parse_named_estimates(home, harness, name))

    criterion_args = _compare_criterion_args(name, quick)
    commands: List[Dict[str, Any]] = []
    attempt_changes: List[List[ChangeEstimate]] = []
    attempt_decisions: List[List[GateDecision]] = []
    attempt_estimates: List[List[Estimate]] = []
    stability_attempts: List[List[StabilityDecision]] = []
    attempt_coverage: List[Dict[str, Dict[str, List[str]]]] = []

    def run_attempt(attempt: int) -> None:
        changes: List[ChangeEstimate] = []
        estimates: List[Estimate] = []
        coverage: Dict[str, Dict[str, List[str]]] = {}
        for harness in matrix:
            home = _normal_harness_home(root, harness)
            clear_transient_comparison_data(root, home)
            order = DEFAULT_ORDER if harness.order_sensitive else None
            command = run_harness(root, harness, home, criterion_args, order)
            command["attempt"] = attempt
            commands.append(command)
            parsed_current = [
                dataclasses.replace(estimate, attempt=attempt)
                for estimate in parse_new_estimates(home, harness)
            ]
            if not parsed_current:
                raise BenchError(f"comparison produced no current estimates for {harness.id}")
            parsed_changes = parse_change_estimates(home, harness)
            current_ids = {estimate.benchmark for estimate in parsed_current}
            saved_ids = set(baseline_index[harness.id])
            change_ids = {change.benchmark for change in parsed_changes}
            harness_coverage = assess_compare_coverage(
                harness.id, current_ids, saved_ids, change_ids
            )
            uncompared = harness_coverage["uncompared_non_regression_ids"]
            if uncompared:
                print(
                    f"INFO {harness.id}: {len(uncompared)} new non-regression "
                    "benchmark(s) reported without a baseline"
                )
            coverage[harness.id] = harness_coverage
            estimates.extend(parsed_current)
            changes.extend(parsed_changes)
        attempt_estimates.append(estimates)
        attempt_changes.append(changes)
        attempt_decisions.append(classify_changes(changes))
        stability_attempts.append(assess_stability(estimates))
        attempt_coverage.append(coverage)

    run_attempt(1)
    calibration = (
        CalibrationDecision("disabled", None, {}, "quick comparisons are diagnostic")
        if quick
        else assess_calibration(baseline_estimates, attempt_estimates[0])
    )
    needs_rerun = attempt_requires_rerun(
        attempt_decisions[0], stability_attempts[0]
    )
    if needs_rerun and not quick and calibration.status == "pass":
        run_attempt(2)
        run_attempt(3)

    assert_run_inputs_unchanged(root, current)
    decisions = majority_gate_decisions(attempt_decisions)
    stability = majority_stability(stability_attempts)
    current["official"] = bool(
        current["official"]
        and baseline.get("official") is True
        and calibration.status == "pass"
        and all(decision.stable for decision in stability)
    )
    current["baseline"] = name
    current["commands"] = commands
    current["attempt_count"] = len(attempt_decisions)
    current["coverage_attempts"] = attempt_coverage
    current["change_attempts"] = [
        [change.as_metadata() for change in attempt] for attempt in attempt_changes
    ]
    current["decision_attempts"] = [
        [decision.as_metadata() for decision in attempt] for attempt in attempt_decisions
    ]
    current["decisions"] = [decision.as_metadata() for decision in decisions]
    current["stability_attempts"] = [
        [decision.as_metadata() for decision in attempt]
        for attempt in stability_attempts
    ]
    current["stability"] = [decision.as_metadata() for decision in stability]
    current["calibration"] = calibration.as_metadata()
    comparison_dir = root / "target" / "criterion" / "comparisons"
    output_name = f"{_run_namespace()}-vs-{_safe_component(name)}.json"
    output_path = comparison_dir / output_name
    _atomic_write_json(output_path, current)

    if calibration.status in {"unavailable", "inconclusive"}:
        print(f"INCONCLUSIVE {calibration.reason}")
        print(f"comparison metadata: {output_path.relative_to(root)}")
        return 2

    unstable = [decision.benchmark for decision in stability if not decision.stable]
    if unstable:
        print(
            "INCONCLUSIVE contract-equal benchmark(s) remained unstable after "
            f"three full attempts: {', '.join(unstable)}"
        )
        print(f"comparison metadata: {output_path.relative_to(root)}")
        return 2

    _print_gate_summary(decisions)
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
        medians = [run.criterion_median_ns or run.mean_ns for run in runs]
        p50_values = [run.normalized_p50_ns or run.mean_ns for run in runs]
        p95_values = [run.normalized_p95_ns or run.mean_ns for run in runs]
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
                criterion_median_ns=statistics.median(medians),
                normalized_p50_ns=statistics.median(p50_values),
                normalized_p95_ns=statistics.median(p95_values),
                far_outlier_ratio=max((run.far_outlier_ratio or 0.0) for run in runs),
            )
        )
    return published


def validate_publish_run_cardinality(
    estimates: Sequence[Estimate], matrix: Sequence[Harness]
) -> None:
    """Require one complete run, or all six parser permutations, for every ID."""

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
    six permutation-local ratios are summarized.  This avoids dividing separately
    aggregated parser estimates and preserves the publication run cardinality.
    """

    per_run: Dict[Tuple[str, int, Optional[str]], Dict[str, Estimate]] = {}
    for estimate in estimates:
        if "/contract_equal/" not in estimate.benchmark:
            continue
        if "/" not in estimate.benchmark:
            continue
        group, implementation = estimate.benchmark.rsplit("/", 1)
        key = (group, estimate.attempt, estimate.rotation)
        implementations = per_run.setdefault(key, {})
        if implementation in implementations:
            raise BenchError(
                f"duplicate {implementation} estimate for {group} rotation {estimate.rotation}"
            )
        implementations[implementation] = estimate

    samples: Dict[Tuple[str, str], List[float]] = {}
    for (group, attempt, rotation), implementations in sorted(
        per_run.items(), key=lambda item: (item[0][0], item[0][1], item[0][2] or "")
    ):
        fhp = implementations.get("fast_html_parser") or implementations.get("fhp")
        if fhp is None or fhp.mean_ns <= 0:
            continue
        for competitor in sorted(implementations):
            if competitor in {"fast_html_parser", "fhp"}:
                continue
            candidate = implementations[competitor]
            samples.setdefault((group, competitor), []).append(
            (candidate.criterion_median_ns or candidate.mean_ns)
            / (fhp.criterion_median_ns or fhp.mean_ns)
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
        "| Benchmark | Category | Correctness status | Estimate | 95% CI / run range | Criterion median | p50 / p95 | Far outliers | Stable | Throughput | Runs |",
        "|---|---|---|---:|---:|---:|---:|---:|---|---:|---:|",
    ]
    for estimate in estimates:
        interval_label = "95% CI" if estimate.run_count == 1 else "run range"
        interval = (
            f"{interval_label}: {_format_duration(estimate.lower_ns)}–"
            f"{_format_duration(estimate.upper_ns)}"
        )
        lines.append(
            "| `{}` | `{}` | {} | {} | {} | {} | {} / {} | {:.2%} | {} | {} | {} |".format(
                _escape_markdown(estimate.benchmark),
                _escape_markdown(_benchmark_category(estimate.benchmark)),
                _escape_markdown(_correctness_status(estimate.benchmark)),
                _format_duration(estimate.mean_ns),
                interval,
                _format_duration(estimate.criterion_median_ns or estimate.mean_ns),
                _format_duration(estimate.normalized_p50_ns or estimate.mean_ns),
                _format_duration(estimate.normalized_p95_ns or estimate.mean_ns),
                estimate.far_outlier_ratio or 0.0,
                "yes" if estimate.stable else "no",
                _format_throughput(estimate),
                estimate.run_count,
            )
        )
    return "\n".join(lines)


def _ratio_table(ratios: Sequence[ContractRatio]) -> str:
    if not ratios:
        return "No explicit `contract_equal` ratio was available in this run."
    lines = [
        "| Contract-equal group | Competitor | Median competitor/FHP | Run range | Stable | Runs |",
        "|---|---|---:|---:|---|---:|",
    ]
    for ratio in ratios:
        lines.append(
            f"| `{_escape_markdown(ratio.group)}` | `{_escape_markdown(ratio.competitor)}` "
            f"| {ratio.ratio:.3f}× | {ratio.lower:.3f}×–{ratio.upper:.3f}× "
            f"| {'yes' if ratio.stable else 'no'} | {ratio.run_count} |"
        )
    return "\n".join(lines)


_README_FIXTURE_LABELS = (
    ("/synthetic/1kb/", "Synthetic 1 KB"),
    ("/synthetic/100kb/", "Synthetic 100 KB"),
    ("/synthetic/5mb/", "Synthetic 5 MB"),
    ("/realworld/hackernews_34kb/", "Hacker News 34 KB"),
    ("/realworld/github_301kb/", "GitHub 301 KB"),
    ("/realworld/stackoverflow_415kb/", "Stack Overflow 415 KB"),
    ("/realworld/wikipedia_590kb/", "Wikipedia 590 KB"),
)

_README_SELECTOR_LABELS = (
    ("/selector/class_card/", "`.card` selector"),
    ("/selector/tag_p/", "`p` selector"),
    ("/selector/descendant_div_p/", "`div p` selector"),
    ("/selector/class_mw_body/", "`.mw-body` selector"),
    ("/selector/descendant_table_td/", "`table td` selector"),
    ("/selector/link_with_href/", "`a[href]` selector"),
)


def _readme_workload_label(benchmark: str) -> str:
    fixture = next(
        (label for marker, label in _README_FIXTURE_LABELS if marker in benchmark),
        "Benchmark",
    )
    if "/dom/build" in benchmark:
        operation = "DOM build"
    else:
        operation = next(
            (
                label
                for marker, label in _README_SELECTOR_LABELS
                if marker in benchmark
            ),
            "selector evaluation",
        )
    return f"{fixture} — {operation}"


def _readme_result_table(estimates: Sequence[PublishedEstimate]) -> str:
    lines = [
        "| Workload | FHP time | Range | Throughput |",
        "|---|---:|---:|---:|",
    ]
    for estimate in estimates:
        lines.append(
            "| {} | {} | {}–{} | {} |".format(
                _readme_workload_label(estimate.benchmark),
                _format_duration(estimate.mean_ns),
                _format_duration(estimate.lower_ns),
                _format_duration(estimate.upper_ns),
                _format_throughput(estimate),
            )
        )
    return "\n".join(lines)


def _readme_comparison_table(
    estimates: Sequence[PublishedEstimate], ratios: Sequence[ContractRatio]
) -> str:
    if not ratios:
        return "No explicit `contract_equal` ratio was available in this run."
    by_benchmark = {estimate.benchmark: estimate for estimate in estimates}
    lines = [
        "| Equal workload | FHP | Competitor | Ratio |",
        "|---|---|---:|---:|",
    ]
    for ratio in ratios:
        fhp_id = ratio.group + "/fast_html_parser"
        competitor_id = ratio.group + "/" + ratio.competitor
        try:
            fhp = by_benchmark[fhp_id]
            competitor = by_benchmark[competitor_id]
        except KeyError as error:
            raise BenchError(
                f"missing compact comparison estimate for {error.args[0]}"
            ) from error
        lines.append(
            f"| {_readme_workload_label(ratio.group)} "
            f"| {_format_duration(fhp.mean_ns)} "
            f"| `{_escape_markdown(ratio.competitor)}` "
            f"{_format_duration(competitor.mean_ns)} | {ratio.ratio:.3f}× |"
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
    attempt_count = int(metadata.get("attempt_count", 1))
    range_note = (
        "Single-run rows show Criterion's 95% confidence interval. The two "
        "order-sensitive comparison harnesses use all six parser permutations"
        f" across {attempt_count} full attempt(s); their estimate is the median of "
        "run means and the range is min–max. Lower time is better."
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
            f"| Metadata schema | `{metadata['schema_version']}` |",
            f"| Official | `{str(bool(metadata.get('official'))).lower()}` |",
            f"| Source digest | `{metadata['source_digest']}` |",
            f"| Semantic contract digest | `{metadata['semantic_contract_sha256']}` |",
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
            "independent permutation run before paired ratios are summarized.",
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
        and estimate.benchmark.endswith("/dom/build/fast_html_parser")
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
        "Representative FHP DOM-build results. Each range is the minimum and "
        "maximum run mean across all six parser permutations. Owned, "
        "zero-copy, streaming, selector, and all other absolute estimates "
        "remain in the full report.",
        "",
        _readme_result_table(absolute),
        "",
        "Validated equal-work comparisons. Times are permutation medians; the "
        "ratio is the median of paired run-local competitor/FHP ratios. "
        "Values above 1× favor FHP:",
        "",
        _readme_comparison_table(estimates, compact_ratios),
    ]
    return "\n".join(lines)


def generate_benchmark_index_summary(
    metadata: Mapping[str, Any], report_path: Path
) -> str:
    environment = metadata["environment"]
    cpu = environment["cpu"]
    relative_report = report_path.relative_to("benchmarks")
    return "\n".join(
        [
            f"- [Full performance report]({relative_report.as_posix()}) contains ",
            "  absolute estimates, six-permutation ranges, and contract-equal ratios for",
            f"  source digest `{str(metadata['source_digest'])[:12]}` on "
            f"{cpu['model']}.",
            f"  Provenance: [{report_path.stem}.json]"
            f"({relative_report.with_suffix('.json').as_posix()}).",
            "- [Local baseline repeatability report]"
            "(results/2026-07-13-local-baseline-repeatability.md) records a",
            "  same-source `save`/`compare` experiment and targeted reruns. It",
            "  documents measurement stability; it is not a cross-parser speed report.",
        ]
    )


def _report_stem(metadata: Mapping[str, Any]) -> str:
    captured = datetime_module.datetime.fromisoformat(str(metadata["captured_at_utc"]))
    date = captured.astimezone(datetime_module.timezone.utc).date().isoformat()
    target = _safe_component(str(metadata["environment"]["target"]))
    return f"{date}-{str(metadata['source_digest'])[:12]}-{target}"


def command_publish(root: Path) -> int:
    ensure_clean_publish_worktree(root)
    fixtures = validate_fixture_manifest(root)
    readme_path = root / "README.md"
    benchmark_index_path = root / "benchmarks" / "README.md"
    if not readme_path.is_file():
        raise BenchError("missing README.md")
    if not benchmark_index_path.is_file():
        raise BenchError("missing benchmarks/README.md")
    _marker_span(readme_path.read_text(encoding="utf-8"))
    _marker_span(
        benchmark_index_path.read_text(encoding="utf-8"),
        BENCH_INDEX_START,
        BENCH_INDEX_END,
    )

    metadata = capture_metadata(root, "publish", FULL_MATRIX, fixtures)
    stem = _report_stem(metadata)
    raw_root = root / "target" / "criterion" / "publish" / stem / _run_namespace()
    commands: List[Dict[str, Any]] = []
    attempt_estimates: List[List[Estimate]] = []
    stability_attempts: List[List[StabilityDecision]] = []

    def run_attempt(attempt: int) -> None:
        estimates: List[Estimate] = []
        for harness in FULL_MATRIX:
            permutations = ORDER_ROTATIONS if harness.order_sensitive else (None,)
            for permutation in permutations:
                home = (
                    raw_root
                    / "criterion"
                    / f"attempt-{attempt}"
                    / _safe_component(harness.id.replace("/", "__"))
                )
                if permutation:
                    home = home / permutation
                command = run_harness(
                    root,
                    harness,
                    home,
                    PUBLISH_RUN_ARGS,
                    permutation,
                )
                command["attempt"] = attempt
                commands.append(command)
                parsed = [
                    dataclasses.replace(estimate, attempt=attempt)
                    for estimate in parse_new_estimates(home, harness, permutation)
                ]
                if not parsed:
                    raise BenchError(
                        f"publish attempt {attempt} produced no estimates: {harness.id}"
                    )
                estimates.extend(parsed)
        validate_publish_run_cardinality(estimates, FULL_MATRIX)
        attempt_estimates.append(estimates)
        stability_attempts.append(assess_stability(estimates))

    run_attempt(1)
    if any(not decision.stable for decision in stability_attempts[0]):
        run_attempt(2)
        run_attempt(3)

    assert_run_inputs_unchanged(root, metadata)
    stability = majority_stability(stability_attempts)
    estimates = [estimate for attempt in attempt_estimates for estimate in attempt]
    stable_by_benchmark = {decision.benchmark: decision.stable for decision in stability}
    published = [
        dataclasses.replace(
            estimate,
            stable=stable_by_benchmark.get(estimate.benchmark, True),
        )
        for estimate in aggregate_estimates(estimates)
    ]
    ratios = []
    for ratio in contract_equal_ratios(estimates):
        related = [
            stable
            for benchmark, stable in stable_by_benchmark.items()
            if benchmark.startswith(ratio.group + "/")
        ]
        ratios.append(dataclasses.replace(ratio, stable=bool(related) and all(related)))

    metadata["commands"] = commands
    metadata["publish_permutations"] = list(ORDER_ROTATIONS)
    metadata["attempt_count"] = len(attempt_estimates)
    metadata["stability_attempts"] = [
        [decision.as_metadata() for decision in attempt]
        for attempt in stability_attempts
    ]
    metadata["stability"] = [decision.as_metadata() for decision in stability]
    metadata["official"] = bool(
        metadata["official"] and all(decision.stable for decision in stability)
    )
    report_relative = Path("benchmarks") / "results" / f"{stem}.md"
    sidecar_relative = report_relative.with_suffix(".json")
    report_path = root / report_relative
    sidecar_path = root / sidecar_relative
    metadata["report"] = report_relative.as_posix()
    metadata["provenance_sidecar"] = sidecar_relative.as_posix()

    report_text = generate_report_markdown(metadata, published, ratios)
    report_sha256 = hashlib.sha256(report_text.encode("utf-8")).hexdigest()
    raw_document = {
        "metadata": metadata,
        "report_sha256": report_sha256,
        "runs": [estimate.as_metadata() for estimate in estimates],
        "summary": [estimate.as_metadata() for estimate in published],
        "contract_equal_ratios": [ratio.as_metadata() for ratio in ratios],
    }
    _atomic_write_json(raw_root / "metadata-and-results.json", raw_document)

    if not metadata["official"]:
        unstable = [decision.benchmark for decision in stability if not decision.stable]
        raise BenchError(
            "publication remained unstable after three full attempts; latest docs were "
            "not updated. Machine-local evidence: "
            f"{raw_root.relative_to(root)}; unstable IDs: {', '.join(unstable)}"
        )

    _atomic_write_text(report_path, report_text)
    _atomic_write_json(sidecar_path, raw_document)
    readme_summary = generate_readme_summary(
        metadata,
        published,
        ratios,
        report_relative,
    )
    update_readme_summary(readme_path, readme_summary)
    update_readme_summary(
        benchmark_index_path,
        generate_benchmark_index_summary(metadata, report_relative),
        BENCH_INDEX_START,
        BENCH_INDEX_END,
    )
    print(f"published benchmark summary: {report_relative}")
    print(f"published provenance sidecar: {sidecar_relative}")
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
