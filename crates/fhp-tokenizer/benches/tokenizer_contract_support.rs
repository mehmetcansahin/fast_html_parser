//! Shared correctness contract for the tokenizer comparison benchmark.
//!
//! Keep this module independent from Criterion so the same fixture checks can
//! run as an integration test. Benchmark binaries call these checks before
//! starting a timing loop; correctness work is therefore never timed.
//!
//! Every adapter returns the number of native tokens or events its library
//! delivered, and the timed closure is exactly the counting function whose
//! result the contract checks. Full-token adapters take a token callback, so
//! the canonical token-stream digest is computed by the same code path with a
//! different callback. Two implementations are contract-equal only when their
//! digests match: the same decoded tag names, attributes, text and comments.
//!
//! The libraries split work differently, so only full-token workloads are
//! comparable. `lol_html` without handlers is a lazy pass-through that never
//! tokenizes content, `lol_html` with handlers parses attributes lazily and
//! never decodes text, and `tl` builds a DOM with a parser that is not
//! WHATWG-compliant. Those rows are absolute references, never ratios.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::fmt;
use std::hint::black_box;
use std::sync::OnceLock;

use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token as FhpToken;
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};

/// A benchmark input: Criterion path component, contract ID and contents.
#[derive(Clone, Copy, Debug)]
pub struct BenchFixture {
    pub benchmark_id: &'static str,
    pub contract_id: &'static str,
    pub html: &'static str,
}

/// Every fixture the comparison measures, in registration order.
pub const FIXTURES: &[BenchFixture] = &[
    BenchFixture {
        benchmark_id: "realworld/hackernews_34kb",
        contract_id: "realworld_hackernews_34kb",
        html: include_str!("../../../testdata/hackernews.html"),
    },
    BenchFixture {
        benchmark_id: "realworld/github_301kb",
        contract_id: "realworld_github_301kb",
        html: include_str!("../../../testdata/github.html"),
    },
    BenchFixture {
        benchmark_id: "realworld/stackoverflow_415kb",
        contract_id: "realworld_stackoverflow_415kb",
        html: include_str!("../../../testdata/stackoverflow.html"),
    },
    BenchFixture {
        benchmark_id: "realworld/wikipedia_590kb",
        contract_id: "realworld_wikipedia_590kb",
        html: include_str!("../../../testdata/wikipedia.html"),
    },
    BenchFixture {
        benchmark_id: "synthetic/5mb",
        contract_id: "synthetic_5mb",
        html: include_str!("../../../testdata/large_5mb.html"),
    },
];

/// Chunk size for the streaming adapters: a typical network or file read.
pub const STREAM_CHUNK_BYTES: usize = 64 * 1024;

/// The library behind an implementation, used to order registration blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Library {
    Fhp,
    Html5gum,
    Html5ever,
    LolHtml,
    Tl,
}

/// What an implementation's timed loop actually does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkClass {
    /// WHATWG tokenization into decoded tag names, attributes, text and
    /// comments. The only class with a canonical token stream.
    FullToken,
    /// `lol_html` rewriting with byte-identical output. It does less work
    /// than full tokenization: content is parsed lazily, or not at all.
    LazyRewriter,
    /// `tl` DOM construction. The parser is not WHATWG-compliant.
    NonSpecDom,
}

impl WorkClass {
    /// Benchmark ID component that labels the work class.
    pub const fn namespace(self) -> &'static str {
        match self {
            Self::FullToken => "full_token",
            Self::LazyRewriter => "lazy_rewriter",
            Self::NonSpecDom => "nonspec_dom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Implementation {
    FhpTokenize,
    FhpTokenizeWith,
    FhpStreamFeed,
    FhpStreamFeedStrWith,
    Html5gumIterator,
    Html5gumCallback,
    Html5everTokenizer,
    LolHtmlNoopPassthrough,
    LolHtmlElementTextComment,
    TlParse,
}

impl Implementation {
    pub const ALL: [Self; 10] = [
        Self::FhpTokenize,
        Self::FhpTokenizeWith,
        Self::FhpStreamFeed,
        Self::FhpStreamFeedStrWith,
        Self::Html5gumIterator,
        Self::Html5gumCallback,
        Self::Html5everTokenizer,
        Self::LolHtmlNoopPassthrough,
        Self::LolHtmlElementTextComment,
        Self::TlParse,
    ];

    /// Contract key and `semantic_reference` benchmark name.
    pub const fn id(self) -> &'static str {
        match self {
            Self::FhpTokenize => "fhp_tokenize",
            Self::FhpTokenizeWith => "fhp_tokenize_with",
            Self::FhpStreamFeed => "fhp_stream_feed_64k",
            Self::FhpStreamFeedStrWith => "fhp_stream_feed_str_with_64k",
            Self::Html5gumIterator => "html5gum_iterator",
            Self::Html5gumCallback => "html5gum_callback",
            Self::Html5everTokenizer => "html5ever_tokenizer",
            Self::LolHtmlNoopPassthrough => "lol_html_noop_passthrough",
            Self::LolHtmlElementTextComment => "lol_html_element_text_comment_handlers",
            Self::TlParse => "tl_parse",
        }
    }

