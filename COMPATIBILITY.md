# Compatibility contract

`fast-html-parser` 0.2 is a high-speed, pragmatic parser for web-scraping
workloads. It is not a browser engine and does not promise a byte-for-byte or
node-for-node match with a fully conforming HTML5 DOM.

## Document model

Every successful parse returns a `Document` whose `root()` is a synthetic
document node. Top-level nodes from the input are children of that root. The
parser does not synthesize `html`, `head`, or `body` elements and does not expose
a context-aware fragment mode.

The public `Tag` representation and 64-byte arena node layout remain stable for
the 0.2 line. Unknown and custom element names are retained as custom tags;
CSS and XPath tag tests use the same custom-name behavior.

## Curated HTML repair

The 0.2 tree builder provides the repairs needed by the maintained scraping
corpus:

- a source `/>` marker does not close a non-void HTML element;
- implicit closing for `p`, `li`, `dt`/`dd`, headings, `option`, and
  `optgroup`;
- table section, row, and cell repair, including automatic `tbody`/`tr` and
  foster parenting of misplaced content;
- invalid child filtering while parsing `select`;
- active-formatting repair for standard formatting elements such as `b`, `i`,
  `strong`, and `em`;
- literal `plaintext` content through end of input;
- duplicate attributes resolved ASCII-case-insensitively with first-wins
  semantics; and
- the complete vendored WHATWG named-character-reference set, including
  multi-codepoint values and the context-dependent legacy semicolon rules.

These behaviors are regression-tested against canonical DOM output. They are a
bounded compatibility contract, not a claim that every HTML parsing algorithm
from the WHATWG specification is implemented.

## Intentionally unsupported in 0.2

- browser `html`/`head`/`body` wrapper synthesis;
- SVG, MathML, foreign-content integration points, and namespace processing;
- `template` insertion modes and template contents;
- scripting-dependent tokenization or tree-construction algorithms;
- context-aware fragment parsing; and
- browser layout, script execution, CSS cascade, or DOM mutation semantics.

Do not use this crate as an HTML sanitizer. Applications handling untrusted
markup must apply a security policy designed for the output context.

## Encodings and input limits

String entry points consume UTF-8 Rust strings. Byte and streaming entry points
detect a BOM, prescan HTML `meta` declarations, and otherwise fall back to
UTF-8. An HTTP `Content-Type` header is not accepted as an encoding hint in
0.2; the HTTP client layer must resolve or apply that signal before parsing.

Configured limits apply independently to raw bytes and decoded UTF-8 bytes.
The effective maximum is `min(configured_limit, u32::MAX)`, because arena
offsets are stored as `u32`. At most 512 nested elements are permitted,
excluding the synthetic root. Attempting to create an element at depth 513 returns
`HtmlError::Parse(ParseError::NestingTooDeep)` and no partial `Document`.

Encoding, depth, and size errors are terminal for a streaming parser. Iterator,
Tokio, and async-std adapters stop pulling their source after the first error.

## Selectors and XPath

CSS selector evaluation memoizes `(NodeId, chain_index)` states instead of
using recursive backtracking. Selector parsing rejects inputs beyond these
limits with `SelectorError::Invalid`:

- 16 KiB selector text;
- 64 comma-separated selector branches;
- 64 combinators per branch;
- 256 simple parts in total; and
- 16 nested `:not(...)` levels.

The dense memo table is used through 8,388,608 states; larger valid searches
use a sparse map. XPath name tests retain non-interned names, so queries such as
`//option` and `//my-widget` follow the same custom-tag behavior as CSS.

## Checking a dependency upgrade

0.2 intentionally contains breaking streaming and early-stop API changes. Read
[MIGRATION-0.2.md](MIGRATION-0.2.md), run your scraper fixtures through every
entry point you use, and compare canonical output rather than node counts
alone. The repository's full local development gate is:

```bash
python3 scripts/release.py check
```
