# html5lib-tests tokenizer suite

`tokenizer/` is a pinned copy of the WHATWG tokenizer conformance tests from
<https://github.com/html5lib/html5lib-tests>, distributed under the MIT license
in `LICENSE`. The format is described in `tokenizer/README.md`.

- Commit: `c777c408b61078ea2eb4acefc2535f54dbc8b28a` (2026-10-01)
- Retrieved: 2026-10-13
- Files: `LICENSE`, `tokenizer/README.md`, and every `tokenizer/*.test`

`SHA256SUMS` pins every vendored byte; `.gitattributes` disables line-ending
conversion so it verifies on every platform:

```console
cd vendor/html5lib-tests && sha256sum -c SHA256SUMS
```

`crates/fhp-tokenizer/tests/html5lib.rs` runs the suite. Tests that fail
because of documented divergences are listed in
`crates/fhp-tokenizer/tests/html5lib/expected-failures.txt`.

To refresh, replace `LICENSE` and `tokenizer/` with the files from a newer
commit, update the commit above, regenerate `SHA256SUMS`, and re-bless the
expectations:

```console
FHP_HTML5LIB_BLESS=1 cargo test -p fhp-tokenizer --test html5lib
```

Review the resulting diff of `expected-failures.txt`: every new entry is a
behavior change that needs a reason.