    /// Benchmark name inside a `contract_equal` group. `scripts/bench.py`
    /// forms competitor/FHP ratios against the member named `fhp`.
    pub const fn contract_equal_id(self) -> &'static str {
        match self.library() {
            Library::Fhp => "fhp",
            _ => self.id(),
        }
    }

    pub const fn library(self) -> Library {
        match self {
            Self::FhpTokenize
            | Self::FhpTokenizeWith
            | Self::FhpStreamFeed
            | Self::FhpStreamFeedStrWith => Library::Fhp,
            Self::Html5gumIterator | Self::Html5gumCallback => Library::Html5gum,
            Self::Html5everTokenizer => Library::Html5ever,
            Self::LolHtmlNoopPassthrough | Self::LolHtmlElementTextComment => Library::LolHtml,
            Self::TlParse => Library::Tl,
        }
    }

    pub const fn work_class(self) -> WorkClass {
        match self.library() {
            Library::Fhp | Library::Html5gum | Library::Html5ever => WorkClass::FullToken,
            Library::LolHtml => WorkClass::LazyRewriter,
            Library::Tl => WorkClass::NonSpecDom,
        }
    }

    /// The function the benchmark times. It returns the native token or
    /// event count, which the contract checks before timing starts.
    pub fn count_fn(self) -> fn(&str) -> usize {
        match self {
            Self::FhpTokenize => |html| {
                fhp_tokenize(html, |token| {
                    black_box(token);
                })
            },
            Self::FhpTokenizeWith => |html| {
                fhp_tokenize_with(html, |token| {
                    black_box(token);
                })
            },
            Self::FhpStreamFeed => |html| {
                fhp_stream_feed(html, |token| {
                    black_box(token);
                })
            },
            Self::FhpStreamFeedStrWith => |html| {
                fhp_stream_feed_str_with(html, |token| {
                    black_box(token);
                })
            },
            Self::Html5gumIterator => |html| {
                html5gum_iterator(html, |token| {
                    black_box(token);
                })
            },
            Self::Html5gumCallback => |html| {
                html5gum_callback(html, |event| {
                    black_box(event);
                })
            },
            Self::Html5everTokenizer => |html| {
                html5ever_tokenizer(html, |token| {
                    black_box(token);
                })
            },
            Self::LolHtmlNoopPassthrough => |html| {
                lol_html_noop_passthrough(html, |chunk| {
                    black_box(chunk);
                })
            },
            Self::LolHtmlElementTextComment => |html| {
                lol_html_element_text_comment_handlers(html, |chunk| {
                    black_box(chunk);
                })
            },
            Self::TlParse => tl_parse,
        }
    }

    /// Canonical token stream of a full-token implementation, produced by the
    /// same adapter the benchmark times.
    pub fn token_stream_digest(self, html: &str) -> Option<TokenStreamDigest> {
        let mut encoder = TokenStreamEncoder::new();
        match self {
            Self::FhpTokenize => {
                fhp_tokenize(html, |token| encoder.fhp_token(token));
            }
            Self::FhpTokenizeWith => {
                fhp_tokenize_with(html, |token| encoder.fhp_token(token));
            }
            Self::FhpStreamFeed => {
                fhp_stream_feed(html, |token| encoder.fhp_token(token));
            }
            Self::FhpStreamFeedStrWith => {
                fhp_stream_feed_str_with(html, |token| encoder.fhp_token(token));
            }
            Self::Html5gumIterator => {
                html5gum_iterator(html, |token| encoder.html5gum_token(token));
            }
            Self::Html5gumCallback => {
                html5gum_callback(html, |event| encoder.html5gum_event(event));
            }
            Self::Html5everTokenizer => {
                html5ever_tokenizer(html, |token| encoder.html5ever_token(token));
            }
            Self::LolHtmlNoopPassthrough | Self::LolHtmlElementTextComment | Self::TlParse => {
                return None;
            }
        }
        Some(encoder.finish())
    }

    /// Observe everything the contract records for this implementation.
    pub fn observe(self, html: &str) -> ImplementationContract {
        ImplementationContract {
            count: (self.count_fn())(html) as u64,
            token_stream: self.token_stream_digest(html),
        }
    }
}

// ---------------------------------------------------------------------------
// Raw-text rules
// ---------------------------------------------------------------------------

