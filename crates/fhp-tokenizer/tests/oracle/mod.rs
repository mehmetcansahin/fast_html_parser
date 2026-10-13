//! Differential, invariant, and metamorphic checks for the tokenizer.
//!
//! Shared by `tests/differential.rs` and the `tokenizer_differential` fuzz
//! target, which includes this file by path. It therefore uses only the public
//! `fhp-core` and `fhp-tokenizer` APIs.
//!
//! [`check`] runs one input through every entry point and panics with a
//! report on the first disagreement:
//!
//! - `tokenize_with` and `tokenize_into` must match `tokenize` exactly;
//! - `StreamTokenizer::feed` and `feed_str_with` must match it for any
//!   chunking, up to how text runs are split;
//! - the `tokenize_into` spans must borrow from the input in order, never lose
//!   text bytes, and never contain bytes that start markup in their context;
//! - tags, attributes, comments, doctypes, and CDATA sections must be
//!   well-formed;
//! - shifting the input across 64-byte SIMD blocks must not change any token;
//! - serializing the tokens and tokenizing again must reproduce them.

#![allow(dead_code)]

use std::collections::HashSet;
use std::fmt::Write as _;

use fhp_core::attr::iter_raw_attrs;
use fhp_core::is_html_whitespace;
use fhp_core::tag::Tag;
use fhp_tokenizer::entity::{decode_attribute_entities, decode_entities};
use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token;
use fhp_tokenizer::{TreeSink, tokenize, tokenize_into, tokenize_with};

/// Size of the tokenizer's SIMD processing block.
const BLOCK_SIZE: usize = 64;

/// Inputs up to this size are printed in full when a check fails.
const MAX_REPORTED_INPUT: usize = 1024;

/// A token with owned payloads, comparable across entry points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Open {
        tag: Tag,
        name: String,
        attrs: Vec<(String, Option<String>)>,
        self_closing: bool,
    },
    Close {
        tag: Tag,
        name: String,
    },
    Text(String),
    Comment(String),
    Doctype(String),
    CData(String),
}

impl Event {
    fn from_token(token: &Token<'_>) -> Self {
        match token {
            Token::OpenTag {
                tag,
                name,
                attributes,
                self_closing,
            } => Self::Open {
                tag: *tag,
                name: name.to_string(),
                attrs: attributes
                    .iter()
                    .map(|attr| {
                        (
                            attr.name.to_string(),
                            attr.value.as_ref().map(|value| value.to_string()),
                        )
                    })
                    .collect(),
                self_closing: *self_closing,
            },
            Token::CloseTag { tag, name } => Self::Close {
                tag: *tag,
                name: name.to_string(),
            },
            Token::Text { content } => Self::Text(content.to_string()),
            Token::Comment { content } => Self::Comment(content.to_string()),
            Token::Doctype { content } => Self::Doctype(content.to_string()),
            Token::CData { content } => Self::CData(content.to_string()),
        }
    }
}

/// How the streaming entry points receive their input.
#[derive(Clone, Copy, Debug)]
pub enum Chunking {
    /// Chunks of this many bytes (the last one may be shorter).
    Fixed(usize),
    /// Pseudo-random chunk lengths from this seed: mostly a few bytes,
    /// sometimes a few hundred.
    Random(u64),
}

impl Chunking {
    /// Ascending chunk end offsets; the last one is `len`.
    fn boundaries(self, len: usize) -> Vec<usize> {
        let mut ends = Vec::new();
        let mut end = 0;
        let mut rng = match self {
            Self::Fixed(_) => SplitMix64(0),
            Self::Random(seed) => SplitMix64(seed),
        };
        while end < len {
            let step = match self {
                Self::Fixed(size) => size.max(1),
                Self::Random(_) => {
                    let roll = rng.next();
                    let bound = if roll % 4 == 0 { 300 } else { 8 };
                    1 + (roll >> 8) as usize % bound
                }
            };
            end = (end + step).min(len);
            ends.push(end);
        }
        ends
    }
}

/// SplitMix64: a tiny deterministic generator, so failures reproduce exactly.
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// Run every check on `input`, panicking with a report on the first failure.
///
/// `chunking` drives both streaming entry points; `shift % 64` is the length
/// of the neutral prefix the block-alignment check prepends (`0` skips it).
pub fn check(input: &str, chunking: Chunking, shift: usize) {
    let run = Run {
        input,
        chunking,
        shift: shift % BLOCK_SIZE,
        decoding: entity_decoding_enabled(),
    };

    let expected: Vec<Event> = tokenize(input).iter().map(Event::from_token).collect();
    run.check_token_invariants(&expected);

    let mut callback = Vec::new();
    tokenize_with(input, |token| callback.push(Event::from_token(&token)));
    run.assert_same("tokenize_with", &expected, &callback);

    let sink = SpanSink::record(input, run.decoding);
    run.assert_same("tokenize_into", &expected, &sink.events);
    run.check_spans(&sink);

    let expected = merge_text(expected);
    run.check_stream_bytes(&expected);
    run.check_stream_str(&expected);
    run.check_block_shift(&expected);
    run.check_round_trip(&expected);
}

