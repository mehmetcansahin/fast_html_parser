//! Tokenization time must grow linearly with input size.
//!
//! Every adversarial pattern is tokenized at `SMALL` and `GROWTH * SMALL`
//! bytes, one-shot and streamed in small chunks. Linear work takes about
//! `GROWTH` times longer at the larger size and quadratic work `GROWTH²`
//! times longer; the test fails above `MAX_RATIO`, which leaves headroom for
//! timer and scheduler noise while staying far below the quadratic ratio.
//! Unterminated constructs matter most: they keep the streaming tokenizer's
//! residual buffer growing, so any per-feed rescan of it is quadratic.

use std::hint::black_box;
use std::time::{Duration, Instant};

use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::tokenize;

const SMALL: usize = 16 * 1024;
const GROWTH: usize = 8;
const MAX_RATIO: f64 = 3.0 * GROWTH as f64;

/// Streaming chunk size: small and odd, so terminators straddle chunks.
const CHUNK: usize = 7;

/// Adversarial inputs: a prefix followed by a unit repeated to `len` bytes.
const PATTERNS: &[(&str, &str, &str)] = &[
    ("plain text", "", "x"),
    ("less-than run", "", "<"),
    ("unclosed comment", "<!--", "a"),
    ("comment dashes", "<!--", "-"),
    ("comment bang dashes", "<!--", "--!"),
    ("unclosed doctype", "<!DOCTYPE ", "x"),
    ("unclosed CDATA", "<![CDATA[", "]"),
    ("unclosed bogus comment", "<?", "x"),
    ("unclosed tag", "<a", " b"),
    ("unclosed quoted value", "<a b=\"", "x"),
    ("quote toggles", "<a", " b=\"\""),
    ("many attributes", "<a", " b=1"),
    ("many tags", "", "<a>"),
    ("unterminated end tags", "", "</a"),
    ("bogus comment starts", "", "<?>"),
    ("unclosed script", "<script>", "x"),
    ("script escapes", "<script>", "<!--<script>"),
    ("script end tag prefixes", "<script>", "</scrip"),
    ("RCDATA end tag prefixes", "<title>", "</titl"),
    ("raw text end tag prefixes", "<style>", "</style"),
    ("plaintext", "<plaintext>", "</plaintext>"),
    ("ampersands", "", "&"),
    ("legacy references", "", "&amp"),
    ("numeric reference starts", "", "&#"),
    ("long numeric reference", "&#x", "f"),
];

#[test]
fn tokenization_time_is_linear_in_input_size() {
    let mut violations = Vec::new();
    for &(name, prefix, unit) in PATTERNS {
        let small = build(prefix, unit, SMALL);
        let large = build(prefix, unit, SMALL * GROWTH);
        for (entry, run) in [
            ("tokenize", one_shot as fn(&str)),
            ("StreamTokenizer::feed", streamed),
        ] {
            let ratio = seconds(|| run(&large)) / seconds(|| run(&small));
            if ratio > MAX_RATIO {
                violations.push(format!("  {name} via {entry}: {ratio:.1}x"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "time grew more than {MAX_RATIO}x for {GROWTH}x more input:\n{}",
        violations.join("\n")
    );
}

fn build(prefix: &str, unit: &str, len: usize) -> String {
    let mut input = String::with_capacity(len + unit.len());
    input.push_str(prefix);
    while input.len() < len {
        input.push_str(unit);
    }
    input
}

fn one_shot(input: &str) {
    black_box(tokenize(black_box(input)));
}

fn streamed(input: &str) {
    let mut tokenizer = StreamTokenizer::new();
    for chunk in input.as_bytes().chunks(CHUNK) {
        black_box(tokenizer.feed(black_box(chunk)));
    }
    black_box(tokenizer.finish());
}

/// Seconds per call: the best of three batches of at least 10 ms each. A
/// call slower than 250 ms is far above timer noise and is returned at once,
/// so a quadratic regression fails quickly instead of repeating slow calls.
fn seconds(mut f: impl FnMut()) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let start = Instant::now();
        let mut calls = 0u32;
        while calls == 0 || start.elapsed() < Duration::from_millis(10) {
            f();
            calls += 1;
        }
        let elapsed = start.elapsed();
        if elapsed > Duration::from_millis(250) {
            return elapsed.as_secs_f64() / f64::from(calls);
        }
        best = best.min(elapsed.as_secs_f64() / f64::from(calls));
    }
    best
}