/// Tokenizer state entered after a start tag.
///
/// Standalone WHATWG tokenizers leave this switch to the tree builder, so the
/// html5gum and html5ever adapters apply fhp-tokenizer's rules themselves:
/// `script` is script data; `style`, `iframe`, `xmp`, `noembed` and
/// `noframes` are raw text; `title` and `textarea` are RCDATA; `plaintext`
/// never ends. `noscript` stays markup, as with scripting disabled. html5gum's
/// built-in `naively_switch_states` would treat `noscript` as raw text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawTextKind {
    ScriptData,
    RawText,
    RcData,
    PlainText,
}

pub fn raw_text_kind(name: &[u8]) -> Option<RawTextKind> {
    match name {
        b"script" => Some(RawTextKind::ScriptData),
        b"style" | b"iframe" | b"xmp" | b"noembed" | b"noframes" => Some(RawTextKind::RawText),
        b"title" | b"textarea" => Some(RawTextKind::RcData),
        b"plaintext" => Some(RawTextKind::PlainText),
        _ => None,
    }
}

fn html5gum_raw_text_state(name: &[u8]) -> Option<html5gum::State> {
    raw_text_kind(name).map(|kind| match kind {
        RawTextKind::ScriptData => html5gum::State::ScriptData,
        RawTextKind::RawText => html5gum::State::RawText,
        RawTextKind::RcData => html5gum::State::RcData,
        RawTextKind::PlainText => html5gum::State::PlainText,
    })
}

// ---------------------------------------------------------------------------
// Adapters
// ---------------------------------------------------------------------------

/// Split `input` into chunks of at most `size` bytes on character boundaries,
/// so both streaming adapters see identical chunks.
fn str_chunks(input: &str, size: usize) -> impl Iterator<Item = &str> {
    let mut rest = input;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut end = rest.len().min(size);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, tail) = rest.split_at(end);
        rest = tail;
        Some(chunk)
    })
}

