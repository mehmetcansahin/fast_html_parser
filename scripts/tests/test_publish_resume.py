"""End-to-end regression tests for checkpointed, resumable benchmark publication.

Only the external ``cargo bench`` process, the toolchain/CPU probe, and the
clock are faked.  The fake cargo writes deterministic Criterion artifacts into
the CRITERION_HOME chosen by ``bench.py``; checkpointing, artifact parsing,
compatibility and input checks, stability decisions, and report generation are
production code running inside temporary Git repositories.  Interruptions are
hard ``os._exit`` calls in a child process, like a reboot: no ``finally`` block
or exception handler runs after the kill.
"""

import contextlib
import copy
import datetime
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock


TEST_FILE = Path(__file__).resolve()
SCRIPTS_DIR = TEST_FILE.parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))
sys.path.insert(0, str(TEST_FILE.parent))
import bench  # noqa: E402
from test_bench import (  # noqa: E402
    estimate_document,
    sample_metadata,
    write_fixture_repository,
    write_json,
    write_sample_statistics,
    write_semantic_contract,
)


CHECKPOINT_NAME = "publication-progress.json"
CHILD_FLAG = "--publish-resume-child"
CHILD_TIMEOUT_SECONDS = 180
KILLED_EXIT = 75
REJECTED_EXIT = 2
COLLIDING_PID = 4242
PARTIAL_VALUE_NS = 9_999_999.0
# The run starts just before midnight UTC and resumes the next morning.
STARTED_AT = "2026-11-12T23:59:30+00:00"
RESUMED_AT = "2026-11-13T08:15:00+00:00"
FIXTURE_NAME = "fixture.html"
FIXTURE_BYTES = b"<p>fixture</p>"
LOCKFILE = 'version = 4\n\n[[package]]\nname = "criterion"\nversion = "0.5.1"\n'
REAL_SUBPROCESS_RUN = subprocess.run
PLAN_SLICES = tuple(
    (harness, order)
    for harness in bench.FULL_MATRIX
    for order in (bench.ORDER_ROTATIONS if harness.order_sensitive else (None,))
)
SLICES_PER_ATTEMPT = len(PLAN_SLICES)
HARNESSES = {harness.id: harness for harness in bench.FULL_MATRIX}


def write_text(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))


def sha256_hex(data):
    return hashlib.sha256(data).hexdigest()


def extended(path):
    """Extended-length Windows form; Criterion homes can approach MAX_PATH."""
    path = Path(path)
    if os.name != "nt" or str(path).startswith("\\\\?\\"):
        return path
    return Path("\\\\?\\" + str(path.resolve()))


def home_key(path):
    return os.path.normcase(os.path.realpath(str(path)))


def is_within(path, directory):
    return home_key(path).startswith(home_key(directory) + os.sep)


def replace_tree(source, destination):
    if extended(destination).exists():
        shutil.rmtree(extended(destination))
    shutil.copytree(extended(source), extended(destination))


def tree_snapshot(directory):
    base = extended(directory)
    return {
        path.relative_to(base).as_posix(): path.read_bytes()
        for path in sorted(base.rglob("*"))
        if path.is_file()
    }


def plan(attempts):
    return [
        (attempt, harness.id, order)
        for attempt in range(1, attempts + 1)
        for harness, order in PLAN_SLICES
    ]


def synthetic_environment():
    environment = sample_metadata()["environment"]
    environment["target"] = "x86_64-test"
    return environment


def frozen_datetime_module(moment):
    frozen = datetime.datetime.fromisoformat(moment)

    class FrozenDateTime(datetime.datetime):
        @classmethod
        def now(cls, tz=None):
            return frozen if tz is None else frozen.astimezone(tz)

    module = types.SimpleNamespace(**vars(datetime))
    module.datetime = FrozenDateTime
    return module


def harness_for(command):
    for harness in bench.FULL_MATRIX:
        if list(command) == bench.build_cargo_command(harness, bench.PUBLISH_RUN_ARGS):
            return harness
    raise AssertionError(f"unexpected cargo command: {command}")


