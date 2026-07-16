#!/usr/bin/env python3
"""Keep the license texts embedded in every publishable crate identical."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys


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
LICENSES = ("LICENSE-MIT", "LICENSE-APACHE")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    stale: list[str] = []
    for license_name in LICENSES:
        source = (ROOT / license_name).read_bytes()
        for crate in CRATES:
            destination = ROOT / "crates" / crate / license_name
            if destination.is_file() and destination.read_bytes() == source:
                continue
            if args.check:
                stale.append(destination.relative_to(ROOT).as_posix())
            else:
                destination.write_bytes(source)

    if stale:
        print("license copies are missing or stale:", file=sys.stderr)
        for path in stale:
            print(f"  {path}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