/// `fhp_tokenizer::tokenize`: materialize every token in a `Vec`.
pub fn fhp_tokenize(html: &str, mut on_token: impl FnMut(&FhpToken<'_>)) -> usize {
    let tokens = fhp_tokenizer::tokenize(html);
    for token in &tokens {
        on_token(token);
    }
    tokens.len()
}

/// `fhp_tokenizer::tokenize_with`: one-shot callback tokenization.
pub fn fhp_tokenize_with(html: &str, mut on_token: impl FnMut(&FhpToken<'_>)) -> usize {
    let mut count = 0;
    fhp_tokenizer::tokenize_with(html, |token| {
        on_token(&token);
        count += 1;
    });
    count
}

/// `StreamTokenizer::feed`: owned tokens per 64 KiB chunk.
pub fn fhp_stream_feed(html: &str, mut on_token: impl FnMut(&FhpToken<'_>)) -> usize {
    let mut tokenizer = StreamTokenizer::new();
    let mut count = 0;
    for chunk in str_chunks(html, STREAM_CHUNK_BYTES) {
        let tokens = tokenizer.feed(chunk.as_bytes());
        tokens.iter().for_each(&mut on_token);
        count += tokens.len();
    }
    let tokens = tokenizer.finish();
    tokens.iter().for_each(&mut on_token);
    count + tokens.len()
}

/// `StreamTokenizer::feed_str_with`: borrowed tokens per 64 KiB chunk.
pub fn fhp_stream_feed_str_with(html: &str, mut on_token: impl FnMut(&FhpToken<'_>)) -> usize {
    let mut tokenizer = StreamTokenizer::new();
    let mut count = 0;
    for chunk in str_chunks(html, STREAM_CHUNK_BYTES) {
        tokenizer.feed_str_with(chunk, |token| {
            on_token(token);
            count += 1;
        });
    }
    tokenizer.finish_with(|token| {
        on_token(token);
        count += 1;
    });
    count
}

/// html5gum's default iterator, which yields owned tokens including parse
/// errors. Raw-text states are switched between tokens.
pub fn html5gum_iterator(html: &str, mut on_token: impl FnMut(&html5gum::Token)) -> usize {
    let mut tokenizer = html5gum::Tokenizer::new(html);
    let mut count = 0;
    while let Some(token) = tokenizer.next() {
        let Ok(token) = token;
        if let html5gum::Token::StartTag(tag) = &token {
            if let Some(state) = html5gum_raw_text_state(&tag.name) {
                tokenizer.set_state(state);
            }
        }
        on_token(&token);
        count += 1;
    }
    count
}

/// html5gum's `CallbackEmitter`, which reports borrowed events, start tags
/// split into name, attribute and close events. The callback yields a state
/// from the iterator only when a raw-text start tag closes.
pub fn html5gum_callback(html: &str, mut on_event: impl FnMut(&CallbackEvent<'_>)) -> usize {
    let mut count = 0;
    {
        let mut pending_state = None;
        let emitter = CallbackEmitter::new(
            |event: CallbackEvent<'_>, _span: html5gum::Span<()>| -> Option<html5gum::State> {
                count += 1;
                let next_state = match &event {
                    CallbackEvent::OpenStartTag { name } => {
                        pending_state = html5gum_raw_text_state(name);
                        None
                    }
                    CallbackEvent::CloseStartTag { .. } => pending_state.take(),
                    _ => None,
                };
                on_event(&event);
                next_state
            },
        );
        let mut tokenizer = html5gum::Tokenizer::new_with_emitter(html, emitter);
        while let Some(state) = tokenizer.next() {
            let Ok(state) = state;
            tokenizer.set_state(state);
        }
    }
    count
}

/// Counting html5ever sink that applies the raw-text rules.
struct Html5everSink<F> {
    on_token: RefCell<F>,
    count: Cell<usize>,
}

impl<F: FnMut(&html5ever::tokenizer::Token)> html5ever::tokenizer::TokenSink for Html5everSink<F> {
    type Handle = ();

    fn process_token(
        &self,
        token: html5ever::tokenizer::Token,
        _line_number: u64,
    ) -> html5ever::tokenizer::TokenSinkResult<()> {
        use html5ever::tokenizer::states::RawKind;
        use html5ever::tokenizer::{TagKind, Token, TokenSinkResult};

        self.count.set(self.count.get() + 1);
        let result = match &token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => {
                match raw_text_kind(tag.name.as_bytes()) {
                    Some(RawTextKind::ScriptData) => TokenSinkResult::RawData(RawKind::ScriptData),
                    Some(RawTextKind::RawText) => TokenSinkResult::RawData(RawKind::Rawtext),
                    Some(RawTextKind::RcData) => TokenSinkResult::RawData(RawKind::Rcdata),
                    Some(RawTextKind::PlainText) => TokenSinkResult::Plaintext,
                    None => TokenSinkResult::Continue,
                }
            }
            _ => TokenSinkResult::Continue,
        };
        (self.on_token.borrow_mut())(&token);
        result
    }
}

/// html5ever's standalone tokenizer: the input is copied into a `StrTendril`
/// and fed through a `BufferQueue` to a counting `TokenSink`. Tokens include
/// parse errors and EOF.
pub fn html5ever_tokenizer(
    html: &str,
    on_token: impl FnMut(&html5ever::tokenizer::Token),
) -> usize {
    use html5ever::tendril::StrTendril;
    use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerOpts};

    let sink = Html5everSink {
        on_token: RefCell::new(on_token),
        count: Cell::new(0),
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    let result = tokenizer.feed(&input);
    assert!(
        matches!(result, html5ever::TokenizerResult::Done),
        "html5ever tokenizer stopped before the end of its input"
    );
    tokenizer.end();
    tokenizer.sink.count.get()
}

/// `lol_html` with no handlers: a lazy pass-through that never tokenizes
/// content. Returns the number of output bytes.
pub fn lol_html_noop_passthrough(html: &str, mut on_output: impl FnMut(&[u8])) -> usize {
    let mut output_bytes = 0;
    let mut rewriter = lol_html::HtmlRewriter::new(lol_html::Settings::new(), |chunk: &[u8]| {
        output_bytes += chunk.len();
        on_output(chunk);
    });
    rewriter
        .write(html.as_bytes())
        .unwrap_or_else(|error| panic!("lol_html no-op write failed: {error}"));
    rewriter
        .end()
        .unwrap_or_else(|error| panic!("lol_html no-op end failed: {error}"));
    output_bytes
}

/// `lol_html` with `element!("*")`, `doc_text!` and `doc_comments!` handlers.
/// Returns the number of handler invocations. Attributes are parsed only on
/// access and text is never entity-decoded.
pub fn lol_html_element_text_comment_handlers(
    html: &str,
    mut on_output: impl FnMut(&[u8]),
) -> usize {
    let events = Cell::new(0);
    let settings = lol_html::Settings {
        element_content_handlers: vec![lol_html::element!("*", |element| {
            black_box(&element);
            events.set(events.get() + 1);
            Ok(())
        })],
        document_content_handlers: vec![
            lol_html::doc_text!(|text| {
                black_box(&text);
                events.set(events.get() + 1);
                Ok(())
            }),
            lol_html::doc_comments!(|comment| {
                black_box(&comment);
                events.set(events.get() + 1);
                Ok(())
            }),
        ],
        ..lol_html::Settings::new()
    };
    let mut rewriter = lol_html::HtmlRewriter::new(settings, |chunk: &[u8]| on_output(chunk));
    rewriter
        .write(html.as_bytes())
        .unwrap_or_else(|error| panic!("lol_html handler write failed: {error}"));
    rewriter
        .end()
        .unwrap_or_else(|error| panic!("lol_html handler end failed: {error}"));
    events.get()
}

/// `tl::parse`: a DOM-level reference. Returns the node count.
pub fn tl_parse(html: &str) -> usize {
    let dom = tl::parse(html, tl::ParserOptions::default())
        .unwrap_or_else(|error| panic!("tl parse failed: {error}"));
    dom.nodes().len()
}

// ---------------------------------------------------------------------------
// Canonical token stream
// ---------------------------------------------------------------------------

/// Deterministic digest of a canonical token stream.
///
/// The stream contains lowercase start tags with their attributes (first of
/// each duplicate name kept, then sorted by name, because html5gum does not
/// report source order), the self-closing flag, lowercase end tags, decoded
/// text with adjacent runs merged, comments, and a marker per doctype. Parse
/// errors, EOF tokens and doctype identifiers are outside the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenStreamDigest {
    pub event_count: u64,
    pub canonical_bytes: u64,
    pub fnv1a_128: u128,
}

impl fmt::Display for TokenStreamDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{ \"event_count\": {}, \"canonical_bytes\": {}, \"fnv1a_128\": \"{}\" }}",
            self.event_count, self.canonical_bytes, self.fnv1a_128
        )
    }
}