/// Whether this build of the tokenizer decodes character references.
fn entity_decoding_enabled() -> bool {
    matches!(
        tokenize("&amp;").as_slice(),
        [Token::Text { content }] if content == "&"
    )
}

/// Concatenate adjacent text events: streaming may split a text run at any
/// chunk boundary.
fn merge_text(events: impl IntoIterator<Item = Event>) -> Vec<Event> {
    let mut merged: Vec<Event> = Vec::new();
    for event in events {
        if let Event::Text(text) = &event {
            if let Some(Event::Text(previous)) = merged.last_mut() {
                previous.push_str(text);
                continue;
            }
        }
        merged.push(event);
    }
    merged
}

/// Which tokenizer state text content is read in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextContext {
    Data,
    RawText(Tag),
    RcData(Tag),
    Plaintext,
}

impl TextContext {
    /// The context after the start tag `tag`.
    fn after_open(tag: Tag) -> Self {
        if tag == Tag::Plaintext {
            Self::Plaintext
        } else if tag.is_raw_text() {
            Self::RawText(tag)
        } else if tag.is_rcdata() {
            Self::RcData(tag)
        } else {
            Self::Data
        }
    }

    fn decodes_entities(self) -> bool {
        matches!(self, Self::Data | Self::RcData(_))
    }
}

/// A non-empty slice a [`TreeSink`] received, as a byte range of the input.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
    /// The text context for text slices; `None` for markup slices.
    text: Option<TextContext>,
}

/// Records [`TreeSink`] events as [`Event`]s, decoding them the way
/// `tokenize` does, together with the input range of every slice.
struct SpanSink<'i> {
    input: &'i str,
    decoding: bool,
    context: TextContext,
    events: Vec<Event>,
    spans: Vec<Span>,
    errors: Vec<String>,
}

impl<'i> SpanSink<'i> {
    fn record(input: &'i str, decoding: bool) -> Self {
        let mut sink = Self {
            input,
            decoding,
            context: TextContext::Data,
            events: Vec::new(),
            spans: Vec::new(),
            errors: Vec::new(),
        };
        tokenize_into(input, &mut sink);
        sink
    }

    /// Record the input range of `slice`. Empty slices are skipped: the
    /// parser hands out a static `""` for empty ranges.
    fn span(&mut self, slice: &str, text: Option<TextContext>, what: &str) {
        if slice.is_empty() {
            return;
        }
        let start = (slice.as_ptr() as usize).checked_sub(self.input.as_ptr() as usize);
        match start {
            Some(start) if start + slice.len() <= self.input.len() => self.spans.push(Span {
                start,
                end: start + slice.len(),
                text,
            }),
            _ => self
                .errors
                .push(format!("{what} {slice:?} does not borrow from the input")),
        }
    }

    /// Only text and the element's own end tag may follow a raw-text, RCDATA,
    /// or plaintext start tag.
    fn expect_markup_allowed(&mut self, what: &str) {
        if self.context != TextContext::Data {
            self.errors
                .push(format!("{what} inside text context {:?}", self.context));
        }
    }
}

