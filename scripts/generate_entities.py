#!/usr/bin/env python3
"""Vendor WHATWG named entities and generate the Rust lookup tables.

The generated Rust source is checked in so normal Cargo builds never access
the network. Refreshing the vendored JSON is an explicit maintainer action.

Every name lives in one DAFSA: the trie of all names with equal subtrees
merged, which the matcher walks once per reference to find both the longest
name allowed without a semicolon and the complete name. A merged state cannot
hold a value, so each edge records how many names sort before the ones it
leads to; the ranks summed along a name's path index its value.
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
NO_EDGE = 0xFFFF
EXACT_FLAG = 1
LEGACY_FLAG = 2
DENSE_FLAG = 4
# States with this many edges or more find them in a row indexed by the byte
# instead of scanning a short sorted list; names use bytes from `0` to `z`.
DENSE_MIN_EDGES = 5
DENSE_FIRST_BYTE = ord("0")
DENSE_ROW_LEN = ord("z") - DENSE_FIRST_BYTE + 1


@dataclass
class TrieNode:
    children: dict[int, "TrieNode"] = field(default_factory=dict)
    exact: bool = False
    legacy: bool = False


@dataclass
class State:
    """A DAFSA state: terminal flags, sorted out-edges and its name count."""

    exact: bool
    legacy: bool
    edges: list[tuple[int, int]]
    names: int


def parse_source(raw: bytes) -> tuple[dict[str, str], dict[str, str], int, int]:
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise ValueError("WHATWG entity source must be a JSON object")

    exact: dict[str, str] = {}
    legacy: dict[str, str] = {}
    max_name_len = 0
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

    # WHATWG provides a semicolon-terminated counterpart for every legacy
    # spelling. Keep that invariant explicit so the two lookup paths cannot
    # silently disagree.
    for name, replacement in legacy.items():
        if exact.get(name) != replacement:
            raise ValueError(f"legacy entity has no matching exact record: {name!r}")

    return exact, legacy, max_name_len, len(data)


def build_trie(exact: dict[str, str], legacy: dict[str, str]) -> TrieNode:
    root = TrieNode()
    for name in sorted(exact):
        if not name.isalnum():
            # The matcher stops at the first byte with no edge, and the
            # tokenizer relies on names being ASCII alphanumeric.
            raise ValueError(f"entity name is not alphanumeric: {name!r}")
        node = root
        for byte in name.encode("ascii"):
            node = node.children.setdefault(byte, TrieNode())
        node.exact = True
        node.legacy = name in legacy
    return root


def minimize(root: TrieNode) -> list[State]:
    """Merge equal subtrees bottom-up and number the states from the root.

    Two nodes are equal when their flags and their edges (byte and target)
    are. The root becomes state 0 and the others follow in breadth-first
    order, so the numbering does not depend on dictionary order.
    """
    states: list[State] = []
    registry: dict[tuple, int] = {}

    def visit(node: TrieNode) -> int:
        edges = tuple((byte, visit(child)) for byte, child in sorted(node.children.items()))
        key = (node.exact, node.legacy, edges)
        index = registry.get(key)
        if index is None:
            names = int(node.exact) + sum(states[target].names for _, target in edges)
            index = len(states)
            registry[key] = index
            states.append(State(node.exact, node.legacy, list(edges), names))
        return index

    order = [visit(root)]
    numbering = {order[0]: 0}
    for index in order:
        for _, target in states[index].edges:
            if target not in numbering:
                numbering[target] = len(order)
                order.append(target)
    return [
        State(
            states[index].exact,
            states[index].legacy,
            [(byte, numbering[target]) for byte, target in states[index].edges],
            states[index].names,
        )
        for index in order
    ]


@dataclass
class Tables:
    """The arrays the generated Rust source holds, as `EntityWalk` reads them."""

    states: list[int]
    dense_edges: list[int]
    edge_bytes: list[int]
    edge_targets: list[int]
    edge_skips: list[int]

    def walk(self, name: str) -> tuple[int, int] | None:
        """The state and rank reached for `name`, as the Rust matcher walks."""
        state, rank = 0, 0
        for byte in name.encode("ascii"):
            packed = self.states[state]
            if packed & DENSE_FLAG:
                column = byte - DENSE_FIRST_BYTE
                if not 0 <= column < DENSE_ROW_LEN:
                    return None
                edge = self.dense_edges[(packed >> 16) * DENSE_ROW_LEN + column]
                if edge == NO_EDGE:
                    return None
            else:
                first, count = packed >> 16, (packed >> 8) & 0xFF
                bytes_ = self.edge_bytes[first : first + count]
                if byte not in bytes_:
                    return None
                edge = first + bytes_.index(byte)
            state, rank = self.edge_targets[edge], rank + self.edge_skips[edge]
        return state, rank


def build_tables(states: list[State]) -> Tables:
    """Lay the states out as the packed arrays the Rust matcher reads."""
    tables = Tables([], [], [], [], [])
    for state in states:
        first_edge = len(tables.edge_bytes)
        # Names through an edge sort after the state's own name and after
        # every name through an earlier (smaller-byte) edge.
        skip = int(state.exact)
        for byte, target in state.edges:
            tables.edge_bytes.append(byte)
            tables.edge_targets.append(target)
            tables.edge_skips.append(skip)
            skip += states[target].names

        flags = (EXACT_FLAG if state.exact else 0) | (LEGACY_FLAG if state.legacy else 0)
        if len(state.edges) >= DENSE_MIN_EDGES:
            row = [NO_EDGE] * DENSE_ROW_LEN
            for offset, (byte, _) in enumerate(state.edges):
                if not 0 <= byte - DENSE_FIRST_BYTE < DENSE_ROW_LEN:
                    raise ValueError(f"entity name byte {byte} is outside 0..z")
                row[byte - DENSE_FIRST_BYTE] = first_edge + offset
            location = len(tables.dense_edges) // DENSE_ROW_LEN
            tables.dense_edges.extend(row)
            flags |= DENSE_FLAG
        else:
            location = first_edge
        if location > 0xFFFF or len(state.edges) > 0xFF:
            raise ValueError("entity DAFSA state does not fit its packed form")
        tables.states.append(location << 16 | len(state.edges) << 8 | flags)

    if len(states) > 0xFFFF or len(tables.edge_bytes) >= NO_EDGE:
        raise ValueError("entity DAFSA is too large for u16 indexes")
    return tables


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


def number_rows(values: list[int], per_line: int = 16) -> list[str]:
    return [
        "    " + ", ".join(str(value) for value in values[start : start + per_line]) + ","
        for start in range(0, len(values), per_line)
    ]


def generate(raw: bytes) -> str:
    exact, legacy, max_name_len, source_record_count = parse_source(raw)
    states = minimize(build_trie(exact, legacy))
    names = sorted(exact)
    if len(names) > 0xFFFF:
        raise ValueError("too many entity names for u16 ranks")
    tables = build_tables(states)

    # Check the tables before emitting them: every name reaches an exact
    # state whose rank is its position in sorted order.
    for rank, name in enumerate(names):
        reached = tables.walk(name)
        if reached is None or not states[reached[0]].exact or reached[1] != rank:
            raise ValueError(f"entity DAFSA does not rank {name!r} correctly")
        if states[reached[0]].legacy != (name in legacy):
            raise ValueError(f"entity DAFSA has the wrong legacy flag for {name!r}")

    values = sorted(set(exact.values()))
    if len(values) > 0xFFFF:
        raise ValueError("too many unique entity replacements for u16 index")
    value_indexes = {value: index for index, value in enumerate(values)}

    digest = hashlib.sha256(raw).hexdigest()
    lines = [
        "// @generated by scripts/generate_entities.py; do not edit by hand.",
        f"// Source: {SOURCE_URL}",
        f"// SHA-256: {digest}",
        "",
        f'pub(crate) const ENTITY_SOURCE_URL: &str = "{SOURCE_URL}";',
        f'pub(crate) const ENTITY_SOURCE_SHA256: &str = "{digest}";',
        f"pub(crate) const ENTITY_SOURCE_RECORD_COUNT: usize = {source_record_count};",
        "#[cfg(test)]",
        f"pub(crate) const EXACT_ENTITY_COUNT: usize = {len(exact)};",
        "#[cfg(test)]",
        f"pub(crate) const LEGACY_ENTITY_COUNT: usize = {len(legacy)};",
        f"pub(crate) const MAX_ENTITY_NAME_LEN: usize = {max_name_len};",
        f"pub(crate) const ENTITY_EXACT: u8 = {EXACT_FLAG};",
        f"pub(crate) const ENTITY_LEGACY: u8 = {LEGACY_FLAG};",
        f"pub(crate) const ENTITY_DENSE: u8 = {DENSE_FLAG};",
        f"pub(crate) const ENTITY_DENSE_FIRST_BYTE: u8 = {DENSE_FIRST_BYTE};",
        f"pub(crate) const ENTITY_DENSE_ROW_LEN: usize = {DENSE_ROW_LEN};",
        f"pub(crate) const NO_ENTITY_EDGE: u16 = {NO_EDGE};",
        "",
        "// Per state: location << 16 | edge count << 8 | ENTITY_EXACT,",
        "// ENTITY_LEGACY and ENTITY_DENSE flags. The location is the state's",
        "// first edge, or with ENTITY_DENSE its row of ENTITY_DENSE_EDGES.",
        "// State 0 is the start state.",
        f"pub(crate) static ENTITY_STATES: [u32; {len(tables.states)}] = [",
        *number_rows(tables.states, 8),
        "];",
        "",
        "// Rows of edge indexes, or NO_ENTITY_EDGE, by byte from",
        "// ENTITY_DENSE_FIRST_BYTE, for the states with the most edges.",
        f"pub(crate) static ENTITY_DENSE_EDGES: [u16; {len(tables.dense_edges)}] = [",
        *number_rows(tables.dense_edges, DENSE_ROW_LEN // 5),
        "];",
        "",
        "// Each state's edges, sorted by byte: the byte, the target state, and",
        "// the number of names that sort before the ones the edge leads to.",
        f"pub(crate) static ENTITY_EDGE_BYTES: [u8; {len(tables.edge_bytes)}] = [",
        *number_rows(tables.edge_bytes),
        "];",
        "",
        f"pub(crate) static ENTITY_EDGE_TARGETS: [u16; {len(tables.edge_targets)}] = [",
        *number_rows(tables.edge_targets),
        "];",
        "",
        f"pub(crate) static ENTITY_EDGE_SKIPS: [u16; {len(tables.edge_skips)}] = [",
        *number_rows(tables.edge_skips),
        "];",
        "",
        "// Index into ENTITY_VALUES of each name, in sorted name order.",
        f"pub(crate) static ENTITY_VALUE_INDEXES: [u16; {len(names)}] = [",
        *number_rows([value_indexes[exact[name]] for name in names]),
        "];",
        "",
        f"pub(crate) static ENTITY_VALUES: [&str; {len(values)}] = [",
        *(f"    {rust_string(value)}," for value in values),
        "];",
        "",
        "#[cfg(test)]",
        f"pub(crate) static EXACT_ENTITIES: [(&str, &str); {len(names)}] = [",
        *(f"    ({rust_string(name)}, {rust_string(exact[name])})," for name in names),
        "];",
        "",
        "#[cfg(test)]",
        f"pub(crate) static LEGACY_ENTITY_NAMES: [&str; {len(legacy)}] = [",
        *(f"    {rust_string(name)}," for name in sorted(legacy)),
        "];",
        "",
    ]
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
