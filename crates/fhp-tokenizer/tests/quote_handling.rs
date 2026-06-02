//! Tests for quote handling: quotes in text/comments must NOT mask following
//! tags, while `>` inside an attribute value must NOT close the tag early.
//!
//! Regression coverage for the global quote-masking bug where a lone `'`/`"`
//! in text content swallowed the rest of the document.

use fhp_core::tag::Tag;
use fhp_tokenizer::token::Token;
use fhp_tokenizer::{TreeSink, tokenize, tokenize_into, tokenize_with};

// ---- helpers -------------------------------------------------------------

fn collect_with(input: &str) -> Vec<Token<'_>> {
    let mut v = Vec::new();
    tokenize_with(input, |t| v.push(t));
    v
}

#[derive(Default)]
struct EventSink {
    events: Vec<String>,
}

impl TreeSink for EventSink {
    fn open_tag(&mut self, _tag: Tag, name: &str, _attr_raw: &str, _self_closing: bool) {
        self.events.push(format!("open:{name}"));
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
    fn doctype(&mut self, _c: &str) {}
    fn cdata(&mut self, _c: &str) {}
}

fn sink_events(input: &str) -> Vec<String> {
    let mut s = EventSink::default();
    tokenize_into(input, &mut s);
    s.events
}

fn count_open(tokens: &[Token<'_>], want: &str) -> usize {
    tokens
        .iter()
        .filter(|t| matches!(t, Token::OpenTag { name, .. } if name == want))
        .count()
}
fn count_close(tokens: &[Token<'_>], want: &str) -> usize {
    tokens
        .iter()
        .filter(|t| matches!(t, Token::CloseTag { name, .. } if name == want))
        .count()
}

// ---- apostrophe in text --------------------------------------------------

#[test]
fn apostrophe_in_text_two_stage() {
    let toks = tokenize("<p>It's me</p><div>x</div>");
    assert_eq!(count_close(&toks, "p"), 1, "</p> must be recognized");
    assert_eq!(
        count_open(&toks, "div"),
        1,
        "<div> after apostrophe must parse"
    );
    assert_eq!(count_close(&toks, "div"), 1);
}

#[test]
fn apostrophe_in_text_fused_cb() {
    let toks = collect_with("<p>It's me</p><div>x</div>");
    assert_eq!(count_close(&toks, "p"), 1);
    assert_eq!(count_open(&toks, "div"), 1);
}

#[test]
fn apostrophe_in_text_sink() {
    let ev = sink_events("<p>It's me</p><div>x</div>");
    assert!(ev.contains(&"close:p".to_string()), "got {ev:?}");
    assert!(ev.contains(&"open:div".to_string()), "got {ev:?}");
    assert!(ev.contains(&"text:It's me".to_string()), "got {ev:?}");
}

// ---- double-quote in text ------------------------------------------------

#[test]
fn dquote_in_text_two_stage() {
    let toks = tokenize("<p>a 6\" nail</p><div>y</div>");
    assert_eq!(count_open(&toks, "div"), 1);
    assert_eq!(count_close(&toks, "div"), 1);
}

#[test]
fn dquote_in_text_sink() {
    let ev = sink_events("<p>a 6\" nail</p><div>y</div>");
    assert!(ev.contains(&"open:div".to_string()), "got {ev:?}");
}

// ---- regression: `>` inside attribute value must not close tag ----------

#[test]
fn gt_inside_double_quoted_attr() {
    let toks = tokenize("<a title=\"x>y\">hi</a>");
    assert_eq!(count_open(&toks, "a"), 1);
    let attr_val = toks.iter().find_map(|t| match t {
        Token::OpenTag { attributes, .. } => attributes
            .iter()
            .find(|a| a.name == "title")
            .map(|a| a.value.clone()),
        _ => None,
    });
    assert_eq!(
        attr_val,
        Some(Some("x>y".into())),
        "title attr should contain the full x>y"
    );
}

#[test]
fn gt_inside_single_quoted_attr() {
    let toks = tokenize("<a title='x>y'>hi</a>");
    assert_eq!(count_open(&toks, "a"), 1);
    assert_eq!(count_close(&toks, "a"), 1);
}

#[test]
fn gt_inside_attr_sink() {
    let ev = sink_events("<a title=\"x>y\">hi</a>");
    assert!(ev.contains(&"open:a".to_string()), "got {ev:?}");
    assert!(ev.contains(&"close:a".to_string()), "got {ev:?}");
    assert!(ev.contains(&"text:hi".to_string()), "got {ev:?}");
}

// ---- comment with odd quote must still terminate -------------------------

#[test]
fn odd_quote_in_comment_terminates() {
    let toks = tokenize("<!-- 6\" inch --><div>x</div>");
    assert_eq!(
        count_open(&toks, "div"),
        1,
        "comment with stray quote must terminate"
    );
    assert!(toks.iter().any(|t| matches!(t, Token::Comment { .. })));
}
