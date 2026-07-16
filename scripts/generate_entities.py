#!/usr/bin/env python3
"""Vendor WHATWG named entities and generate the Rust lookup tables.

The generated Rust source is checked in so normal Cargo builds never access
the network. Refreshing the vendored JSON is an explicit maintainer action.

Names that require a trailing semicolon live in a perfect-hash map. The much
smaller set of names that WHATWG permits without a semicolon lives in a trie,
where longest-prefix matching is required.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from dataclasses import dataclass, field
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
VENDORED = ROOT / "vendor" / "whatwg" / "entities.json"
GENERATED = ROOT / "crates" / "fhp-core" / "src" / "generated_entities.rs"
SOURCE_URL = "https://html.spec.whatwg.org/entities.json"
EXPECTED_SOURCE_SHA256 = "d741d877ac77c4194c4ad526b5b4a19aef8dfe411ab840a466891cdbb9f362e6"
NO_VALUE = 0xFFFF
NO_NODE = 0xFFFF


@dataclass
class TrieNode:
    children: dict[int, "TrieNode"] = field(default_factory=dict)
    value: str | None = None
    index: int = -1


def parse_source(raw: bytes) -> tuple[dict[str, str], dict[str, str], int, int, int]:
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise ValueError("WHATWG entity source must be a JSON object")

    exact: dict[str, str] = {}
    legacy: dict[str, str] = {}
    max_name_len = 0
    max_legacy_name_len = 0
    for source_name, record in sorted(data.items()):
        if not source_name.startswith("&") or not isinstance(record, dict):
            raise ValueError(f"invalid entity record: {source_name!r}")
        replacement = record.get("characters")
        if not isinstance(replacement, str):
            raise ValueError(f"missing characters for {source_name!r}")

        has_semicolon = source_name.endswith(";")
        name = source_name[1:-1] if has_semicolon else source_name[1:]
        if not name or not name.isascii():
            raise ValueError(f"entity name must be non-empty ASCII: {source_name!r}")
        max_name_len = max(max_name_len, len(name))

        destination = exact if has_semicolon else legacy
        previous = destination.setdefault(name, replacement)
        if previous != replacement:
            raise ValueError(f"conflicting replacements for {source_name!r}")
        if not has_semicolon:
            max_legacy_name_len = max(max_legacy_name_len, len(name))

    # WHATWG provides a semicolon-terminated counterpart for every legacy
    # spelling. Keep that invariant explicit so the two lookup paths cannot
    # silently disagree.
    for name, replacement in legacy.items():
        if exact.get(name) != replacement:
            raise ValueError(f"legacy entity has no matching exact record: {name!r}")

    return exact, legacy, max_name_len, max_legacy_name_len, len(data)


def build_trie(entries: dict[str, str]) -> TrieNode:
    root = TrieNode()
    for name, replacement in sorted(entries.items()):
        node = root
        for byte in name.encode("ascii"):
            node = node.children.setdefault(byte, TrieNode())
        if node.value is not None and node.value != replacement:
            raise ValueError(f"conflicting replacements for {name!r}")
        node.value = replacement

    return root


def flatten(root: TrieNode) -> list[TrieNode]:
    nodes: list[TrieNode] = []

    def visit(node: TrieNode) -> None:
        node.index = len(nodes)
        nodes.append(node)
        for _, child in sorted(node.children.items()):
            visit(child)

    visit(root)
    return nodes


def rust_string(value: str) -> str:
    escaped: list[str] = []
    for char in value:
        codepoint = ord(char)
        if char == "\\":
            escaped.append("\\\\")
        elif char == '"':
            escaped.append('\\"')
        elif char == "\n":
            escaped.append("\\n")
        elif char == "\r":
            escaped.append("\\r")
        elif char == "\t":
            escaped.append("\\t")
        elif 0x20 <= codepoint <= 0x7E:
            escaped.append(char)
        else:
            # ASCII-only generated source avoids invisible-character lints and
            # makes every vendored code point explicit during review.
            escaped.append(f"\\u{{{codepoint:x}}}")
    return f'"{"".join(escaped)}"'


def generate(raw: bytes) -> str:
    exact, legacy, max_name_len, max_legacy_name_len, source_record_count = parse_source(raw)
    root = build_trie(legacy)
    nodes = flatten(root)
    values = sorted({node.value for node in nodes if node.value is not None})
    if len(values) >= NO_VALUE:
        raise ValueError("too many unique entity replacements for u16 index")
    value_indexes = {value: index for index, value in enumerate(values)}

    edges: list[tuple[int, int]] = []
    node_rows: list[tuple[int, int, int]] = []
    for node in nodes:
        first_edge = len(edges)
        for byte, child in sorted(node.children.items()):
            edges.append((byte, child.index))
        value_index = NO_VALUE if node.value is None else value_indexes[node.value]
        node_rows.append((first_edge, len(node.children), value_index))

    if len(nodes) > 0xFFFF or len(edges) > 0xFFFF:
        raise ValueError("legacy entity trie is too large for u16 indexes")

    root_transitions = [NO_NODE] * 128
    for byte, child in sorted(root.children.items()):
        if byte >= len(root_transitions):
            raise ValueError("legacy entity root contains a non-ASCII edge")
        root_transitions[byte] = child.index

    digest = hashlib.sha256(raw).hexdigest()
    lines = [
        "// @generated by scripts/generate_entities.py; do not edit by hand.",
        f"// Source: {SOURCE_URL}",
        f"// SHA-256: {digest}",
        "",
        "#[derive(Clone, Copy)]",
        "pub(crate) struct LegacyEntityTrieNode {",
        "    pub(crate) first_edge: u16,",
        "    pub(crate) edge_count: u16,",
        "    pub(crate) value_index: u16,",
        "}",
        "",
        "#[derive(Clone, Copy)]",
        "pub(crate) struct LegacyEntityTrieEdge {",
        "    pub(crate) byte: u8,",
        "    pub(crate) next: u16,",
        "}",
        "",
        f'pub(crate) const ENTITY_SOURCE_URL: &str = "{SOURCE_URL}";',
        f'pub(crate) const ENTITY_SOURCE_SHA256: &str = "{digest}";',
        f"pub(crate) const ENTITY_SOURCE_RECORD_COUNT: usize = {source_record_count};",
        "#[cfg(test)]",
        f"pub(crate) const EXACT_ENTITY_COUNT: usize = {len(exact)};",
        "#[cfg(test)]",
        f"pub(crate) const LEGACY_ENTITY_COUNT: usize = {len(legacy)};",
        f"pub(crate) const MAX_ENTITY_NAME_LEN: usize = {max_name_len};",
        f"pub(crate) const MAX_LEGACY_ENTITY_NAME_LEN: usize = {max_legacy_name_len};",
        f"pub(crate) const NO_ENTITY_VALUE: u16 = {NO_VALUE};",
        f"pub(crate) const NO_ENTITY_NODE: u16 = {NO_NODE};",
        "",
        "pub(crate) static EXACT_ENTITIES: phf::Map<&'static str, &'static str> = phf::phf_map! {",
    ]
    lines.extend(
        f"    {rust_string(name)} => {rust_string(replacement)},"
        for name, replacement in sorted(exact.items())
    )
    lines.extend(
        [
            "};",
            "",
            f"pub(crate) static LEGACY_ENTITY_VALUES: [&str; {len(values)}] = [",
        ]
    )
    lines.extend(f"    {rust_string(value)}," for value in values)
    lines.extend(
        [
            "];",
            "",
            "pub(crate) static LEGACY_ENTITY_TRIE_ROOT: [u16; 128] = [",
        ]
    )
    for start in range(0, len(root_transitions), 16):
        chunk = ", ".join(str(value) for value in root_transitions[start : start + 16])
        lines.append(f"    {chunk},")
    lines.extend(
        [
            "];",
            "",
            "pub(crate) static LEGACY_ENTITY_TRIE_NODES: "
            f"[LegacyEntityTrieNode; {len(node_rows)}] = [",
        ]
    )
    for first_edge, edge_count, value_index in node_rows:
        lines.append(
            "    LegacyEntityTrieNode { "
            f"first_edge: {first_edge}, edge_count: {edge_count}, "
            f"value_index: {value_index} "
            "},"
        )
    lines.extend(
        [
            "];",
            "",
            "pub(crate) static LEGACY_ENTITY_TRIE_EDGES: "
            f"[LegacyEntityTrieEdge; {len(edges)}] = [",
        ]
    )
    for byte, next_index in edges:
        lines.append(f"    LegacyEntityTrieEdge {{ byte: {byte}, next: {next_index} }},")
    lines.append("];")
    lines.extend(
        [
            "",
            "#[cfg(test)]",
            f"pub(crate) static LEGACY_ENTITY_NAMES: [&str; {len(legacy)}] = [",
        ]
    )
    lines.extend(f"    {rust_string(name)}," for name in sorted(legacy))
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=VENDORED)
    parser.add_argument("--update-vendor", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    raw = args.source.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != EXPECTED_SOURCE_SHA256:
        print(
            "WHATWG entity source SHA-256 mismatch: "
            f"expected {EXPECTED_SOURCE_SHA256}, got {digest}",
            file=sys.stderr,
        )
        return 1
    if args.update_vendor:
        VENDORED.parent.mkdir(parents=True, exist_ok=True)
        VENDORED.write_bytes(raw)
        raw = VENDORED.read_bytes()

    rendered = generate(raw)
    if args.check:
        if not VENDORED.exists() or VENDORED.read_bytes() != raw:
            print("vendored WHATWG entity source differs", file=sys.stderr)
            return 1
        if not GENERATED.exists() or GENERATED.read_text() != rendered:
            print("generated entity tables are stale", file=sys.stderr)
            return 1
        return 0

    GENERATED.write_text(rendered)
    print(f"generated {GENERATED.relative_to(ROOT)} from {len(raw)} source bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
