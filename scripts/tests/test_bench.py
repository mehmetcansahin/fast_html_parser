import copy
import dataclasses
import hashlib
import json
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))
import bench  # noqa: E402


def estimate_document(point, lower, upper):
    return {
        "mean": {
            "confidence_interval": {
                "confidence_level": 0.95,
                "lower_bound": lower,
                "upper_bound": upper,
            },
            "point_estimate": point,
            "standard_error": 0.001,
        }
    }


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def write_sample_statistics(result, normalized):
    write_json(
        result / "sample.json",
        {
            "sampling_mode": "Linear",
            "iters": [1] * len(normalized),
            "times": normalized,
        },
    )
    write_json(
        result / "tukey.json",
        [min(normalized) - 1, min(normalized), max(normalized), max(normalized) + 1],
    )


def write_fixture_repository(root, files):
    fixture_dir = root / "testdata"
    fixture_dir.mkdir(parents=True)
    rows = []
    for name, content in files.items():
        (fixture_dir / name).write_bytes(content)
        rows.append(
            "| `{}` | Synthetic | {:,} | `{}` | Test fixture | N/A |".format(
                name,
                len(content),
                hashlib.sha256(content).hexdigest(),
            )
        )
    manifest = "\n".join(
        [
            "# Fixtures",
            "",
            "| File | Kind | Bytes | SHA-256 | Known source | Capture date |",
            "|---|---:|---:|---|---|---|",
            *rows,
            "",
        ]
    )
    (fixture_dir / "README.md").write_text(manifest, encoding="utf-8")
    return fixture_dir / "README.md"


def write_semantic_contract(root):
    path = root / "benchmarks" / "contracts.json"
    write_json(
        path,
        {
            "schema_version": 1,
            "canonical_dom": {"fixtures": [{"id": "fixture"}]},
            "selectors": [{"fixture_id": "fixture", "id": "selector"}],
        },
    )
    return path


def sample_metadata():
    return {
        "schema_version": 3,
        "official": True,
        "captured_at_utc": "2026-07-11T10:00:00+00:00",
        "mode": "publish",
        "source_digest": "a" * 64,
        "semantic_contract_sha256": "e" * 64,
        "lockfile_sha256": "b" * 64,
        "fixture_manifest_sha256": "d" * 64,
        "git": {"commit": "c" * 40, "branch": "main", "dirty": False},
        "environment": {
            "cpu": {
                "architecture": "aarch64",
                "model": "Example CPU",
                "native_target_features": ["aes", "neon"],
            },
            "os": {"system": "Darwin", "release": "25.0", "version": "test"},
            "rustc": {
                "release": "1.93.0",
                "commit_hash": "rust-commit",
                "host": "aarch64-apple-darwin",
                "llvm_version": "21.1.8",
            },
            "cargo": "cargo 1.93.0",
            "target": "aarch64-apple-darwin",
            "python": "3.13.0",
        },
        "build_contract": {
            "scope": "full",
            "cargo_locked": True,
            "cargo_incremental": "0",
            "rustflags": "-C target-cpu=native",
            "criterion": {
                "version": "0.5.1",
                "quick_mode": False,
                "settings": dict(bench.CRITERION_DEFAULT_SETTINGS),
            },
            "stability": dict(bench.STABILITY_POLICY),
            "matrix": [
                {
                    "id": "example/harness",
                    "package": "example",
                    "bench": "example_bench",
                    "default_features": False,
                    "features": ["feature-a"],
                    "filter": None,
                    "order_sensitive": False,
                }
            ],
        },
        "fixtures": [
            {
                "file": "fixture.html",
                "kind": "Synthetic",
                "bytes": 3,
                "sha256": hashlib.sha256(b"abc").hexdigest(),
                "known_source": "test",
                "capture_date": "N/A",
            }
        ],
        "commands": [
            {
                "harness": "example/harness",
                "command": ["cargo", "bench", "--locked"],
                "criterion_home": "target/criterion/example",
                "environment": {
                    "CARGO_INCREMENTAL": "0",
                    "RUSTFLAGS": "-C target-cpu=native",
                    "FHP_BENCH_ORDER": None,
                },
            }
        ],
    }


class ThresholdPolicyTests(unittest.TestCase):
    def change(self, benchmark, point, lower, upper):
        return bench.ChangeEstimate(benchmark, point, lower, upper, "harness")

    def test_significant_five_percent_regression_fails(self):
        decision = bench.classify_change(
            self.change("regression/parser/build", 0.05, 0.001, 0.09)
        )
        self.assertEqual(decision.level, "fail")

    def test_significant_two_to_five_percent_regression_warns(self):
        decision = bench.classify_change(
            self.change("regression/parser/build", 0.03, 0.005, 0.06)
        )
        self.assertEqual(decision.level, "warn")
        self.assertIn("between 2% and 5%", decision.reason)

    def test_noisy_five_percent_regression_warns(self):
        decision = bench.classify_change(
            self.change("regression/parser/build", 0.08, -0.02, 0.15)
        )
        self.assertEqual(decision.level, "warn")
        self.assertIn("noisy", decision.reason)

    def test_small_or_improving_regressions_pass(self):
        small = bench.classify_change(
            self.change("regression/parser/build", 0.019, 0.005, 0.03)
        )
        improving = bench.classify_change(
            self.change("regression/parser/build", -0.12, -0.15, -0.08)
        )
        self.assertEqual(small.level, "pass")
        self.assertEqual(improving.level, "pass")

    def test_non_regression_namespace_never_gates(self):
        decision = bench.classify_change(
            self.change("comparison/contract_equal/build/tl", 0.40, 0.30, 0.50)
        )
        self.assertEqual(decision.level, "info")