impl TreeSink for SpanSink<'_> {
    fn open_tag(&mut self, tag: Tag, name: &str, attr_raw: &str, self_closing: bool) {
        self.expect_markup_allowed("start tag");
        self.span(name, None, "start tag name");
        self.span(attr_raw, None, "attribute region");

        // First wins, ASCII case-insensitively. Written independently of
        // `SeenAttributes` so its fingerprint pre-filter is checked too.
        let mut seen = HashSet::new();
        let attrs = iter_raw_attrs(attr_raw)
            .filter(|attr| seen.insert(attr.name.to_ascii_lowercase()))
            .map(|attr| {
                let value = attr.value.map(|value| {
                    if self.decoding {
                        decode_attribute_entities(value).into_owned()
                    } else {
                        value.to_owned()
                    }
                });
                (attr.name.to_owned(), value)
            })
            .collect();
        self.events.push(Event::Open {
            tag,
            name: name.to_owned(),
            attrs,
            self_closing,
        });
        self.context = TextContext::after_open(tag);
    }

    fn close_tag(&mut self, tag: Tag, name: &str) {
        match self.context {
            TextContext::RawText(open) | TextContext::RcData(open) if open == tag => {}
            _ => self.expect_markup_allowed("end tag"),
        }
        self.span(name, None, "end tag name");
        self.events.push(Event::Close {
            tag,
            name: name.to_owned(),
        });
        self.context = TextContext::Data;
    }

    fn text(&mut self, content: &str) {
        let context = self.context;
        self.span(content, Some(context), "text");
        let text = if self.decoding && context.decodes_entities() {
            decode_entities(content).into_owned()
        } else {
            content.to_owned()
        };
        self.events.push(Event::Text(text));
    }

    fn comment(&mut self, content: &str) {
        self.expect_markup_allowed("comment");
        self.span(content, None, "comment");
        self.events.push(Event::Comment(content.to_owned()));
    }

    fn doctype(&mut self, content: &str) {
        self.expect_markup_allowed("doctype");
        self.span(content, None, "doctype");
        self.events.push(Event::Doctype(content.to_owned()));
    }

    fn cdata(&mut self, content: &str) {
        self.expect_markup_allowed("CDATA section");
        self.span(content, None, "CDATA section");
        self.events.push(Event::CData(content.to_owned()));
    }
}

/// One input under test, with the parameters needed to reproduce a failure.
struct Run<'i> {
    input: &'i str,
    chunking: Chunking,
    shift: usize,
    decoding: bool,
}

impl Run<'_> {
    fn fail(&self, check: &str, detail: &str) -> ! {
        let mut report = format!(
            "tokenizer check `{check}` failed\n  chunking: {:?}\n  shift: {}\n  \
             entity decoding: {}\n  input length: {}\n",
            self.chunking,
            self.shift,
            self.decoding,
            self.input.len()
        );
        if self.input.len() <= MAX_REPORTED_INPUT {
            let _ = writeln!(report, "  input: {:?}", self.input);
        }
        panic!("{report}{detail}");
    }

    fn assert_same(&self, check: &str, expected: &[Event], actual: &[Event]) {
        if let Some(diff) = diff_events(expected, actual) {
            self.fail(check, &diff);
        }
    }

    fn check_token_invariants(&self, events: &[Event]) {
        for (index, event) in events.iter().enumerate() {
            let problem = match event {
                Event::Open {
                    tag, name, attrs, ..
                } => tag_name_problem(*tag, name).or_else(|| attributes_problem(attrs)),
                Event::Close { tag, name } => tag_name_problem(*tag, name),
                Event::Text(_) => None,
                Event::Comment(content) => (content.contains("-->") || content.contains("--!>"))
                    .then(|| "comment contains its terminator".to_owned()),
                Event::Doctype(content) => content
                    .contains('>')
                    .then(|| "doctype contains `>`".to_owned()),
                Event::CData(content) => content
                    .contains("]]>")
                    .then(|| "CDATA section contains `]]>`".to_owned()),
            };
            if let Some(problem) = problem {
                self.fail(
                    "token invariants",
                    &format!("event {index} {event:?}: {problem}"),
                );
            }
        }
    }

    fn check_spans(&self, sink: &SpanSink<'_>) {
        if let Some(error) = sink.errors.first() {
            self.fail("tokenize_into spans", error);
        }
        let bytes = self.input.as_bytes();
        let len = bytes.len();
        for (index, span) in sink.spans.iter().enumerate() {
            let prev_end = index.checked_sub(1).map_or(0, |prev| sink.spans[prev].end);
            if span.start < prev_end {
                self.fail(
                    "tokenize_into spans",
                    &format!(
                        "span {index} {span:?} overlaps or precedes span ending at {prev_end}"
                    ),
                );
            }
            let Some(context) = span.text else {
                continue;
            };

            // Text starts at the input start or right after a `>` (tag,
            // comment, `</>`, ...), and ends at the input end or right before
            // a `<`. Anything else means text bytes were lost.
            let before = &bytes[prev_end..span.start];
            if !before.is_empty() && before.last() != Some(&b'>') {
                self.fail(
                    "lost text",
                    &format!(
                        "bytes {prev_end}..{} before text span {index} are {:?}",
                        span.start,
                        String::from_utf8_lossy(before)
                    ),
                );
            }
            let next_start = sink.spans.get(index + 1).map_or(len, |next| next.start);
            let after = &bytes[span.end..next_start];
            if !after.is_empty() && after[0] != b'<' {
                self.fail(
                    "lost text",
                    &format!(
                        "bytes {}..{next_start} after text span {index} are {:?}",
                        span.end,
                        String::from_utf8_lossy(after)
                    ),
                );
            }

            if let Some(at) = markup_in_text(bytes, span.start, span.end, context) {
                self.fail(
                    "markup in text",
                    &format!(
                        "text span {index} ({context:?}) contains markup at byte {at}: {:?}",
                        String::from_utf8_lossy(&bytes[at..len.min(at + 16)])
                    ),
                );
            }
        }
    }

    fn check_stream_bytes(&self, expected: &[Event]) {
        let bytes = self.input.as_bytes();
        let mut tokenizer = StreamTokenizer::new();
        let mut events = Vec::new();
        let mut start = 0;
        for end in self.chunking.boundaries(bytes.len()) {
            let tokens = tokenizer.feed(&bytes[start..end]);
            events.extend(tokens.iter().map(Event::from_token));
            start = end;
        }
        events.extend(tokenizer.finish().iter().map(Event::from_token));
        self.assert_same("StreamTokenizer::feed", expected, &merge_text(events));
    }

    fn check_stream_str(&self, expected: &[Event]) {
        let mut tokenizer = StreamTokenizer::new();
        let mut events = Vec::new();
        let mut start = 0;
        for mut end in self.chunking.boundaries(self.input.len()) {
            while !self.input.is_char_boundary(end) {
                end -= 1;
            }
            if end <= start {
                continue;
            }
            tokenizer.feed_str_with(&self.input[start..end], |token| {
                events.push(Event::from_token(token));
            });
            start = end;
        }
        tokenizer.finish_with(|token| events.push(Event::from_token(token)));
        self.assert_same(
            "StreamTokenizer::feed_str_with",
            expected,
            &merge_text(events),
        );
    }

    /// A text prefix moves every delimiter to a different offset within its
    /// 64-byte block but must only extend the leading text.
    fn check_block_shift(&self, expected: &[Event]) {
        if self.shift == 0 {
            return;
        }
        let prefix = "x".repeat(self.shift);
        let shifted = format!("{prefix}{}", self.input);
        let actual = merge_text(tokenize(&shifted).iter().map(Event::from_token));

        let mut expected = expected.to_vec();
        match expected.first_mut() {
            Some(Event::Text(text)) => text.insert_str(0, &prefix),
            _ => expected.insert(0, Event::Text(prefix)),
        }
        self.assert_same("block shift", &expected, &actual);
    }

    /// Serializing the tokens and tokenizing the result must reproduce them.
    fn check_round_trip(&self, expected: &[Event]) {
        let Some(html) = serialize(expected, self.decoding) else {
            return;
        };
        let actual = merge_text(tokenize(&html).iter().map(Event::from_token));
        if let Some(diff) = diff_events(expected, &actual) {
            let serialized = if html.len() <= MAX_REPORTED_INPUT {
                format!("serialized: {html:?}\n")
            } else {
                String::new()
            };
            self.fail("round trip", &format!("{serialized}{diff}"));
        }
    }
}