def benchmark_ids(harness):
    component = harness.id.replace("/", "_")
    if harness.order_sensitive:
        group = f"comparison/{component}/contract_equal/dom/build"
        return (f"{group}/fast_html_parser", f"{group}/scraper")
    return (f"diagnostic/{component}/case",)


def measured_ns(session, harness, order, member, unstable):
    # The session is encoded in every value, so published numbers reveal
    # which process actually measured them.
    value = (
        1000.0
        + 10.0 * session
        + bench.FULL_MATRIX.index(harness)
        + 0.1 * (bench.ORDER_ROTATIONS.index(order) if order else 0)
        + 0.01 * member
    )
    if unstable and order == bench.ORDER_ROTATIONS[0] and member == 0:
        value *= 1.5  # exceeds the 1.10 rotation-spread limit
    return value


def write_criterion_result(home, member, benchmark, value):
    new = extended(home) / f"m{member}" / "new"
    document = estimate_document(value, value * 0.99, value * 1.01)
    document["median"] = copy.deepcopy(document["mean"])
    write_json(new / "benchmark.json", {"full_id": benchmark, "throughput": None})
    write_json(new / "estimates.json", document)
    write_sample_statistics(new, [value * factor for factor in (0.99, 0.995, 1.0, 1.005, 1.01)])


class FakeCargo:
    """Stands in for ``cargo bench``; every other subprocess (git) is real."""

    def __init__(self, log_path, session, kill_at=None, unstable_calls=0, on_measure=None):
        self.log_path = Path(log_path)
        self.session = session
        self.kill_at = kill_at
        self.unstable_calls = unstable_calls
        self.on_measure = on_measure
        self.calls = 0

    def __call__(self, command, *args, **kwargs):
        if list(command[:2]) != ["cargo", "bench"]:
            return REAL_SUBPROCESS_RUN(command, *args, **kwargs)
        self.calls += 1
        harness = harness_for(command)
        environment = kwargs["env"]
        home = environment["CRITERION_HOME"]
        order = environment.get("FHP_BENCH_ORDER")
        killed = self.calls == self.kill_at
        values = []
        for member, benchmark in enumerate(benchmark_ids(harness)):
            value = (
                PARTIAL_VALUE_NS
                if killed
                else measured_ns(
                    self.session, harness, order, member, self.calls <= self.unstable_calls
                )
            )
            write_criterion_result(home, member, benchmark, value)
            values.append(value)
            if killed:
                break  # the reboot leaves partial Criterion output behind
        entry = {
            "session": self.session,
            "call": self.calls,
            "harness": harness.id,
            "order": order,
            "home": home,
            "values": values,
            "killed": killed,
        }
        with self.log_path.open("a", encoding="utf-8") as log:
            log.write(json.dumps(entry) + "\n")
        if killed:
            os._exit(KILLED_EXIT)
        if self.on_measure is not None:
            self.on_measure(self.calls)
        return subprocess.CompletedProcess(command, 0)


@contextlib.contextmanager
def fake_lab(cargo, environment, moment, pid=None):
    with contextlib.ExitStack() as stack:
        stack.enter_context(mock.patch("subprocess.run", cargo))
        stack.enter_context(
            mock.patch.object(
                bench, "capture_environment", lambda root: copy.deepcopy(environment)
            )
        )
        stack.enter_context(
            mock.patch.object(bench, "datetime_module", frozen_datetime_module(moment))
        )
        if pid is not None:
            stack.enter_context(mock.patch("os.getpid", return_value=pid))
        yield


def child_main(spec_path):
    spec = json.loads(Path(spec_path).read_text(encoding="utf-8"))
    cargo = FakeCargo(
        spec["log"],
        spec["session"],
        kill_at=spec["kill_at"],
        unstable_calls=spec["unstable_calls"],
    )
    root = Path(spec["root"])
    with fake_lab(cargo, spec["environment"], spec["moment"], spec["pid"]):
        try:
            if spec["resume"] is None:
                code = bench.command_publish(root)
            else:
                code = bench.command_publish(root, resume=Path(spec["resume"]))
        except bench.BenchError as error:
            print(f"benchmark error: {error}", file=sys.stderr)
            code = REJECTED_EXIT
    sys.stdout.flush()
    sys.stderr.flush()
    return code


