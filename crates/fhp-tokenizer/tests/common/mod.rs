//! Helpers shared by the tokenizer integration tests.

#![allow(dead_code)]

use fhp_core::attr::iter_raw_attrs;
use fhp_core::tag::Tag;
use fhp_tokenizer::token::Token;
use fhp_tokenizer::{TreeSink, tokenize_into};

/// An open tag as seen by a [`TreeSink`]: name, attributes parsed from the
/// raw attribute region, and the self-closing flag.
pub type SinkOpenTag = (String, Vec<(String, Option<String>)>, bool);

/// Records [`TreeSink`] events as `kind:payload` strings.
#[derive(Default)]
pub struct EventSink {
    pub events: Vec<String>,
    pub open_tags: Vec<SinkOpenTag>,
}

impl TreeSink for EventSink {
    fn open_tag(&mut self, _tag: Tag, name: &str, attr_raw: &str, self_closing: bool) {
        self.events.push(format!("open:{name}"));
        let attrs = iter_raw_attrs(attr_raw)
            .map(|attr| (attr.name.to_owned(), attr.value.map(str::to_owned)))
            .collect();
        self.open_tags.push((name.to_owned(), attrs, self_closing));
    }
    fn close_tag(&mut self, _tag: Tag, name: &str) {
        self.events.push(format!("close:{name}"));
    }
    fn text(&mut self, t: &str) {
        self.events.push(format!("text:{t}"));
    }
    fn comment(&mut self, c: &str) {
        self.events.push(format!("comment:{c}"));
    }
    fn doctype(&mut self, c: &str) {
        self.events.push(format!("doctype:{c}"));
    }
    fn cdata(&mut self, c: &str) {
        self.events.push(format!("cdata:{c}"));
    }
}

/// Run `input` through [`tokenize_into`] and return the recorded sink.
pub fn sink(input: &str) -> EventSink {
    let mut sink = EventSink::default();
    tokenize_into(input, &mut sink);
    sink
}

/// The events [`EventSink`] records for `input`.
pub fn sink_events(input: &str) -> Vec<String> {
    sink(input).events
}

/// The [`EventSink`] events and open tags equivalent to `tokens`.
///
/// Text is compared verbatim, so callers must use inputs without character
/// references (the sink receives raw text).
pub fn token_events(tokens: &[Token<'_>]) -> (Vec<String>, Vec<SinkOpenTag>) {
    let mut events = Vec::new();
    let mut open_tags = Vec::new();
    for token in tokens {
        match token {
            Token::OpenTag {
                name,
                attributes,
                self_closing,
                ..
            } => {
                events.push(format!("open:{name}"));
                let attrs = attributes
                    .iter()
                    .map(|attr| {
                        (
                            attr.name.to_string(),
                            attr.value.as_ref().map(|value| value.to_string()),
                        )
                    })
                    .collect();
                open_tags.push((name.to_string(), attrs, *self_closing));
            }
            Token::CloseTag { name, .. } => events.push(format!("close:{name}")),
            Token::Text { content } => events.push(format!("text:{content}")),
            Token::Comment { content } => events.push(format!("comment:{content}")),
            Token::Doctype { content } => events.push(format!("doctype:{content}")),
            Token::CData { content } => events.push(format!("cdata:{content}")),
        }
    }
    (events, open_tags)
}