/// The first differing event and its neighbours, or `None` when equal.
fn diff_events(expected: &[Event], actual: &[Event]) -> Option<String> {
    if expected == actual {
        return None;
    }
    let first = expected
        .iter()
        .zip(actual)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    let mut diff = format!(
        "first difference at event {first} ({} expected, {} actual events)\n",
        expected.len(),
        actual.len()
    );
    for (label, events) in [("expected", expected), ("actual", actual)] {
        let _ = writeln!(diff, "  {label}:");
        for index in first.saturating_sub(2)..first + 3 {
            if let Some(event) = events.get(index) {
                let _ = writeln!(diff, "    [{index}] {event:?}");
            }
        }
    }
    Some(diff)
}

/// Why a start or end tag name is malformed, if it is.
fn tag_name_problem(tag: Tag, name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        return Some("tag name does not start with an ASCII letter".to_owned());
    }
    if bytes
        .iter()
        .any(|&byte| is_html_whitespace(byte) || byte == b'/' || byte == b'>')
    {
        return Some("tag name contains whitespace, `/`, or `>`".to_owned());
    }
    if Tag::from_bytes(bytes) != tag {
        return Some(format!(
            "tag {tag:?} does not match name (expected {:?})",
            Tag::from_bytes(bytes)
        ));
    }
    None
}

/// Why an attribute list is malformed, if it is.
fn attributes_problem(attrs: &[(String, Option<String>)]) -> Option<String> {
    let mut seen = HashSet::new();
    for (name, _) in attrs {
        let bytes = name.as_bytes();
        if bytes.is_empty() {
            return Some("empty attribute name".to_owned());
        }
        if bytes.iter().enumerate().any(|(index, &byte)| {
            is_html_whitespace(byte) || byte == b'/' || byte == b'>' || (byte == b'=' && index > 0)
        }) {
            return Some(format!("attribute name {name:?} contains a delimiter"));
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            return Some(format!("duplicate attribute {name:?}"));
        }
    }
    None
}