/// FNV-1a offset basis for the 128-bit contract digest.
const FNV1A_128_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
/// FNV-1a prime for the 128-bit contract digest.
const FNV1A_128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

/// A start tag assembled from html5gum callback events.
struct PendingStartTag {
    name: Vec<u8>,
    attrs: Vec<(Vec<u8>, Vec<u8>)>,
}

struct TokenStreamEncoder {
    hash: u128,
    canonical_bytes: u64,
    event_count: u64,
    text: Vec<u8>,
    start_tag: Option<PendingStartTag>,
}

impl TokenStreamEncoder {
    fn new() -> Self {
        Self {
            hash: FNV1A_128_OFFSET,
            canonical_bytes: 0,
            event_count: 0,
            text: Vec::new(),
            start_tag: None,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.hash = (self.hash ^ u128::from(*byte)).wrapping_mul(FNV1A_128_PRIME);
        }
        self.canonical_bytes += bytes.len() as u64;
    }

    /// Write an unambiguous typed field. Length-prefixing ensures that values
    /// containing delimiters cannot collide with adjacent fields.
    fn field(&mut self, kind: u8, value: &[u8]) {
        self.write(&[kind]);
        self.write(&(value.len() as u64).to_le_bytes());
        self.write(value);
    }

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.event_count += 1;
            self.field(b'T', &text);
        }
    }

    fn start_tag(&mut self, name: &[u8], attrs: Vec<(Vec<u8>, Vec<u8>)>, self_closing: bool) {
        self.flush_text();
        self.event_count += 1;
        self.field(b'S', &name.to_ascii_lowercase());

        let mut unique: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(attrs.len());
        for (mut attr_name, value) in attrs {
            attr_name.make_ascii_lowercase();
            if !unique.iter().any(|(existing, _)| *existing == attr_name) {
                unique.push((attr_name, value));
            }
        }
        unique.sort_unstable();
        self.field(b'#', &(unique.len() as u64).to_le_bytes());
        for (attr_name, value) in &unique {
            self.field(b'A', attr_name);
            self.field(b'V', value);
        }
        self.field(b'/', &[u8::from(self_closing)]);
    }

    fn end_tag(&mut self, name: &[u8]) {
        self.flush_text();
        self.event_count += 1;
        self.field(b'E', &name.to_ascii_lowercase());
    }

    fn text(&mut self, text: &[u8]) {
        self.text.extend_from_slice(text);
    }

    fn comment(&mut self, comment: &[u8]) {
        self.flush_text();
        self.event_count += 1;
        self.field(b'C', comment);
    }

    fn doctype(&mut self) {
        self.flush_text();
        self.event_count += 1;
        self.field(b'D', &[]);
    }

    fn fhp_token(&mut self, token: &FhpToken<'_>) {
        match token {
            FhpToken::OpenTag {
                name,
                attributes,
                self_closing,
                ..
            } => {
                let attrs = attributes
                    .iter()
                    .map(|attr| {
                        (
                            attr.name.as_bytes().to_vec(),
                            attr.value.as_deref().unwrap_or("").as_bytes().to_vec(),
                        )
                    })
                    .collect();
                self.start_tag(name.as_bytes(), attrs, *self_closing);
            }
            FhpToken::CloseTag { name, .. } => self.end_tag(name.as_bytes()),
            FhpToken::Text { content } => self.text(content.as_bytes()),
            FhpToken::Comment { content } => self.comment(content.as_bytes()),
            FhpToken::Doctype { .. } => self.doctype(),
            // Outside foreign content, the WHATWG tokenizer reads CDATA as a
            // bogus comment.
            FhpToken::CData { content } => self.comment(format!("[CDATA[{content}]]").as_bytes()),
        }
    }

    fn html5gum_token(&mut self, token: &html5gum::Token) {
        match token {
            html5gum::Token::StartTag(tag) => {
                let attrs = tag
                    .attributes
                    .iter()
                    .map(|(name, value)| (name.to_vec(), value.value.to_vec()))
                    .collect();
                self.start_tag(&tag.name, attrs, tag.self_closing);
            }
            html5gum::Token::EndTag(tag) => self.end_tag(&tag.name),
            html5gum::Token::String(text) => self.text(&text.value),
            html5gum::Token::Comment(comment) => self.comment(&comment.value),
            html5gum::Token::Doctype(_) => self.doctype(),
            html5gum::Token::Error(_) => {}
        }
    }

    fn html5gum_event(&mut self, event: &CallbackEvent<'_>) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.flush_text();
                self.start_tag = Some(PendingStartTag {
                    name: name.to_vec(),
                    attrs: Vec::new(),
                });
            }
            CallbackEvent::AttributeName { name } => {
                // Attributes reported before an end tag have no start tag.
                if let Some(tag) = &mut self.start_tag {
                    tag.attrs.push((name.to_vec(), Vec::new()));
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some((_, attr_value)) =
                    self.start_tag.as_mut().and_then(|tag| tag.attrs.last_mut())
                {
                    *attr_value = value.to_vec();
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(tag) = self.start_tag.take() {
                    self.start_tag(&tag.name, tag.attrs, *self_closing);
                }
            }
            CallbackEvent::EndTag { name } => {
                self.start_tag = None;
                self.end_tag(name);
            }
            CallbackEvent::String { value } => self.text(value),
            CallbackEvent::Comment { value } => self.comment(value),
            CallbackEvent::Doctype { .. } => self.doctype(),
            CallbackEvent::Error(_) => {}
        }
    }

    fn html5ever_token(&mut self, token: &html5ever::tokenizer::Token) {
        use html5ever::tokenizer::{TagKind, Token};

        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => {
                    let attrs = tag
                        .attrs
                        .iter()
                        .map(|attr| {
                            (
                                attr.name.local.as_bytes().to_vec(),
                                attr.value.as_bytes().to_vec(),
                            )
                        })
                        .collect();
                    self.start_tag(tag.name.as_bytes(), attrs, tag.self_closing);
                }
                TagKind::EndTag => self.end_tag(tag.name.as_bytes()),
            },
            Token::CharacterTokens(text) => self.text(text.as_bytes()),
            Token::NullCharacterToken => self.text(b"\0"),
            Token::CommentToken(comment) => self.comment(comment.as_bytes()),
            Token::DoctypeToken(_) => self.doctype(),
            Token::EOFToken | Token::ParseError(_) => {}
        }
    }

    fn finish(mut self) -> TokenStreamDigest {
        self.flush_text();
        TokenStreamDigest {
            event_count: self.event_count,
            canonical_bytes: self.canonical_bytes,
            fnv1a_128: self.hash,
        }
    }
}