class StabilityAndCalibrationTests(unittest.TestCase):
    def estimate(
        self,
        benchmark,
        median,
        *,
        rotation=None,
        p50=None,
        p95=None,
        far=0.0,
        sample_count=100,
    ):
        return bench.Estimate(
            benchmark=benchmark,
            mean_ns=median,
            lower_ns=median * 0.9,
            upper_ns=median * 1.1,
            throughput_kind=None,
            throughput_value=None,
            harness="harness",
            rotation=rotation,
            criterion_median_ns=median,
            normalized_p50_ns=p50 or median,
            normalized_p95_ns=p95 or median,
            normalized_sample_count=sample_count,
            far_outlier_ratio=far,
        )

    def test_stability_thresholds_and_majority(self):
        benchmark = "comparison/x/contract_equal/all/dom/build/fast_html_parser"
        stable = [
            self.estimate(benchmark, 100 + index, rotation=rotation, p50=100, p95=150)
            for index, rotation in enumerate(bench.ORDER_ROTATIONS)
        ]
        decision = bench.assess_stability(stable)[0]
        self.assertTrue(decision.stable)

        one_noisy_rotation = list(stable)
        one_noisy_rotation[0] = self.estimate(
            benchmark,
            100,
            rotation=bench.ORDER_ROTATIONS[0],
            p50=100,
            p95=150,
            far=0.06,
        )
        pooled = bench.assess_stability(one_noisy_rotation)[0]
        self.assertTrue(pooled.stable)
        self.assertAlmostEqual(pooled.far_outlier_ratio, 0.01)

        noisy = [
            self.estimate(
                benchmark,
                100 + index,
                rotation=rotation,
                p50=100,
                p95=201,
                far=0.06,
            )
            for index, rotation in enumerate(bench.ORDER_ROTATIONS)
        ]
        unstable = bench.assess_stability(noisy)[0]
        self.assertFalse(unstable.stable)
        majority = bench.majority_stability([[unstable], [decision], [decision]])[0]
        self.assertTrue(majority.stable)

    def test_stability_far_outliers_are_weighted_by_sample_count(self):
        benchmark = "comparison/x/contract_equal/all/dom/build/fast_html_parser"
        decision = bench.assess_stability(
            [
                self.estimate(benchmark, 100, far=0.10, sample_count=10),
                self.estimate(benchmark, 100, far=0.00, sample_count=90),
            ]
        )[0]
        self.assertTrue(decision.stable)
        self.assertAlmostEqual(decision.far_outlier_ratio, 0.01)

        exact_limit = bench.assess_stability(
            [self.estimate(benchmark, 100, far=bench.FAR_OUTLIER_LIMIT)]
        )[0]
        self.assertTrue(exact_limit.stable)
        self.assertEqual(exact_limit.far_outlier_ratio, bench.FAR_OUTLIER_LIMIT)

    def test_stability_rejects_missing_criterion_statistics(self):
        benchmark = "comparison/x/contract_equal/all/dom/build/fast_html_parser"
        estimate = self.estimate(benchmark, 100, far=None)
        with self.assertRaisesRegex(
            bench.BenchError, "missing Criterion stability statistics"
        ):
            bench.assess_stability([estimate])

    def test_stability_majority_rejects_attempt_id_drift(self):
        expected_id = "comparison/x/contract_equal/all/dom/build/fast_html_parser"
        extra_id = "comparison/y/contract_equal/all/dom/build/fast_html_parser"

        def decision(benchmark):
            return bench.StabilityDecision(
                benchmark=benchmark,
                stable=True,
                p95_p50=1.1,
                far_outlier_ratio=0.0,
                rotation_spread=1.0,
                reasons=(),
            )

        with self.assertRaisesRegex(bench.BenchError, "benchmark-ID drift") as error:
            bench.majority_stability(
                [
                    [decision(expected_id)],
                    [decision(extra_id)],
                    [decision(expected_id)],
                ]
            )
        self.assertIn(f"missing IDs: {expected_id}", str(error.exception))
        self.assertIn(f"extra IDs: {extra_id}", str(error.exception))

    def test_calibration_detects_environment_drift(self):
        baseline = [self.estimate(benchmark, 100) for benchmark in bench.CALIBRATION_IDS]
        current = [self.estimate(benchmark, 100) for benchmark in bench.CALIBRATION_IDS]
        self.assertEqual(bench.assess_calibration(baseline, current).status, "pass")
        drifted = [self.estimate(benchmark, 80) for benchmark in bench.CALIBRATION_IDS]
        decision = bench.assess_calibration(baseline, drifted)
        self.assertEqual(decision.status, "inconclusive")
        self.assertAlmostEqual(decision.median_baseline_current_ratio, 1.25)

    def test_three_attempt_gate_uses_median_severity(self):
        change = "regression/parser/build"
        decisions = [
            [bench.GateDecision(change, "pass", "pass", 0.0, -0.01, 0.01)],
            [bench.GateDecision(change, "warn", "warn", 0.03, 0.01, 0.05)],
            [bench.GateDecision(change, "fail", "fail", 0.08, 0.06, 0.10)],
        ]
        self.assertEqual(bench.majority_gate_decisions(decisions)[0].level, "warn")

    def test_gate_majority_rejects_attempt_id_drift(self):
        expected = bench.GateDecision("regression/parser/build", "pass", "", 0, 0, 0)
        extra = bench.GateDecision("regression/parser/other", "pass", "", 0, 0, 0)
        with self.assertRaisesRegex(bench.BenchError, "gate decision benchmark-ID drift"):
            bench.majority_gate_decisions([[expected], [extra], [expected]])

    def test_full_attempt_estimate_identity_rejects_non_contract_drift(self):
        first = self.estimate("diagnostic/parser/build", 100, rotation="rotation-a")
        second = dataclasses.replace(first, benchmark="diagnostic/parser/other")
        with self.assertRaisesRegex(bench.BenchError, "estimate benchmark-ID drift"):
            bench._validate_attempt_estimate_ids([[first], [second]], "publish")

    def test_warn_fail_or_instability_repeats_the_full_attempt(self):
        change = "regression/parser/build"
        passing_gate = bench.GateDecision(change, "pass", "pass", 0.0, -0.01, 0.01)
        warning_gate = bench.GateDecision(change, "warn", "warn", 0.03, 0.01, 0.05)
        stable = bench.StabilityDecision(
            benchmark="comparison/x/contract_equal/all/dom/build/fast_html_parser",
            stable=True,
            p95_p50=1.1,
            far_outlier_ratio=0.0,
            rotation_spread=1.0,
            reasons=(),
        )
        unstable = bench.StabilityDecision(
            benchmark=stable.benchmark,
            stable=False,
            p95_p50=stable.p95_p50,
            far_outlier_ratio=stable.far_outlier_ratio,
            rotation_spread=stable.rotation_spread,
            reasons=("noisy",),
        )

        self.assertFalse(bench.attempt_requires_rerun([passing_gate], [stable]))
        self.assertTrue(bench.attempt_requires_rerun([warning_gate], [stable]))
        self.assertTrue(bench.attempt_requires_rerun([passing_gate], [unstable]))


