# WHATWG named character references

`entities.json` is a pinned copy of the WHATWG HTML named character reference
data from <https://html.spec.whatwg.org/entities.json>.

- Retrieved: 2026-07-15
- SHA-256: `d741d877ac77c4194c4ad526b5b4a19aef8dfe411ab840a466891cdbb9f362e6`
- Records: 2,231

Refresh it explicitly and regenerate the checked-in Rust trie with:

```console
python3 scripts/generate_entities.py --source /path/to/entities.json --update-vendor
python3 scripts/generate_entities.py --check
```

Normal Cargo builds use only the generated Rust source and never access the
network.