// ---------------------------------------------------------------------------
// Checked-in contract
// ---------------------------------------------------------------------------

/// What `benchmarks/contracts.json` records for one implementation on one
/// fixture: the native count the timed function returns and, for full-token
/// implementations, the canonical token stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImplementationContract {
    pub count: u64,
    pub token_stream: Option<TokenStreamDigest>,
}

impl fmt::Display for ImplementationContract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{ \"count\": {}", self.count)?;
        if let Some(digest) = self.token_stream {
            write!(f, ", \"token_stream\": {digest}")?;
        }
        write!(f, " }}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureContract {
    pub id: &'static str,
    implementations: Vec<(Implementation, ImplementationContract)>,
}

impl FixtureContract {
    pub fn expected(&self, implementation: Implementation) -> ImplementationContract {
        self.implementations
            .iter()
            .find(|(candidate, _)| *candidate == implementation)
            .map(|(_, contract)| *contract)
            .unwrap_or_else(|| {
                panic!(
                    "missing tokenizer contract for {} on {}",
                    implementation.id(),
                    self.id
                )
            })
    }
}

const CONTRACTS_JSON: &str = include_str!("../../../benchmarks/contracts.json");
const DIGEST_ALGORITHM: &str = "fnv1a-128-length-prefixed-v1";

static FIXTURE_CONTRACTS: OnceLock<Vec<FixtureContract>> = OnceLock::new();

type JsonObject = serde_json::Map<String, serde_json::Value>;

fn required_object<'a>(object: &'a JsonObject, key: &str) -> &'a JsonObject {
    object
        .get(key)
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("tokenizer contract field {key:?} must be an object"))
}

fn required_u64(object: &JsonObject, key: &str) -> u64 {
    object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("tokenizer contract field {key:?} must be an unsigned integer"))
}

fn required_str<'a>(object: &'a JsonObject, key: &str) -> &'a str {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("tokenizer contract field {key:?} must be a string"))
}