class CriterionJsonTests(unittest.TestCase):
    def test_parse_change_estimate_from_sample_tree(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            benchmark_root = home / "group" / "case"
            write_json(
                benchmark_root / "change" / "estimates.json",
                estimate_document(0.061, 0.012, 0.105),
            )
            write_json(
                benchmark_root / "new" / "benchmark.json",
                {
                    "full_id": "regression/group/case",
                    "throughput": {"Bytes": 1024},
                },
            )
            harness = bench.Harness("test/harness", "test", "bench")
            parsed = bench.parse_change_estimates(home, harness)
            self.assertEqual(len(parsed), 1)
            self.assertEqual(parsed[0].benchmark, "regression/group/case")
            self.assertAlmostEqual(parsed[0].point, 0.061)
            self.assertAlmostEqual(parsed[0].lower, 0.012)

    def test_estimate_rejects_non_95_percent_confidence_interval(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "estimates.json"
            document = estimate_document(10.0, 9.0, 11.0)
            document["mean"]["confidence_interval"]["confidence_level"] = 0.90
            with self.assertRaisesRegex(bench.BenchError, "unexpected Criterion confidence"):
                bench._mean_values(document, path)

    def test_parse_and_aggregate_six_order_permutations(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            harness = bench.Harness("comparison", "test", "bench", order_sensitive=True)
            parsed = []
            for rotation, mean in zip(
                bench.ORDER_ROTATIONS, (90.0, 100.0, 110.0, 120.0, 130.0, 140.0)
            ):
                home = root / rotation
                benchmark_root = home / "group" / "case"
                write_json(
                    benchmark_root / "new" / "estimates.json",
                    estimate_document(mean, mean - 2, mean + 2),
                )
                write_json(
                    benchmark_root / "new" / "benchmark.json",
                    {
                        "full_id": "comparison/x/contract_equal/all/build/fast_html_parser",
                        "throughput": {"Bytes": 2048},
                    },
                )
                write_sample_statistics(benchmark_root / "new", [mean, mean])
                parsed.extend(bench.parse_new_estimates(home, harness, rotation))

            aggregated = bench.aggregate_estimates(parsed)
            self.assertEqual(len(aggregated), 1)
            self.assertEqual(aggregated[0].mean_ns, 115.0)
            self.assertEqual(aggregated[0].lower_ns, 90.0)
            self.assertEqual(aggregated[0].upper_ns, 140.0)
            self.assertEqual(aggregated[0].run_count, 6)
            self.assertEqual(aggregated[0].rotations, bench.ORDER_ROTATIONS)
            self.assertEqual(aggregated[0].normalized_sample_count, 12)
            self.assertEqual(aggregated[0].far_outlier_ratio, 0.0)

    def test_normalized_percentiles_and_far_outliers_are_recorded(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            result = home / "case" / "new"
            document = estimate_document(20.0, 18.0, 22.0)
            document["median"] = estimate_document(10.0, 9.0, 11.0)["mean"]
            write_json(result / "estimates.json", document)
            write_json(result / "benchmark.json", {"full_id": "regression/case"})
            write_json(
                result / "sample.json",
                {"sampling_mode": "Linear", "iters": [1, 2, 4, 8], "times": [10, 22, 48, 400]},
            )
            write_json(result / "tukey.json", [0, 5, 20, 40])
            estimate = bench.parse_new_estimates(
                home, bench.Harness("harness", "package", "bench")
            )[0]
            self.assertEqual(estimate.criterion_median_ns, 10.0)
            self.assertAlmostEqual(estimate.normalized_p50_ns, 11.5)
            self.assertGreater(estimate.normalized_p95_ns, 40.0)
            self.assertEqual(estimate.normalized_sample_count, 4)
            self.assertEqual(estimate.far_outlier_ratio, 0.25)

    def test_missing_sample_or_tukey_artifact_is_rejected(self):
        for missing, message in (
            ("sample.json", "missing Criterion sample data"),
            ("tukey.json", "missing Criterion Tukey data"),
        ):
            with self.subTest(
                missing=missing
            ), tempfile.TemporaryDirectory() as temporary:
                home = Path(temporary)
                result = home / "case" / "new"
                write_json(
                    result / "estimates.json", estimate_document(20.0, 18.0, 22.0)
                )
                write_json(result / "benchmark.json", {"full_id": "regression/case"})
                if missing != "sample.json":
                    write_json(
                        result / "sample.json",
                        {"sampling_mode": "Linear", "iters": [1], "times": [20]},
                    )
                if missing != "tukey.json":
                    write_json(result / "tukey.json", [0, 10, 30, 40])

                with self.assertRaisesRegex(bench.BenchError, message):
                    bench.parse_new_estimates(
                        home, bench.Harness("harness", "package", "bench")
                    )

    def test_malformed_sample_or_tukey_artifact_is_rejected(self):
        for malformed, value, message in (
            (
                "sample.json",
                {"iters": "invalid", "times": []},
                "invalid Criterion sample arrays",
            ),
            ("tukey.json", {"not": "fences"}, "must have four fences"),
        ):
            with self.subTest(
                malformed=malformed
            ), tempfile.TemporaryDirectory() as temporary:
                home = Path(temporary)
                result = home / "case" / "new"
                write_json(
                    result / "estimates.json", estimate_document(20.0, 18.0, 22.0)
                )
                write_json(result / "benchmark.json", {"full_id": "regression/case"})
                write_sample_statistics(result, [20.0, 21.0])
                write_json(result / malformed, value)

                with self.assertRaisesRegex(bench.BenchError, message):
                    bench.parse_new_estimates(
                        home, bench.Harness("harness", "package", "bench")
                    )


class CompatibilityTests(unittest.TestCase):
    def test_locked_criterion_version_is_parsed_without_toml_dependency(self):
        with tempfile.TemporaryDirectory() as temporary:
            lockfile = Path(temporary) / "Cargo.lock"
            lockfile.write_text(
                """version = 4

[[package]]
name = "other"
version = "1.0.0"

[[package]]
name = "criterion"
version = "0.5.1"
""",
                encoding="utf-8",
            )
            self.assertEqual(
                bench.locked_package_version(lockfile, "criterion"), "0.5.1"
            )

    def test_matching_metadata_is_compatible_even_when_source_changes(self):
        baseline = sample_metadata()
        current = copy.deepcopy(baseline)
        current["captured_at_utc"] = "2026-07-12T10:00:00+00:00"
        current["source_digest"] = "d" * 64
        current["lockfile_sha256"] = "e" * 64
        current["git"]["commit"] = "f" * 40
        self.assertEqual(bench.compatibility_errors(baseline, current), [])
        messages = bench.source_change_messages(baseline, current)
        self.assertTrue(any(message.startswith("git commit changed") for message in messages))
        self.assertTrue(any(message.startswith("source changed") for message in messages))
        self.assertIn("Cargo.lock changed since the saved baseline", messages)

    def test_cpu_os_rust_features_flags_and_fixtures_are_enforced(self):
        baseline = sample_metadata()
        current = copy.deepcopy(baseline)
        current["environment"]["cpu"]["model"] = "Different CPU"
        current["environment"]["os"]["release"] = "26.0"
        current["environment"]["rustc"]["release"] = "1.94.0"
        current["environment"]["cargo"] = "cargo 1.94.0"
        current["build_contract"]["rustflags"] = "-C opt-level=2"
        current["build_contract"]["criterion"]["version"] = "0.6.0"
        current["build_contract"]["criterion"]["settings"]["sample_size"] = 200
        current["build_contract"]["stability"]["far_outlier_aggregation"] = (
            "maximum_across_rotations"
        )
        current["build_contract"]["matrix"][0]["features"] = ["feature-b"]
        current["fixtures"][0]["sha256"] = "f" * 64
        errors = bench.compatibility_errors(baseline, current)
        joined = "\n".join(errors)
        self.assertIn("environment.cpu.model", joined)
        self.assertIn("environment.os.release", joined)
        self.assertIn("environment.rustc.release", joined)
        self.assertIn("environment.cargo", joined)
        self.assertIn("build_contract.rustflags", joined)
        self.assertIn("build_contract.criterion", joined)
        self.assertIn("build_contract.stability", joined)
        self.assertIn("build_contract.matrix", joined)
        self.assertIn("fixtures", joined)

    def test_quick_and_full_baseline_scopes_are_incompatible(self):
        baseline = sample_metadata()
        current = copy.deepcopy(baseline)
        current["build_contract"]["scope"] = "quick"
        errors = bench.compatibility_errors(baseline, current)
        self.assertTrue(any("build_contract.scope" in error for error in errors))

    def test_schema_two_and_semantic_contract_drift_are_rejected(self):
        baseline = sample_metadata()
        baseline["schema_version"] = 2
        current = sample_metadata()
        errors = bench.compatibility_errors(baseline, current)
        self.assertTrue(any("schema_version" in error for error in errors))

        baseline = sample_metadata()
        current = copy.deepcopy(baseline)
        current["semantic_contract_sha256"] = "f" * 64
        errors = bench.compatibility_errors(baseline, current)
        self.assertTrue(any("semantic_contract_sha256" in error for error in errors))


class FixtureManifestTests(unittest.TestCase):
    def make_repository(self, root, files):
        return write_fixture_repository(root, files)

    def test_manifest_validates_exact_coverage_bytes_and_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.make_repository(root, {"a.html": b"abc", "b.html": b"1234"})
            records = bench.validate_fixture_manifest(root)
            self.assertEqual([record.file for record in records], ["a.html", "b.html"])
            self.assertEqual(records[0].bytes, 3)
            self.assertEqual(records[0].sha256, hashlib.sha256(b"abc").hexdigest())

    def test_manifest_rejects_wrong_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.make_repository(root, {"a.html": b"abc"})
            text = manifest.read_text(encoding="utf-8")
            manifest.write_text(text.replace(hashlib.sha256(b"abc").hexdigest(), "0" * 64))
            with self.assertRaisesRegex(bench.BenchError, "SHA-256 mismatch"):
                bench.validate_fixture_manifest(root)

    def test_manifest_rejects_missing_and_extra_rows(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.make_repository(root, {"a.html": b"abc", "b.html": b"1234"})
            lines = manifest.read_text(encoding="utf-8").splitlines()
            manifest.write_text("\n".join(line for line in lines if "`b.html`" not in line))
            with self.assertRaisesRegex(bench.BenchError, "missing manifest rows: b.html"):
                bench.validate_fixture_manifest(root)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.make_repository(root, {"a.html": b"abc"})
            extra_hash = hashlib.sha256(b"missing").hexdigest()
            text = manifest.read_text(encoding="utf-8")
            text = (
                text.rstrip()
                + f"\n| `missing.html` | Synthetic | 7 | `{extra_hash}` | Test | N/A |\n"
            )
            manifest.write_text(text, encoding="utf-8")
            with self.assertRaisesRegex(bench.BenchError, "manifest rows without files"):
                bench.validate_fixture_manifest(root)


class RunDriftTests(unittest.TestCase):
    def make_root(self, root):
        (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
        write_fixture_repository(root, {"fixture.html": b"abc"})
        write_semantic_contract(root)

    def test_unchanged_inputs_pass_end_of_run_check(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.make_root(root)
            initial = bench.capture_run_inputs(root)
            bench.assert_run_inputs_unchanged(root, initial)

    def test_source_lock_and_fixture_drift_are_rejected(self):
        mutations = (
            ("source_digest", lambda root: (root / "Cargo.toml").write_text("[workspace]\n# drift\n")),
            ("lockfile_sha256", lambda root: (root / "Cargo.lock").write_text("version = 3\n")),
            ("fixture", lambda root: (root / "testdata" / "fixture.html").write_bytes(b"changed")),
            (
                "fixture_manifest_sha256",
                lambda root: (root / "testdata" / "README.md").write_text(
                    (root / "testdata" / "README.md").read_text() + "\nprose drift\n"
                ),
            ),
            (
                "semantic_contract_sha256",
                lambda root: (root / "benchmarks" / "contracts.json").write_text(
                    json.dumps(
                        {
                            "schema_version": 1,
                            "canonical_dom": {"fixtures": [{"id": "changed"}]},
                            "selectors": [{"fixture_id": "changed", "id": "selector"}],
                        }
                    )
                ),
            ),
        )
        for label, mutate in mutations:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.make_root(root)
                initial = bench.capture_run_inputs(root)
                mutate(root)
                with self.assertRaises(bench.BenchError):
                    bench.assert_run_inputs_unchanged(root, initial)


class MarkdownTests(unittest.TestCase):
    def published(self, benchmark, mean):
        return bench.PublishedEstimate(
            benchmark=benchmark,
            mean_ns=mean,
            lower_ns=mean * 0.9,
            upper_ns=mean * 1.1,
            throughput_kind="Bytes",
            throughput_value=1024.0,
            run_count=1,
            rotations=(),
        )

    def raw(self, benchmark, mean, rotation=bench.DEFAULT_ORDER, harness="comparison"):
        return bench.Estimate(
            benchmark=benchmark,
            mean_ns=mean,
            lower_ns=mean * 0.9,
            upper_ns=mean * 1.1,
            throughput_kind="Bytes",
            throughput_value=1024.0,
            harness=harness,
            rotation=rotation,
        )

    def test_ratios_require_explicit_contract_equal_marker(self):
        estimates = [
            self.raw(
                "comparison/1kb/parse/semantic_reference/build/fast_html_parser", 100
            ),
            self.raw("comparison/1kb/parse/semantic_reference/build/scraper", 400),
            self.raw(
                "comparison/1kb/parse/contract_equal/all_dom/build/fast_html_parser", 100
            ),
            self.raw(
                "comparison/1kb/parse/contract_equal/all_dom/build/scraper", 200
            ),
            self.raw("comparison/1kb/owned/lifecycle/fast_html_parser", 80),
        ]
        ratios = bench.contract_equal_ratios(estimates)
        self.assertEqual(len(ratios), 1)
        self.assertEqual(ratios[0].competitor, "scraper")
        self.assertEqual(ratios[0].ratio, 2.0)

    def test_ratios_are_median_of_within_rotation_ratios(self):
        group = "comparison/1kb/parse/contract_equal/all_dom/build"
        estimates = []
        # Median(comp) / median(FHP) is 30/10 = 3, while the required median
        # of paired ratios is median(2, 3, 1.5) = 2.
        for rotation, fhp, competitor in zip(
            bench.ORDER_ROTATIONS,
            (1.0, 10.0, 100.0, 20.0, 5.0, 50.0),
            (2.0, 30.0, 150.0, 50.0, 20.0, 50.0),
        ):
            estimates.append(self.raw(group + "/fast_html_parser", fhp, rotation))
            estimates.append(self.raw(group + "/scraper", competitor, rotation))
            # A second competitor makes every registration permutation distinct,
            # so all six paired ratios participate for this all-DOM group.
            estimates.append(self.raw(group + "/tl", fhp * 4.0, rotation))
        ratios = bench.contract_equal_ratios(estimates)
        by_competitor = {ratio.competitor: ratio for ratio in ratios}
        scraper = by_competitor["scraper"]
        self.assertEqual(scraper.ratio, 2.25)
        self.assertEqual(scraper.lower, 1.0)
        self.assertEqual(scraper.upper, 4.0)
        self.assertEqual(scraper.run_count, 6)

    def test_two_party_ratio_uses_all_six_runs_and_three_per_binary_order(self):
        group = "comparison/1kb/parse/contract_equal/fhp_scraper/dom/build"
        estimates = []
        for rotation, ratio in zip(bench.ORDER_ROTATIONS, (2.0, 3.0, 4.0, 5.0, 6.0, 7.0)):
            estimates.append(self.raw(group + "/fast_html_parser", 10.0, rotation))
            estimates.append(self.raw(group + "/scraper", 10.0 * ratio, rotation))
        result = bench.contract_equal_ratios(estimates)[0]
        self.assertEqual(result.ratio, 4.5)
        self.assertEqual(result.lower, 2.0)
        self.assertEqual(result.upper, 7.0)
        self.assertEqual(result.run_count, 6)

    def test_report_keeps_absolute_rows_and_marks_ratios(self):
        metadata = sample_metadata()
        estimates = [
            self.published(
                "comparison/1kb/parse/semantic_reference/build/fast_html_parser", 100
            ),
            self.published(
                "comparison/1kb/parse/contract_equal/all_dom/build/fast_html_parser", 100
            ),
            self.published(
                "comparison/1kb/parse/contract_equal/all_dom/build/tl", 120
            ),
            self.published("regression/e2e/streaming/sync/build", 90),
        ]
        ratio_inputs = [
            self.raw(
                "comparison/1kb/parse/contract_equal/all_dom/build/fast_html_parser", 100
            ),
            self.raw(
                "comparison/1kb/parse/contract_equal/all_dom/build/tl", 120
            ),
        ]
        ratios = bench.contract_equal_ratios(ratio_inputs)
        report = bench.generate_report_markdown(metadata, estimates, ratios)
        self.assertIn("semantic_reference", report)
        self.assertIn("streaming/sync/build", report)
        self.assertIn("1.200×", report)
        self.assertIn("contract_equal", report)
        self.assertIn("Harness and feature matrix", report)
        self.assertIn("feature-a", report)
        self.assertIn("| Benchmark | Category | Correctness status |", report)
        self.assertIn("| Fixture | Kind | Bytes | SHA-256 | Known source | Capture date |", report)
        self.assertIn("Criterion settings", report)
        self.assertIn("Stability policy", report)
        self.assertIn("sample_count_weighted_across_rotations", report)
        self.assertEqual(report.count("Single-run rows show"), 1)

    def test_readme_summary_uses_compact_dom_and_equal_work_tables(self):
        metadata = sample_metadata()
        selected_dom = self.published(
            "comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/"
            "semantic_reference/dom/build/fast_html_parser",
            100,
        )
        excluded_selector = self.published(
            "comparison/fast-html-parser/realworld_bench/realworld/wikipedia_590kb/"
            "selector/link_with_href/semantic_reference/"
            "evaluate_materialized/fast_html_parser",
            200,
        )
        excluded = [
            self.published(
                "comparison/1kb/parse/semantic_reference/dom/lifecycle/fast_html_parser", 110
            ),
            self.published(
                "comparison/1kb/parse/semantic_reference/owned/build/fast_html_parser", 90
            ),
            self.published(
                "comparison/1kb/parse/contract_equal/all/dom/build/fast_html_parser", 95
            ),
        ]
        ratios = [
            bench.ContractRatio(
                "comparison/fast-html-parser/comparison_bench/synthetic/1kb/parse/"
                "contract_equal/fhp_scraper_dom/dom/build",
                "scraper",
                2.0,
                1.9,
                2.1,
                3,
            ),
            bench.ContractRatio(
                "comparison/fast-html-parser/comparison_bench/synthetic/100kb/selector/"
                "class_card/contract_equal/fhp_tl/evaluate_materialized",
                "tl",
                1.5,
                1.4,
                1.6,
                3,
            ),
            bench.ContractRatio(
                "comparison/x/dom/lifecycle", "scraper", 3.0, 2.9, 3.1, 3
            ),
            bench.ContractRatio("comparison/x/compile", "tl", 4.0, 3.9, 4.1, 3),
        ]
        equal_estimates = [
            self.published(ratios[0].group + "/fast_html_parser", 100),
            self.published(ratios[0].group + "/scraper", 200),
            self.published(ratios[1].group + "/fast_html_parser", 200),
            self.published(ratios[1].group + "/tl", 300),
        ]
        summary = bench.generate_readme_summary(
            metadata,
            [selected_dom, excluded_selector, *excluded, *equal_estimates],
            ratios,
            Path("benchmarks/results/report.md"),
        )
        self.assertIn("| Workload | FHP time | Range | Throughput |", summary)
        self.assertIn("Synthetic 1 KB — DOM build", summary)
        self.assertIn("| Equal workload | FHP | Competitor | Ratio |", summary)
        self.assertIn("Synthetic 100 KB — `.card` selector", summary)
        self.assertIn("| Synthetic 1 KB — DOM build | 100.00 ns | `scraper` 200.00 ns | 2.000× |", summary)
        self.assertIn("| Synthetic 100 KB — `.card` selector | 200.00 ns | `tl` 300.00 ns | 1.500× |", summary)
        self.assertNotIn(selected_dom.benchmark, summary)
        self.assertNotIn(excluded_selector.benchmark, summary)
        for estimate in excluded:
            self.assertNotIn(estimate.benchmark, summary)
        self.assertIn("`scraper`", summary)
        self.assertIn("`tl`", summary)
        self.assertNotIn("comparison/x/dom/lifecycle", summary)
        self.assertNotIn("comparison/x/compile", summary)

    def test_marker_replacement_is_deterministic_and_idempotent(self):
        original = (
            "before\n"
            + bench.README_START
            + "\nold summary\n"
            + bench.README_END
            + "\nafter\n"
        )
        replaced = bench.replace_marked_section(original, "new summary\n")
        self.assertIn("\nnew summary\n", replaced)
        self.assertNotIn("old summary", replaced)
        self.assertEqual(replaced, bench.replace_marked_section(replaced, "new summary"))
        with self.assertRaisesRegex(bench.BenchError, "marker pair"):
            bench.replace_marked_section("no markers", "summary")

    def test_benchmark_index_summary_uses_generated_report(self):
        summary = bench.generate_benchmark_index_summary(
            sample_metadata(),
            Path("benchmarks/results/2026-07-15-example.md"),
        )
        self.assertIn("results/2026-07-15-example.md", summary)
        self.assertIn("source digest `aaaaaaaaaaaa`", summary)
        self.assertIn("Example CPU", summary)

        document = (
            "before\n"
            + bench.BENCH_INDEX_START
            + "\nold\n"
            + bench.BENCH_INDEX_END
            + "\nafter\n"
        )
        replaced = bench.replace_marked_section(
            document,
            summary,
            bench.BENCH_INDEX_START,
            bench.BENCH_INDEX_END,
        )
        self.assertIn(summary, replaced)
        self.assertNotIn("\nold\n", replaced)

    def test_publish_requires_clean_worktree(self):
        with mock.patch.object(bench, "_capture", return_value=""):
            bench.ensure_clean_publish_worktree(Path("."))
        with mock.patch.object(
            bench,
            "_capture",
            return_value=" M README.md\n?? benchmarks/results/report.md",
        ):
            with self.assertRaisesRegex(bench.BenchError, "clean Git worktree"):
                bench.ensure_clean_publish_worktree(Path("."))

    def test_readme_update_uses_latest_on_disk_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            readme = Path(temporary) / "README.md"
            readme.write_text(
                "latest prefix\n"
                + bench.README_START
                + "\nstale summary\n"
                + bench.README_END
                + "\nlatest suffix\n",
                encoding="utf-8",
            )
            bench.update_readme_summary(readme, "fresh summary")
            updated = readme.read_text(encoding="utf-8")
            self.assertIn("latest prefix", updated)
            self.assertIn("latest suffix", updated)
            self.assertIn("fresh summary", updated)
            self.assertNotIn("stale summary", updated)


class MatrixAndCommandTests(unittest.TestCase):
    def test_contract_equal_parse_override_is_recorded_in_metadata(self):
        with mock.patch.object(
            bench, "locked_package_version", return_value="0.5.1"
        ):
            criterion = bench.criterion_contract(Path("."), "full")
        override = criterion["settings"]["overrides"]["parse_contract_equal"]
        self.assertEqual(
            override,
            {
                "benchmark_group_pattern": "*/parse/contract_equal/*",
                "sampling_mode": "flat",
                "measurement_time_seconds": 10.0,
            },
        )

    def test_production_matrix_enables_simd_explicitly(self):
        by_id = {harness.id: harness for harness in bench.FULL_MATRIX}
        self.assertEqual(by_id["fhp-simd/simd"].features, ("simd",))
        self.assertEqual(
            by_id["fhp-tokenizer/tokenizer"].features,
            ("entity-decode", "simd"),
        )
        self.assertEqual(
            by_id["fhp-tree/tree"].features,
            ("encoding", "entity-decode", "simd"),
        )
        for harness in bench.FULL_MATRIX:
            if harness.package == "fast-html-parser":
                self.assertIn("simd", harness.features)

    def test_full_save_requires_clean_tree_but_quick_metadata_is_unofficial(self):
        with mock.patch.object(
            bench,
            "ensure_clean_publish_worktree",
            side_effect=bench.BenchError("clean tree required"),
        ) as clean:
            with self.assertRaisesRegex(bench.BenchError, "clean tree"):
                bench.command_save(Path("."), "candidate")
            clean.assert_called_once()

        with (
            mock.patch.object(bench, "capture_run_inputs", return_value={}),
            mock.patch.object(bench, "capture_environment", return_value={}),
            mock.patch.object(
                bench,
                "_git_metadata",
                return_value={"commit": "c", "branch": "main", "dirty": True},
            ),
        ):
            metadata = bench.capture_metadata(Path("."), "save", (), (), "quick")
        self.assertFalse(metadata["official"])

    def test_harness_homes_are_unique_and_cargo_is_locked(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            homes = {
                bench._criterion_home(root, "harnesses", harness)
                for harness in bench.FULL_MATRIX
            }
            self.assertEqual(len(homes), len(bench.FULL_MATRIX))
        command = bench.build_cargo_command(bench.FULL_MATRIX[0], ("--test",))
        self.assertEqual(command[:3], ["cargo", "bench", "--locked"])
        self.assertIn("--no-default-features", command)

    def test_fresh_quick_and_publish_runs_keep_new_estimates(self):
        self.assertNotIn("--discard-baseline", bench.QUICK_RUN_ARGS)
        self.assertNotIn("--discard-baseline", bench.PUBLISH_RUN_ARGS)
        quick_command = bench.build_cargo_command(
            bench._quick_matrix()[0], bench.QUICK_RUN_ARGS
        )
        publish_command = bench.build_cargo_command(
            bench.FULL_MATRIX[0], bench.PUBLISH_RUN_ARGS
        )
        self.assertNotIn("--discard-baseline", quick_command)
        self.assertNotIn("--discard-baseline", publish_command)

    def test_comparison_and_realworld_are_distinct_and_order_sensitive(self):
        ordered = [harness for harness in bench.FULL_MATRIX if harness.order_sensitive]
        self.assertEqual(
            [(harness.bench, harness.id) for harness in ordered],
            [
                ("comparison_bench", "fast-html-parser/comparison"),
                ("realworld_bench", "fast-html-parser/realworld"),
            ],
        )

    def test_quick_matrix_contains_only_regression_slices(self):
        quick = bench._quick_matrix()
        self.assertTrue(quick)
        self.assertTrue(all(not harness.order_sensitive for harness in quick))
        self.assertTrue(
            all(
                harness.filter_pattern in {"regression/", "streaming/async"}
                for harness in quick
            )
        )

    def test_save_and_compare_cli_accept_quick_flag(self):
        parser = bench.build_parser()
        save = parser.parse_args(["save", "smoke", "--quick"])
        compare = parser.parse_args(["compare", "smoke", "--quick"])
        self.assertTrue(save.quick)
        self.assertTrue(compare.quick)

    def test_compare_uses_lenient_criterion_baseline(self):
        full = bench._compare_criterion_args("main", False)
        quick = bench._compare_criterion_args("main", True)
        self.assertIn("--baseline-lenient", full)
        self.assertNotIn("--baseline", full)
        self.assertIn("--quick", quick)

    def test_lenient_compare_allows_new_non_regression_but_requires_regression(self):
        coverage = bench.assess_compare_coverage(
            "harness",
            {"regression/existing", "diagnostic/new"},
            {"regression/existing"},
            {"regression/existing"},
        )
        self.assertEqual(
            coverage["uncompared_non_regression_ids"], ["diagnostic/new"]
        )
        with self.assertRaisesRegex(bench.BenchError, "missing saved regression"):
            bench.assess_compare_coverage(
                "harness",
                {"regression/new"},
                set(),
                set(),
            )
        with self.assertRaisesRegex(bench.BenchError, "missing regression change"):
            bench.assess_compare_coverage(
                "harness",
                {"regression/existing"},
                {"regression/existing"},
                set(),
            )

    def test_baseline_name_is_immutable_if_metadata_or_result_exists(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            metadata = bench._baseline_metadata_path(root, "main")
            metadata.parent.mkdir(parents=True)
            metadata.write_text("{}")
            with self.assertRaisesRegex(bench.BenchError, "baselines are immutable"):
                bench.ensure_baseline_name_available(root, "main")

    def test_failed_transactional_save_does_not_commit_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(bench, "validate_fixture_manifest", return_value=[]), mock.patch.object(
                bench, "capture_metadata", return_value=sample_metadata()
            ), mock.patch.object(
                bench, "run_harness", side_effect=bench.BenchError("simulated failure")
            ):
                with self.assertRaisesRegex(bench.BenchError, "did not commit metadata"):
                    bench.command_save(root, "fresh", quick=True)
            self.assertFalse(bench._baseline_metadata_path(root, "fresh").exists())

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = bench._normal_harness_home(root, bench.FULL_MATRIX[0])
            result = home / "group" / "main"
            result.mkdir(parents=True)
            (result / "estimates.json").write_text("{}")
            write_json(result / "benchmark.json", {"full_id": "regression/group/case"})
            with self.assertRaisesRegex(bench.BenchError, "partial artifacts"):
                bench.ensure_baseline_name_available(root, "main")

    def test_benchmark_component_matching_baseline_name_is_not_an_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = bench._normal_harness_home(root, bench.FULL_MATRIX[0])
            nested = home / "group" / "candidate" / "case" / "other-baseline"
            nested.mkdir(parents=True)
            (nested / "estimates.json").write_text("{}")
            write_json(nested / "benchmark.json", {"full_id": "regression/group/case"})
            bench.ensure_baseline_name_available(root, "candidate")

    def test_saved_baseline_index_reads_exact_full_ids(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            for suffix, full_id in (("a", "regression/a"), ("b", "diagnostic/b")):
                result = home / suffix / "main"
                result.mkdir(parents=True)
                (result / "estimates.json").write_text("{}")
                write_json(result / "benchmark.json", {"full_id": full_id})
            self.assertEqual(
                bench.parse_saved_baseline_ids(home, "main"),
                ["diagnostic/b", "regression/a"],
            )

    def test_baseline_names_reject_paths_and_criterion_directories(self):
        bench.validate_baseline_name("main-2026.07")
        for name in ("../main", "new", "", "has space"):
            with self.subTest(name=name):
                with self.assertRaises(bench.BenchError):
                    bench.validate_baseline_name(name)

    def test_publish_cardinality_requires_six_permutations_or_one_plain_run(self):
        ordered = bench.Harness(
            "ordered", "package", "bench", order_sensitive=True
        )
        plain = bench.Harness("plain", "package", "bench")

        def estimate(harness, rotation):
            return bench.Estimate(
                benchmark=f"comparison/{harness.id}/case",
                mean_ns=10,
                lower_ns=9,
                upper_ns=11,
                throughput_kind=None,
                throughput_value=None,
                harness=harness.id,
                rotation=rotation,
            )

        valid = [estimate(ordered, rotation) for rotation in bench.ORDER_ROTATIONS]
        valid.append(estimate(plain, None))
        bench.validate_publish_run_cardinality(valid, (ordered, plain))

        with self.assertRaisesRegex(bench.BenchError, "exactly one run for each rotation"):
            bench.validate_publish_run_cardinality(valid[:-2] + [valid[-1]], (ordered, plain))
        with self.assertRaisesRegex(bench.BenchError, "exactly one unrotated run"):
            bench.validate_publish_run_cardinality(
                valid + [estimate(plain, None)], (ordered, plain)
            )

    def test_compare_cleanup_removes_only_guarded_new_and_change_directories(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = root / "target" / "criterion" / "harnesses" / "example"
            (home / "group" / "main").mkdir(parents=True)
            (home / "group" / "main" / "estimates.json").write_text("{}")
            (home / "group" / "main" / "benchmark.json").write_text("{}")
            (home / "group" / "new").mkdir()
            (home / "group" / "new" / "estimates.json").write_text("stale")
            (home / "group" / "new" / "benchmark.json").write_text("stale")
            (home / "group" / "change").mkdir()
            (home / "group" / "change" / "estimates.json").write_text("stale")
            (home / "group" / "report").mkdir()

            # Benchmark ID components named new/change must not be treated as
            # Criterion result leaves or delete their named baselines.
            for component in ("new", "change"):
                baseline = home / "collision" / component / "case" / "main"
                baseline.mkdir(parents=True)
                (baseline / "estimates.json").write_text("baseline")
                (baseline / "benchmark.json").write_text("baseline")

            bench.clear_transient_comparison_data(root, home)
            self.assertTrue((home / "group" / "main" / "estimates.json").is_file())
            self.assertTrue((home / "group" / "report").is_dir())
            self.assertFalse((home / "group" / "new").exists())
            self.assertFalse((home / "group" / "change").exists())
            self.assertTrue(
                (home / "collision" / "new" / "case" / "main" / "estimates.json").is_file()
            )
            self.assertTrue(
                (
                    home
                    / "collision"
                    / "change"
                    / "case"
                    / "main"
                    / "estimates.json"
                ).is_file()
            )

            outside = root / "outside"
            outside.mkdir()
            with self.assertRaisesRegex(bench.BenchError, "unguarded"):
                bench.clear_transient_comparison_data(root, outside)


if __name__ == "__main__":
    unittest.main()
