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
the 0.2 line. `Tag` interns `xmp`, `noembed`, `noframes`, and `plaintext`
alongside the other known names. Unknown and custom element names are retained
as custom tags; CSS and XPath tag tests use the same custom-name behavior.

## Curated HTML repair

The 0.2 tree builder provides the repairs needed by the maintained scraping
corpus:

- a source `/>` marker does not close a non-void HTML element;
- implicit closing for `p`, `li`, `dt`/`dd`, headings, `option`, and
  `optgroup`, limited to the HTML element scopes so nested lists and nested
  tables keep their structure;
- table section, row, cell, caption, and column-group repair, including
  automatic `tbody`/`tr`/`colgroup`, foster parenting of misplaced content
  decided by the current node, and in-table `script`, `style`, `form`, and
  hidden `input` elements;
- invalid child filtering while parsing `select`, including table tags that
  close a `select` inside a table;
- `</br>` as `<br>`, a stray `</p>` as an empty paragraph, and the leading
  newline dropped after `pre`, `textarea`, and `listing` start tags;
- end-tag repair for standard formatting elements such as `b`, `i`, `strong`,
  and `em`; formatting elements that another start tag closes implicitly are
  not reconstructed later, so `<p><b>x<p>y` yields `<p><b>x</b></p><p>y</p>`;
- raw-text `xmp`, `noembed`, and `noframes` content, script-data escape
  states, and literal `plaintext` content through end of input;
- `<?…>`, `<!…>` declarations other than comments, CDATA, and doctypes, and
  `</` followed by a non-letter become comments; `</>` is ignored; a doctype
  ends at its first `>`, even one inside quotes;
- duplicate attributes resolved ASCII-case-insensitively with first-wins
  semantics; and
- the complete vendored WHATWG named-character-reference set, including
  multi-codepoint values and the context-dependent legacy semicolon rules.

These behaviors are regression-tested against canonical DOM output. They are a
bounded compatibility contract, not a claim that every HTML parsing algorithm
from the WHATWG specification is implemented.

## Tokenizer conformance

The tokenizer runs the vendored
[html5lib-tests](vendor/html5lib-tests/README.md) tokenizer suite on every
`cargo test`. At the pinned commit, 6,905 runs pass and 82 are skipped: the
CDATA section state, initial states that no start tag enters exactly, and four
inputs with lone surrogates. The remaining 62 fail for these documented
reasons:

- U+0000 is kept where the specification emits U+FFFD (in tag and attribute
  names and values, comments, doctypes, and raw-text, RCDATA, and plaintext
  content);
- `<?` starts a bogus comment rather than a processing instruction; and
- `<![CDATA[` starts a CDATA section in all content, not only in foreign
  content.

The tokenizer does not normalize CR and CRLF newlines to LF; the specification
does this while preprocessing the input stream, and the suite assumes it has
happened. Doctype content is exposed as a single string, so only the doctype
name is compared.

Because newlines are not normalized, the tree builder treats a leading CR or
CRLF after a `pre`, `textarea`, or `listing` start tag as the newline it drops,
and keeps a CR written as a character reference (`&#13;`), as the
specification does. It decodes text itself to tell them apart. A program that
feeds `TreeBuilder::process` already-decoded text tokens loses that
distinction, and a leading CR is then taken to be a source newline; pairing
`StreamTokenizer::with_undecoded_text` with `TreeBuilder::decode_text_tokens`
keeps it, as the streaming parsers do.

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
UTF-8. Malformed byte sequences are replaced with U+FFFD; decoding never
produces an error. An HTTP `Content-Type` header is not accepted as an encoding
hint in 0.2; the HTTP client layer must resolve or apply that signal before
parsing.

Configured limits apply independently to raw bytes and decoded UTF-8 bytes.
The effective maximum is `min(configured_limit, 2 GiB)`: arena offsets are
stored as `u32`, and named-character-reference decoding can expand text by up
to 6/5. At most 512 elements are open at once, excluding the synthetic root.
A start tag beyond that inserts an empty element at depth 513, and the content
after it goes to the innermost open element, so deeply nested input still
parses into a tree no deeper than 513.

Size errors are terminal for a streaming parser. Iterator, Tokio, and
async-std adapters stop pulling their source after the first error.

The streaming tokenizer bounds unfinished raw-text/RCDATA closing-tag candidates
by discarding oversized prefixes while retaining the original tag name, scanner
state, and a bounded text tail. A completed closing tag still closes the element.
As in browsers, a closing tag that EOF interrupts is dropped, and the element
ends with the text before it; this is identical for one-shot, streaming, and
early-stop parsing.

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
use a sparse map. Sibling (`~`) chains and selections over many roots are
evaluated in time linear in the searched subtrees. Selecting within a
`Selection` searches each selected root and its descendants; combinators may
match ancestors outside the selected roots.

XPath requires the `xpath` feature. XPath name tests retain non-interned names,
so queries such as `//option` and `//my-widget` follow the same custom-tag
behavior as CSS.

## Checking a dependency upgrade

0.2 intentionally contains breaking streaming, early-stop, decoding, and
feature-gating changes. Read
[MIGRATION-0.2.md](MIGRATION-0.2.md), run your scraper fixtures through every
entry point you use, and compare canonical output rather than node counts
alone. The repository's full local development gate is:

```bash
python3 scripts/release.py check
```