fn parse_digest(object: &JsonObject) -> TokenStreamDigest {
    TokenStreamDigest {
        event_count: required_u64(object, "event_count"),
        canonical_bytes: required_u64(object, "canonical_bytes"),
        fnv1a_128: required_str(object, "fnv1a_128")
            .parse::<u128>()
            .unwrap_or_else(|error| panic!("invalid tokenizer fnv1a_128: {error}")),
    }
}

fn parse_fixture(object: &JsonObject) -> FixtureContract {
    let id: &'static str = Box::leak(required_str(object, "id").to_owned().into_boxed_str());
    let entries = required_object(object, "implementations");
    if let Some(unknown) = entries.keys().find(|key| {
        !Implementation::ALL
            .iter()
            .any(|known| known.id() == key.as_str())
    }) {
        panic!("unknown tokenizer implementation {unknown:?} in contract {id}");
    }
    let implementations = Implementation::ALL
        .into_iter()
        .map(|implementation| {
            let entry = required_object(entries, implementation.id());
            let token_stream = entry.get("token_stream").map(|value| {
                parse_digest(value.as_object().expect("token_stream must be an object"))
            });
            assert_eq!(
                token_stream.is_some(),
                implementation.work_class() == WorkClass::FullToken,
                "{}/{} must record a token_stream exactly when it is a full-token implementation",
                id,
                implementation.id()
            );
            (
                implementation,
                ImplementationContract {
                    count: required_u64(entry, "count"),
                    token_stream,
                },
            )
        })
        .collect();
    FixtureContract {
        id,
        implementations,
    }
}

pub fn fixture_contracts() -> &'static [FixtureContract] {
    FIXTURE_CONTRACTS.get_or_init(|| {
        let document: serde_json::Value = serde_json::from_str(CONTRACTS_JSON)
            .unwrap_or_else(|error| panic!("invalid benchmarks/contracts.json: {error}"));
        let root = document
            .as_object()
            .expect("benchmarks/contracts.json root must be an object");
        assert_eq!(
            required_u64(root, "schema_version"),
            1,
            "unsupported benchmarks/contracts.json schema"
        );
        let tokenizer = required_object(root, "tokenizer");
        assert_eq!(
            required_str(tokenizer, "algorithm"),
            DIGEST_ALGORITHM,
            "unsupported tokenizer contract digest algorithm"
        );
        let fixtures: Vec<FixtureContract> = tokenizer
            .get("fixtures")
            .and_then(serde_json::Value::as_array)
            .expect("tokenizer contract field \"fixtures\" must be an array")
            .iter()
            .map(|value| {
                parse_fixture(
                    value
                        .as_object()
                        .expect("tokenizer fixture must be an object"),
                )
            })
            .collect();
        assert!(
            !fixtures.is_empty(),
            "tokenizer benchmark contracts cannot be empty"
        );
        fixtures
    })
}

pub fn fixture_contract(id: &str) -> &'static FixtureContract {
    fixture_contracts()
        .iter()
        .find(|contract| contract.id == id)
        .unwrap_or_else(|| panic!("missing checked-in tokenizer contract for fixture {id:?}"))
}

/// Check every implementation against the checked-in contract.
///
/// Mismatches are collected and reported together with the observed values,
/// formatted as `contracts.json` entries.
pub fn validate_fixture_contract(contract: &FixtureContract, html: &str) {
    let mut mismatches = Vec::new();
    let mut fhp_streams = Vec::new();
    for implementation in Implementation::ALL {
        let actual = implementation.observe(html);
        if actual != contract.expected(implementation) {
            mismatches.push(format!("\"{}\": {actual}", implementation.id()));
        }
        if implementation.library() == Library::Fhp {
            fhp_streams.push((implementation.id(), actual.token_stream));
        }
    }
    assert!(
        mismatches.is_empty(),
        "tokenizer benchmark contract mismatch on {}; observed:\n  {}",
        contract.id,
        mismatches.join(",\n  ")
    );
    assert!(
        fhp_streams.windows(2).all(|pair| pair[0].1 == pair[1].1),
        "fhp-tokenizer entry points disagree on {}: {fhp_streams:?}",
        contract.id
    );
    validate_lol_html_passthrough(contract.id, html);
}