/// Position of the first `<` in `bytes[start..end]` that starts markup in
/// `context`. Looks past `end` because a `<` at the end of a text slice is
/// classified by the bytes that follow it.
fn markup_in_text(bytes: &[u8], start: usize, end: usize, context: TextContext) -> Option<usize> {
    let starts_markup = |at: usize| -> bool {
        match context {
            // WHATWG tag open state: `<!`, `<?`, and `<` + letter start
            // markup; `</` does unless it ends the input.
            TextContext::Data => match bytes.get(at + 1) {
                Some(byte) if byte.is_ascii_alphabetic() || matches!(byte, b'!' | b'?') => true,
                Some(b'/') => at + 2 < bytes.len(),
                _ => false,
            },
            // Script escape states decide whether `</script` ends the
            // element; the other text elements end at their end tag.
            TextContext::RawText(Tag::Script) | TextContext::Plaintext => false,
            TextContext::RawText(tag) | TextContext::RcData(tag) => {
                let name = tag.as_str().unwrap_or_default().as_bytes();
                let name_end = at + 2 + name.len();
                bytes.get(at + 1) == Some(&b'/')
                    && bytes
                        .get(at + 2..name_end)
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
                    && bytes.get(name_end).is_some_and(|&byte| {
                        byte == b'>' || byte == b'/' || is_html_whitespace(byte)
                    })
            }
        }
    };
    (start..end).find(|&at| bytes[at] == b'<' && starts_markup(at))
}

/// Serialize events so that tokenizing the result reproduces them.
///
/// Without entity decoding nothing can be escaped, so this returns `None` when
/// an attribute value contains both quote characters or a text run contains
/// `<` followed by markup-starting bytes. Such a run is valid output when
/// ignored markup (`</>`) separated two text tokens that merging joined.
fn serialize(events: &[Event], decoding: bool) -> Option<String> {
    let mut out = String::new();
    let mut context = TextContext::Data;
    for event in events {
        match event {
            Event::Open {
                tag,
                name,
                attrs,
                self_closing,
            } => {
                out.push('<');
                out.push_str(name);
                let mut after_boolean = false;
                for (attr, value) in attrs {
                    // After `a `, a `=` would start a value for `a`; a `/`
                    // returns to the before-attribute-name state, where a
                    // leading `=` belongs to the name.
                    out.push_str(if after_boolean && attr.starts_with('=') {
                        " /"
                    } else {
                        " "
                    });
                    after_boolean = value.is_none();
                    out.push_str(attr);
                    if let Some(value) = value {
                        out.push('=');
                        push_attribute_value(&mut out, value, decoding)?;
                    }
                }
                if *self_closing {
                    out.push('/');
                }
                out.push('>');
                context = TextContext::after_open(*tag);
            }
            Event::Close { name, .. } => {
                let _ = write!(out, "</{name}>");
                context = TextContext::Data;
            }
            Event::Text(text) if decoding && context.decodes_entities() => {
                for ch in text.chars() {
                    match ch {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        _ => out.push(ch),
                    }
                }
            }
            Event::Text(text) => {
                let ambiguous = context.decodes_entities()
                    && text.as_bytes().windows(2).any(|pair| {
                        pair[0] == b'<'
                            && (pair[1].is_ascii_alphabetic()
                                || matches!(pair[1], b'!' | b'?' | b'/'))
                    });
                if ambiguous {
                    return None;
                }
                out.push_str(text);
            }
            Event::Comment(content) => {
                let _ = write!(out, "<!--{content}-->");
            }
            Event::Doctype(content) => {
                let _ = write!(out, "<!DOCTYPE {content}>");
            }
            Event::CData(content) => {
                let _ = write!(out, "<![CDATA[{content}]]>");
            }
        }
    }
    Some(out)
}

fn push_attribute_value(out: &mut String, value: &str, decoding: bool) -> Option<()> {
    if decoding {
        out.push('"');
        for ch in value.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '"' => out.push_str("&quot;"),
                _ => out.push(ch),
            }
        }
        out.push('"');
        return Some(());
    }
    let quote = if !value.contains('"') {
        '"'
    } else if !value.contains('\'') {
        '\''
    } else {
        return None;
    };
    out.push(quote);
    out.push_str(value);
    out.push(quote);
    Some(())
}
