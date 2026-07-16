# Migrating to fast-html-parser 0.2

Version 0.2 is intentionally breaking. It makes streaming failures explicit,
returns owned early-stop results, removes the ineffective fragment toggle, and
tightens parser and selector limits.

## Streaming feed is fallible

`StreamParser::feed` returned `()` in 0.1. It now returns
`Result<(), HtmlError>` and must be handled:

```rust
use fast_html_parser::streaming::StreamParser;

let mut parser = StreamParser::new();
parser.feed(b"<main>")?;
parser.feed(b"content</main>")?;
let document = parser.finish()?;
# Ok::<(), fast_html_parser::HtmlError>(())
```

Configure streaming limits directly, or use the limited iterator helper:

```rust
use fast_html_parser::streaming::{StreamParser, parse_stream_with_limit};

let mut parser = StreamParser::with_max_input_size(8 * 1024 * 1024);
parser.feed(b"<p>bounded</p>")?;
let document = parser.finish()?;

let bytes = b"<p>also bounded</p>";
let document = parse_stream_with_limit(bytes.chunks(4), 8 * 1024 * 1024)?;
# Ok::<(), fast_html_parser::HtmlError>(())
```

The configured limit applies to both raw and decoded bytes and is capped at
`u32::MAX`. An encoding, nesting, or size failure makes that parser terminal.
The first operation returns the specific error; later `feed`/`finish` calls
return `HtmlError::ParserTerminated`. Iterator, Tokio, and async-std wrappers
stop reading their source at the first failure.

The first chunk no longer needs to be small: at most 1 KiB is copied for the
encoding prescan, and the remainder is processed during the same call in
64 KiB internal blocks.

## Early stop has two explicit modes

`EarlyStopParser::stop_when` and `ParseStatus` are removed. Choose the point at
which parsing may stop:

- `stop_on_create` matches as soon as the start tag and attributes exist;
- `stop_after_element` waits until the matching element is explicitly or
  implicitly closed and returns its complete subtree.

```rust
use fast_html_parser::Tag;
use fast_html_parser::streaming::{
    EarlyStopOutcome, EarlyStopParser, EarlyStopProgress, MatchCompleteness,
};

let mut parser = EarlyStopParser::stop_after_element(|node| node.tag() == Tag::Article)
    .max_input_size(8 * 1024 * 1024);

if parser.feed(b"<article><p>result</p></article><footer>later</footer>")?
    == EarlyStopProgress::Matched
{
    let EarlyStopOutcome::Matched(found) = parser.finish()? else {
        unreachable!()
    };
    assert_eq!(found.completeness(), MatchCompleteness::SubtreeComplete);
    assert_eq!(found.node().text_content(), "result");
    let node_id = found.node_id();
    let document = found.into_document();
    assert_eq!(document.get(node_id).tag(), Tag::Article);
}
# Ok::<(), fast_html_parser::HtmlError>(())
```

The new `EarlyStopMatch` owns the partial `Document`; its `NodeId` is valid in
that document. Do not retain a node id from the old builder and apply it to a
different document.

## `fragment_mode` is removed

The 0.1 builder accepted `fragment_mode(bool)` but did not provide
context-aware fragment parsing. Remove that call:

```rust
use fast_html_parser::HtmlParser;

let parser = HtmlParser::builder()
    .max_input_size(64 * 1024 * 1024)
    .build();
let document = parser.parse_str("<p>top-level content</p>")?;
# Ok::<(), fast_html_parser::HtmlError>(())
```

`Document::root()` remains a synthetic document root. The parser does not
synthesize `html`, `head`, or `body`. Context-aware fragment parsing remains
outside the 0.2 scope.

## Errors are extensible and tree building is fallible

`HtmlError` now includes `HtmlError::Parse(ParseError)`. `HtmlError`,
`ParseError`, `EncodingError`, and selector error enums are non-exhaustive;
downstream matches must include a wildcard arm:

```rust
use fast_html_parser::{HtmlError, HtmlParser};

match HtmlParser::parse("<p>input</p>") {
    Ok(document) => println!("{}", document.root().text_content()),
    Err(HtmlError::Parse(error)) => eprintln!("malformed input: {error}"),
    Err(other) => eprintln!("parse failed: {other}"),
}
```

Low-level `TreeBuilder` users must propagate both operations:

```rust,ignore
let created = builder.process(token)?;
let (arena, root) = builder.finish()?;
```

Nesting is limited to 512 elements, excluding the synthetic root. A
513th nested element returns `ParseError::NestingTooDeep`; a partial `Document`
is not returned.

For `TreeSink` implementors, the `self_closing` argument now means only that a
slash appeared in the source start tag. Determine HTML voidness with
`Tag::is_void()`. A slash does not close a non-void element.

## SIMD feature selection

`simd` is a real default feature. With defaults enabled, the scanner performs
runtime dispatch to the supported SIMD backend. To force scalar execution:

```toml
[dependencies]
fast-html-parser = { version = "0.2.0", default-features = false }
```

Add only the other features you need. Enabling `simd` explicitly with default
features disabled restores runtime-dispatched acceleration.

## DOM and selector behavior changes

- duplicate attributes are ASCII-case-insensitive and first-wins before later
  values are decoded;
- named entities use the complete vendored WHATWG table and distinguish text
  from attribute legacy-semicolon rules;
- non-void `/>`, optional end tags, table repair, formatting repair, `select`,
  and `plaintext` follow the curated 0.2 compatibility contract;
- selectors now have explicit complexity limits and return
  `SelectorError::Invalid` when a limit is exceeded; and
- XPath preserves literal custom tag tests, so `//my-widget` behaves like the
  corresponding CSS type selector.

See [COMPATIBILITY.md](COMPATIBILITY.md) for the supported surface and explicit
non-goals.