/// The observed contract entry for `fixture`, formatted for the `tokenizer`
/// section of `benchmarks/contracts.json`.
pub fn observed_fixture_json(fixture: &BenchFixture) -> String {
    let entries = Implementation::ALL
        .iter()
        .map(|implementation| {
            format!(
                "          \"{}\": {}",
                implementation.id(),
                implementation.observe(fixture.html)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "      {{\n        \"id\": \"{}\",\n        \"implementations\": {{\n{entries}\n        }}\n      }}",
        fixture.contract_id
    )
}

/// Both lol_html workloads must reproduce the fixture byte-for-byte, so the
/// rewriter cannot skip output it is timed for.
pub fn validate_lol_html_passthrough(fixture: &str, html: &str) {
    for implementation in [
        Implementation::LolHtmlNoopPassthrough,
        Implementation::LolHtmlElementTextComment,
    ] {
        let mut output = Vec::with_capacity(html.len());
        let collect = |chunk: &[u8]| output.extend_from_slice(chunk);
        if implementation == Implementation::LolHtmlNoopPassthrough {
            lol_html_noop_passthrough(html, collect);
        } else {
            lol_html_element_text_comment_handlers(html, collect);
        }
        assert!(
            output == html.as_bytes(),
            "{} changed fixture bytes on {fixture}",
            implementation.id()
        );
    }
}

// ---------------------------------------------------------------------------
// Contract-equal groups
// ---------------------------------------------------------------------------

/// API shapes compared in `contract_equal` groups. Each pairs one fhp entry
/// point with the competitors that deliver tokens the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiShape {
    /// Push tokens to a callback without collecting them.
    PushCallback,
    /// Hand the caller owned token values.
    TokenValues,
}

impl ApiShape {
    pub const ALL: [Self; 2] = [Self::PushCallback, Self::TokenValues];

    pub const fn namespace(self) -> &'static str {
        match self {
            Self::PushCallback => "push_callback",
            Self::TokenValues => "token_values",
        }
    }

    pub const fn fhp(self) -> Implementation {
        match self {
            Self::PushCallback => Implementation::FhpTokenizeWith,
            Self::TokenValues => Implementation::FhpTokenize,
        }
    }

    pub const fn competitors(self) -> &'static [Implementation] {
        match self {
            Self::PushCallback => &[
                Implementation::Html5gumCallback,
                Implementation::Html5everTokenizer,
            ],
            Self::TokenValues => &[Implementation::Html5gumIterator],
        }
    }
}

/// Members of each `contract_equal` group: the fhp entry point plus every
/// competitor whose checked-in token stream equals it. Groups without a
/// competitor are omitted, since there is nothing to compare.
pub fn contract_equal_sets(contract: &FixtureContract) -> Vec<(ApiShape, Vec<Implementation>)> {
    ApiShape::ALL
        .into_iter()
        .filter_map(|shape| {
            let reference = contract.expected(shape.fhp()).token_stream;
            let mut members = vec![shape.fhp()];
            members.extend(
                shape
                    .competitors()
                    .iter()
                    .copied()
                    .filter(|competitor| contract.expected(*competitor).token_stream == reference),
            );
            (members.len() > 1).then_some((shape, members))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Registration order
// ---------------------------------------------------------------------------

/// The six accepted `FHP_BENCH_ORDER` values. `scripts/bench.py` names them
/// after the DOM harness's fhp/scraper/tl blocks; this harness binds the
/// second and third blocks to html5gum and html5ever.
pub const BENCH_ORDERS: &[(&str, [Library; 3])] = &[
    (
        "fhp-scraper-tl",
        [Library::Fhp, Library::Html5gum, Library::Html5ever],
    ),
    (
        "fhp-tl-scraper",
        [Library::Fhp, Library::Html5ever, Library::Html5gum],
    ),
    (
        "scraper-fhp-tl",
        [Library::Html5gum, Library::Fhp, Library::Html5ever],
    ),
    (
        "scraper-tl-fhp",
        [Library::Html5gum, Library::Html5ever, Library::Fhp],
    ),
    (
        "tl-fhp-scraper",
        [Library::Html5ever, Library::Fhp, Library::Html5gum],
    ),
    (
        "tl-scraper-fhp",
        [Library::Html5ever, Library::Html5gum, Library::Fhp],
    ),
];

/// Resolve the registration order used to expose order/thermal bias. Local
/// runs default to fhp first; publication runs all six permutations.
pub fn bench_order() -> [Library; 3] {
    match std::env::var("FHP_BENCH_ORDER") {
        Ok(value) => parse_bench_order(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_bench_order(None),
        Err(error) => panic!("could not read FHP_BENCH_ORDER: {error}"),
    }
}

pub fn parse_bench_order(value: Option<&str>) -> [Library; 3] {
    let name = value.unwrap_or(BENCH_ORDERS[0].0);
    BENCH_ORDERS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, order)| *order)
        .unwrap_or_else(|| {
            panic!(
                "invalid FHP_BENCH_ORDER={name:?}; expected one of: {}",
                BENCH_ORDERS
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Order the full-token libraries as permuted blocks without splitting a
/// library's implementations. lol_html and tl stay last: they are absolute
/// references outside every contract-equal comparison.
pub fn order_implementations(
    implementations: &[Implementation],
    order: [Library; 3],
) -> Vec<Implementation> {
    let mut ordered = Vec::with_capacity(implementations.len());
    for library in order {
        ordered.extend(
            implementations
                .iter()
                .copied()
                .filter(|implementation| implementation.library() == library),
        );
    }
    ordered.extend(
        implementations
            .iter()
            .copied()
            .filter(|implementation| !order.contains(&implementation.library())),
    );
    ordered
}
