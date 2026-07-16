import io
import hashlib
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))
import release  # noqa: E402
import bench  # noqa: E402


class ReleaseScriptTests(unittest.TestCase):
    def test_cli_requires_release_version(self):
        parser = release.build_parser()
        self.assertEqual(parser.parse_args(["check"]).command, "check")
        parsed = parser.parse_args(["release", "--version", "0.2.0"])
        self.assertEqual(parsed.version, "0.2.0")

    def test_version_validation_checks_internal_requirements(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            crate = root / "crates" / "sample"
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                """[package]
name = "sample"
version = "0.2.0"

[dependencies]
other = { version = "0.1.0", path = "../other" }
""",
                encoding="utf-8",
            )
            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(release, "CRATES", ("sample", "other")),
                mock.patch.object(release, "package_manifests", return_value=[crate / "Cargo.toml"]),
            ):
                with self.assertRaisesRegex(release.ReleaseError, "requires other"):
                    release.validate_versions("0.2.0")

    def test_package_audit_requires_both_licenses(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "LICENSE-MIT").write_bytes(b"MIT")
            (root / "LICENSE-APACHE").write_bytes(b"Apache")
            package = root / "target" / "package" / "sample-0.2.0.crate"
            package.parent.mkdir(parents=True)
            with tarfile.open(package, "w:gz") as archive:
                for name, payload in (
                    ("Cargo.toml", b"[package]\nname='sample'\nversion='0.2.0'\n"),
                    ("LICENSE-MIT", b"MIT"),
                ):
                    info = tarfile.TarInfo(f"sample-0.2.0/{name}")
                    info.size = len(payload)
                    archive.addfile(info, io.BytesIO(payload))
            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(release, "CRATES", ("sample",)),
            ):
                with self.assertRaisesRegex(release.ReleaseError, "LICENSE-APACHE"):
                    release.audit_package_archives("0.2.0")

    def test_benchmark_release_provenance_requires_ancestor_and_digests(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            results = root / "benchmarks" / "results"
            results.mkdir(parents=True)
            contract = root / "benchmarks" / "contracts.json"
            contract.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "canonical_dom": {"fixtures": [{"id": "fixture"}]},
                        "selectors": [{"fixture_id": "fixture", "id": "selector"}],
                    }
                ),
                encoding="utf-8",
            )
            report = results / "official.md"
            commit = "c" * 40
            source_digest = bench.source_digest(root)
            report_text = (
                f"| Git commit | `{commit}` (clean) |\n"
                f"| Source digest | `{source_digest}` |\n"
            )
            report.write_text(report_text, encoding="utf-8")
            (root / "benchmarks" / "README.md").write_text(
                "[Full performance report](results/official.md)", encoding="utf-8"
            )
            provenance = {
                "metadata": {
                    "schema_version": 3,
                    "official": True,
                    "source_digest": source_digest,
                    "semantic_contract_sha256": bench.semantic_contract_sha256(root),
                    "report": "benchmarks/results/official.md",
                    "git": {"commit": commit, "dirty": False},
                },
                "report_sha256": hashlib.sha256(report_text.encode()).hexdigest(),
            }
            report.with_suffix(".json").write_text(
                json.dumps(provenance), encoding="utf-8"
            )
            completed = mock.Mock(returncode=0, stderr="")
            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(release, "capture", return_value="d" * 40),
                mock.patch.object(release.subprocess, "run", return_value=completed) as run,
            ):
                release.validate_clean_benchmark_report()
            run.assert_called_once()
            self.assertEqual(run.call_args.args[0][:3], ["git", "merge-base", "--is-ancestor"])


if __name__ == "__main__":
    unittest.main()