def make_repository(root):
    write_text(root / ".gitignore", "/target/\n")
    write_text(root / "Cargo.toml", '[workspace]\nmembers = ["crates/core"]\n')
    write_text(root / "Cargo.lock", LOCKFILE)
    write_text(
        root / "crates" / "core" / "Cargo.toml",
        '[package]\nname = "core-example"\nversion = "0.1.0"\n',
    )
    write_text(root / "crates" / "core" / "src" / "lib.rs", "pub fn parse() {}\n")
    write_fixture_repository(root, {FIXTURE_NAME: FIXTURE_BYTES})
    write_semantic_contract(root)
    write_text(
        root / "README.md",
        f"# Example\n\n{bench.README_START}\nold summary\n{bench.README_END}\n",
    )
    write_text(
        root / "benchmarks" / "README.md",
        f"# Benchmarks\n\n{bench.BENCH_INDEX_START}\nold index\n{bench.BENCH_INDEX_END}\n",
    )


class Sandbox:
    """A committed temporary repository plus a cross-process measurement log."""

    def __init__(self):
        self._temporary = tempfile.TemporaryDirectory(prefix="rp-")
        self.base = Path(self._temporary.name).resolve()
        self.root = self.base / "repo"
        self.log = self.base / "measurements.jsonl"
        self.environment = synthetic_environment()
        self._children = 0
        make_repository(self.root)
        for arguments in (
            ("init", "-q"),
            ("config", "user.name", "Bench Test"),
            ("config", "user.email", "bench@example.invalid"),
            ("config", "commit.gpgsign", "false"),
            ("config", "core.autocrlf", "false"),
            ("add", "-A"),
            ("commit", "-q", "--no-verify", "-m", "initial"),
        ):
            self.git(*arguments)
        self.head = self.git("rev-parse", "HEAD")

    def cleanup(self):
        self._temporary.cleanup()

    def git(self, *arguments):
        completed = REAL_SUBPROCESS_RUN(
            ["git", *arguments],
            cwd=str(self.root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if completed.returncode != 0:
            raise AssertionError(f"git {' '.join(arguments)} failed: {completed.stderr}")
        return completed.stdout.strip()

    def status(self):
        return self.git("status", "--porcelain=v1", "--untracked-files=normal")

    def measurements(self, session=None):
        if not self.log.exists():
            return []
        rows = [
            json.loads(line)
            for line in self.log.read_text(encoding="utf-8").splitlines()
            if line
        ]
        return [row for row in rows if session is None or row["session"] == session]

    def run_dirs(self):
        publish = self.root / "target" / "criterion" / "publish"
        return sorted(path for path in publish.glob("*/*") if path.is_dir())

    def publish_child(
        self,
        session,
        *,
        kill_at=None,
        resume=None,
        moment=STARTED_AT,
        unstable_calls=0,
        pid=None,
    ):
        self._children += 1
        spec_path = self.base / f"child-{self._children}.json"
        spec_path.write_text(
            json.dumps(
                {
                    "root": str(self.root),
                    "log": str(self.log),
                    "session": session,
                    "kill_at": kill_at,
                    "resume": None if resume is None else str(resume),
                    "environment": self.environment,
                    "moment": moment,
                    "unstable_calls": unstable_calls,
                    "pid": pid,
                }
            ),
            encoding="utf-8",
        )
        return REAL_SUBPROCESS_RUN(
            [sys.executable, str(TEST_FILE), CHILD_FLAG, str(spec_path)],
            cwd=str(self.root),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1", "PYTHONIOENCODING": "utf-8"},
            timeout=CHILD_TIMEOUT_SECONDS,
            check=False,
        )

    def run_publish(self, *, session, moment, resume=None, environment=None, on_measure=None):
        cargo = FakeCargo(self.log, session, on_measure=on_measure)
        with fake_lab(cargo, environment or self.environment, moment), contextlib.redirect_stdout(
            io.StringIO()
        ):
            if resume is None:
                return bench.command_publish(self.root)
            return bench.command_publish(self.root, resume=resume)

    def resume(self, run_dir, *, environment=None):
        return self.run_publish(
            session=2, moment=RESUMED_AT, resume=run_dir, environment=environment
        )

    @contextlib.contextmanager
    def hidden_edits(self, edits):
        """Change tracked inputs while Git still reports a clean tree."""

        originals = {relative: (self.root / relative).read_bytes() for relative in edits}
        self.git("update-index", "--skip-worktree", *edits)
        try:
            for relative, data in edits.items():
                (self.root / relative).write_bytes(data)
            yield
        finally:
            for relative, data in originals.items():
                (self.root / relative).write_bytes(data)
            self.git("update-index", "--no-skip-worktree", *edits)

    @contextlib.contextmanager
    def extra_commit(self):
        write_text(self.root / "NOTES.md", "unrelated documentation commit\n")
        self.git("add", "NOTES.md")
        self.git("commit", "-q", "--no-verify", "-m", "docs: unrelated note")
        try:
            yield
        finally:
            self.git("reset", "-q", "--hard", self.head)


@unittest.skipUnless(shutil.which("git"), "publication resume tests need git")
class PublishResumeTests(unittest.TestCase):
    def sandbox(self):
        box = Sandbox()
        self.addCleanup(box.cleanup)
        return box

    def interrupt(self, box, kill_at, **options):
        completed = box.publish_child(1, kill_at=kill_at, **options)
        self.assertEqual(completed.returncode, KILLED_EXIT, completed.stdout + completed.stderr)
        measured = box.measurements(1)
        self.assertEqual(len(measured), kill_at)
        run_dirs = box.run_dirs()
        self.assertEqual(len(run_dirs), 1)
        self.assertTrue((run_dirs[0] / CHECKPOINT_NAME).is_file())
        self.assertTrue(all(is_within(entry["home"], run_dirs[0]) for entry in measured))
        self.assertEqual(box.status(), "")
        return run_dirs[0]

    def assert_rejected_without_measuring(self, box, run_dir, **options):
        measured = box.measurements()
        status = box.status()
        checkpoint = Path(run_dir) / CHECKPOINT_NAME
        progress = checkpoint.read_bytes() if checkpoint.is_file() else None
        with self.assertRaises(bench.BenchError):
            box.resume(run_dir, **options)
        self.assertEqual(box.measurements(), measured, "rejected resume ran a benchmark")
        self.assertEqual(box.status(), status, "rejected resume changed tracked files")
        if progress is not None:
            self.assertEqual(checkpoint.read_bytes(), progress, "rejected resume lost progress")

    def resume_and_verify(self, box, run_dir, attempts, unrecorded=None):
        prior = box.measurements()
        if unrecorded is None:
            unrecorded = next(
                (entry for entry in prior if entry["killed"] and is_within(entry["home"], run_dir)),
                None,
            )
        pending = {home_key(unrecorded["home"])} if unrecorded else set()
        recorded = {home_key(entry["home"]) for entry in prior if not entry["killed"]} - pending
        self.assertEqual(box.resume(run_dir), 0)
        homes = [home_key(entry["home"]) for entry in box.measurements()[len(prior):]]
        self.assertFalse(recorded.intersection(homes), "a recorded slice was measured again")
        self.assertEqual(len(set(homes)), len(homes))
        if pending:
            self.assertEqual(homes[0], next(iter(pending)), "unrecorded slice must run first")
        self.assertEqual(len(recorded) + len(homes), attempts * SLICES_PER_ATTEMPT)
        return self.assert_publication(box, attempts)

    def assert_publication(self, box, attempts):
        sidecars = sorted((box.root / "benchmarks" / "results").glob("*.json"))
        self.assertEqual(len(sidecars), 1)
        self.assertTrue(sidecars[0].name.startswith("2026-11-12-"), sidecars[0].name)
        document = json.loads(sidecars[0].read_text(encoding="utf-8"))
        report = sidecars[0].with_suffix(".md").read_text(encoding="utf-8")
        self.assertEqual(sha256_hex(report.encode("utf-8")), document["report_sha256"])
        metadata = document["metadata"]
        self.assertEqual(metadata["captured_at_utc"], STARTED_AT)
        self.assertEqual(metadata["git"]["commit"], box.head)
        self.assertFalse(metadata["git"]["dirty"])
        self.assertEqual(metadata["environment"], box.environment)
        self.assertTrue(metadata["official"])
        self.assertEqual(metadata["attempt_count"], attempts)

        commands = metadata["commands"]
        identities = [
            (command["attempt"], command["harness"], command["environment"]["FHP_BENCH_ORDER"])
            for command in commands
        ]
        self.assertEqual(identities, plan(attempts))
        homes = {}
        for identity, command in zip(identities, commands):
            self.assertEqual(
                command["command"],
                bench.build_cargo_command(HARNESSES[command["harness"]], bench.PUBLISH_RUN_ARGS),
            )
            homes[identity] = home_key(box.root / command["criterion_home"])
        self.assertEqual(len(set(homes.values())), len(homes))

        # Every published number must come from the last completed measurement
        # of its own slice: never a partial, stale, or foreign artifact.
        final = {
            home_key(entry["home"]): entry for entry in box.measurements() if not entry["killed"]
        }
        self.assertEqual(set(final), set(homes.values()))
        runs = document["runs"]
        self.assertEqual(
            len(runs), attempts * sum(len(benchmark_ids(harness)) for harness, _ in PLAN_SLICES)
        )
        for row in runs:
            measurement = final[homes[(row["attempt"], row["harness"], row["rotation"])]]
            member = benchmark_ids(HARNESSES[row["harness"]]).index(row["benchmark"])
            value = measurement["values"][member]
            self.assertEqual((row["mean_ns"], row["criterion_median_ns"]), (value, value), row)
        return document

    def test_interrupted_slice_is_rerun_and_recorded_slices_keep_original_values(self):
        third_permutation = 1 + next(
            index
            for index, (_, order) in enumerate(PLAN_SLICES)
            if order == bench.ORDER_ROTATIONS[2]
        )
        # kill_at=1 also proves the checkpoint exists before the first measurement.
        for kill_at in (1, third_permutation):
            with self.subTest(kill_at=kill_at):
                box = self.sandbox()
                run_dir = self.interrupt(box, kill_at)
                document = self.resume_and_verify(box, run_dir, attempts=1)
                sessions = document["metadata"]["resume_sessions"]
                self.assertEqual([session["resumed_at_utc"] for session in sessions], [RESUMED_AT])

    def test_resume_in_later_stability_attempt_reuses_completed_attempts(self):
        for kill_at in (SLICES_PER_ATTEMPT + 1, 2 * SLICES_PER_ATTEMPT + 3):
            with self.subTest(kill_at=kill_at):
                box = self.sandbox()
                run_dir = self.interrupt(box, kill_at, unstable_calls=SLICES_PER_ATTEMPT)
                document = self.resume_and_verify(box, run_dir, attempts=3)
                stable = [
                    [decision["stable"] for decision in attempt]
                    for attempt in document["metadata"]["stability_attempts"]
                ]
                # Attempt-1 instability exists only in the original artifacts.
                self.assertIn(False, stable[0])
                self.assertNotIn(False, stable[1] + stable[2])

    def test_input_drift_during_measurement_discards_that_slice(self):
        box = self.sandbox()
        source = box.root / "crates" / "core" / "src" / "lib.rs"
        original = source.read_bytes()
        drift_at = 6

        def edit_source_while_measuring(call):
            if call == drift_at:
                source.write_bytes(b"pub fn parse() { /* edited mid-run */ }\n")

        with self.assertRaises(bench.BenchError):
            box.run_publish(session=1, moment=STARTED_AT, on_measure=edit_source_while_measuring)
        measured = box.measurements(1)
        self.assertEqual(len(measured), drift_at)
        self.assertFalse((box.root / "benchmarks" / "results").exists())
        run_dirs = box.run_dirs()
        self.assertEqual(len(run_dirs), 1)
        progress = json.loads((run_dirs[0] / CHECKPOINT_NAME).read_text(encoding="utf-8"))
        self.assertEqual(len(progress["completed_runs"]), drift_at - 1)

        source.write_bytes(original)
        self.assertEqual(box.status(), "")
        self.resume_and_verify(box, run_dirs[0], attempts=1, unrecorded=measured[-1])

    def test_input_environment_build_and_commit_drift_rejected_before_measuring(self):
        box = self.sandbox()
        run_dir = self.interrupt(box, 3)

        def check(label, drift=contextlib.nullcontext, environment=None):
            with self.subTest(drift=label), drift():
                self.assertEqual(box.status(), "", "drift must be caught without a dirty tree")
                self.assert_rejected_without_measuring(box, run_dir, environment=environment)

        changed_fixture = b"<p>fixturf</p>"
        manifest = (box.root / "testdata" / "README.md").read_bytes()
        contract_path = box.root / "benchmarks" / "contracts.json"
        contract = json.loads(contract_path.read_text(encoding="utf-8"))
        contract["selectors"].append({"fixture_id": "fixture", "id": "another"})
        locked_dependency = '\n[[package]]\nname = "itoa"\nversion = "1.0.11"\n'
        hidden_input_edits = {
            "source": {"crates/core/src/lib.rs": b"pub fn parse() { }\n"},
            "Cargo.lock": {"Cargo.lock": (LOCKFILE + locked_dependency).encode()},
            "fixture and manifest": {
                f"testdata/{FIXTURE_NAME}": changed_fixture,
                "testdata/README.md": manifest.replace(
                    sha256_hex(FIXTURE_BYTES).encode(), sha256_hex(changed_fixture).encode()
                ),
            },
            "semantic contract": {"benchmarks/contracts.json": json.dumps(contract).encode()},
        }
        for label, edits in hidden_input_edits.items():
            check(label, lambda: box.hidden_edits(edits))

        check("commit", box.extra_commit)

        environment_changes = {
            "CPU model": lambda e: e["cpu"].update(model="Other CPU"),
            "CPU features": lambda e: e["cpu"]["native_target_features"].append("sve2"),
            "rustc": lambda e: e["rustc"].update(release="1.94.0"),
        }
        for label, change in environment_changes.items():
            environment = copy.deepcopy(box.environment)
            change(environment)
            check(label, environment=environment)

        build_contract_changes = {
            "RUSTFLAGS": lambda: mock.patch.object(bench, "RUSTFLAGS", "-C target-cpu=x86-64"),
            "Criterion settings": lambda: mock.patch.dict(
                bench.CRITERION_DEFAULT_SETTINGS, {"sample_size": 50}
            ),
            "matrix": lambda: mock.patch.object(bench, "FULL_MATRIX", bench.FULL_MATRIX[:-1]),
        }
        for label, drift in build_contract_changes.items():
            check(label, drift)

        with self.subTest(drift="partially written generated docs"):
            readme = box.root / "README.md"
            readme.write_bytes(readme.read_bytes().replace(b"old summary", b"half-written"))
            try:
                self.assert_rejected_without_measuring(box, run_dir)
            finally:
                box.git("checkout", "-q", "--", "README.md")

        # Rejections are non-destructive: with inputs restored, the run finishes.
        self.resume_and_verify(box, run_dir, attempts=1)

    def test_missing_or_altered_raw_artifacts_rejected_before_measuring(self):
        box = self.sandbox()
        run_dir = self.interrupt(box, 4)
        other, target = (Path(entry["home"]) for entry in box.measurements(1)[:2])
        result = extended(target) / "m0" / "new"
        backup = box.base / "backup"
        replace_tree(target, backup)

        def rewrite_json(name, change):
            path = result / name
            document = json.loads(path.read_text(encoding="utf-8"))
            change(document)
            path.write_text(json.dumps(document), encoding="utf-8")

        def shift_mean():
            rewrite_json("estimates.json", lambda d: d["mean"].update(point_estimate=1001.0))

        def stretch_sample_tail():
            rewrite_json("sample.json", lambda d: d["times"].append(d["times"][-1] * 1.5))
            rewrite_json("sample.json", lambda d: d["iters"].append(1.0))

        def remove_sample():
            (result / "sample.json").unlink()

        def remove_slice():
            shutil.rmtree(extended(target))

        def inject_benchmark():
            write_criterion_result(target, 9, "diagnostic/injected/case", 1.0)

        def copy_other_slice():
            replace_tree(other, target)

        for tamper in (
            shift_mean,
            stretch_sample_tail,
            remove_sample,
            remove_slice,
            inject_benchmark,
            copy_other_slice,
        ):
            with self.subTest(tamper=tamper.__name__):
                tamper()
                try:
                    self.assert_rejected_without_measuring(box, run_dir)
                finally:
                    replace_tree(backup, target)

        self.resume_and_verify(box, run_dir, attempts=1)

    def test_malformed_reordered_or_forged_checkpoint_rejected(self):
        box = self.sandbox()
        run_dir = self.interrupt(box, 4)
        checkpoint = run_dir / CHECKPOINT_NAME
        original = checkpoint.read_bytes()
        pristine = json.loads(original.decode("utf-8"))
        # Only measured AND parsed slices are durable; the killed one is not.
        self.assertEqual(len(pristine["completed_runs"]), 3)

        def mutated(change):
            document = copy.deepcopy(pristine)
            change(document)
            return json.dumps(document, indent=2, sort_keys=True).encode("utf-8")

        def second_command(document):
            return document["completed_runs"][1]["command"]

        def first_home(document):
            return document["completed_runs"][0]["command"]["criterion_home"]

        def metadata(document):
            return document["metadata"]

        unrelated_home = str(Path("target") / "criterion" / "harnesses" / "fhp-simd__simd")
        changes = {
            "unknown schema": lambda d: d.update(schema_version=2),
            "cached numbers": lambda d: d.update(cached_estimates=[]),
            "reordered records": lambda d: d["completed_runs"].reverse(),
            "missing first record": lambda d: d["completed_runs"].pop(0),
            "duplicated record": lambda d: d["completed_runs"].append(d["completed_runs"][-1]),
            "attempt": lambda d: second_command(d).update(attempt=2),
            "permutation": lambda d: second_command(d)["environment"].update(
                FHP_BENCH_ORDER=bench.ORDER_ROTATIONS[0]
            ),
            "CLI": lambda d: second_command(d)["command"].append("--quick"),
            "home of another slice": lambda d: second_command(d).update(
                criterion_home=first_home(d)
            ),
            "home outside run": lambda d: second_command(d).update(criterion_home=unrelated_home),
            "digest": lambda d: d["completed_runs"][1].update(estimates_sha256="0" * 64),
            "digest missing": lambda d: d["completed_runs"][1].pop("estimates_sha256"),
            "CPU": lambda d: metadata(d)["environment"]["cpu"].update(model="Forged CPU"),
            "unofficial": lambda d: metadata(d).update(official=False),
            "mode": lambda d: metadata(d).update(mode="save"),
            "quick scope": lambda d: metadata(d)["build_contract"].update(scope="quick"),
            "dirty tree": lambda d: metadata(d)["git"].update(dirty=True),
            "commit": lambda d: metadata(d)["git"].update(commit="f" * 40),
            "source digest": lambda d: metadata(d).update(source_digest="0" * 64),
            "fixture digest": lambda d: metadata(d)["fixtures"][0].update(sha256="0" * 64),
            "capture date": lambda d: metadata(d).update(
                captured_at_utc="2026-11-01T00:00:00+00:00"
            ),
        }
        cases = {
            "truncated": original[: len(original) // 2],
            "not an object": b"[]\n",
            **{label: mutated(change) for label, change in changes.items()},
        }
        for label, data in cases.items():
            with self.subTest(checkpoint=label):
                checkpoint.write_bytes(data)
                try:
                    self.assert_rejected_without_measuring(box, run_dir)
                finally:
                    checkpoint.write_bytes(original)

        self.resume_and_verify(box, run_dir, attempts=1)

    def test_runs_without_checkpoint_or_outside_publish_tree_rejected(self):
        box = self.sandbox()
        run_dir = self.interrupt(box, 3)
        relative = run_dir.relative_to(box.root / "target" / "criterion" / "publish")
        copies = (
            box.base / "elsewhere" / relative,
            box.root / "target" / "criterion" / "archive" / relative,
        )
        for copied in copies:
            replace_tree(run_dir, copied)
        for candidate in (*copies, run_dir.parent, run_dir / "missing"):
            with self.subTest(run_directory=str(candidate)):
                self.assert_rejected_without_measuring(box, candidate)

        # What a pre-checkpoint publish leaves behind: raw data, no progress file.
        (run_dir / CHECKPOINT_NAME).unlink()
        with self.subTest(run_directory="without checkpoint"):
            self.assert_rejected_without_measuring(box, run_dir)

    def test_completed_publication_is_never_measured_again(self):
        box = self.sandbox()
        self.assertEqual(box.run_publish(session=1, moment=STARTED_AT), 0)
        published = self.assert_publication(box, attempts=1)
        run_dirs = box.run_dirs()
        self.assertEqual(len(run_dirs), 1)

        self.assertNotEqual(box.status(), "")
        self.assert_rejected_without_measuring(box, run_dirs[0])

        box.git("checkout", "-q", "--", ".")
        box.git("clean", "-fdq", "--", "benchmarks/results")
        self.assertEqual(box.status(), "")
        measured = box.measurements()
        try:
            outcome = box.resume(run_dirs[0])
        except bench.BenchError:
            outcome = None
        self.assertEqual(box.measurements(), measured)
        if outcome is not None:
            self.assertEqual(outcome, 0)
            self.assertEqual(self.assert_publication(box, attempts=1)["runs"], published["runs"])

    def test_concurrent_publish_and_resume_rejected_while_running(self):
        box = self.sandbox()
        competitors = []

        def start_competitors(call):
            if call == 1:
                run_dirs = box.run_dirs()
                self.assertEqual(len(run_dirs), 1)
                competitors.append(box.publish_child(2, resume=run_dirs[0]))
                competitors.append(box.publish_child(3))

        self.assertEqual(
            box.run_publish(session=1, moment=STARTED_AT, on_measure=start_competitors), 0
        )
        self.assertEqual(
            [completed.returncode for completed in competitors],
            [REJECTED_EXIT, REJECTED_EXIT],
            [completed.stderr for completed in competitors],
        )
        self.assertEqual(box.measurements(2) + box.measurements(3), [])
        self.assert_publication(box, attempts=1)

    def test_fresh_publish_namespace_collision_keeps_existing_checkpoint(self):
        box = self.sandbox()
        run_dir = self.interrupt(box, 4, pid=COLLIDING_PID)
        snapshot = tree_snapshot(run_dir)
        # Same second and same PID produce the same run-directory name.
        second = box.publish_child(2, kill_at=1, pid=COLLIDING_PID)
        self.assertIn(second.returncode, (REJECTED_EXIT, KILLED_EXIT), second.stderr)
        self.assertEqual(tree_snapshot(run_dir), snapshot)
        self.resume_and_verify(box, run_dir, attempts=1)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == CHILD_FLAG:
        raise SystemExit(child_main(sys.argv[2]))
    unittest.main()
