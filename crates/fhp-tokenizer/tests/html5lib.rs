//! WHATWG tokenizer conformance against the vendored html5lib-tests suite
//! (`vendor/html5lib-tests`).
//!
//! The suite describes a fully conforming tokenizer, while this crate
//! implements the bounded contract in `COMPATIBILITY.md`. Runs that fail for
//! known reasons are listed in `tests/html5lib/expected-failures.txt`. The test
//! fails on any unlisted failure and on any listed run that now passes, so the
//! list always matches current behavior. Rewrite it with
//! `FHP_HTML5LIB_BLESS=1 cargo test -p fhp-tokenizer --test html5lib` and review
//! the diff.
//!
//! The suite's token format is matched as follows:
//!
//! - the harness applies input-stream preprocessing (CR and CRLF become LF),
//!   which the suite specifies as happening before tokenization;
//! - tag and attribute names are compared ASCII-lowercased, because the
//!   tokenizer keeps their source case;
//! - a DOCTYPE is compared by name only, because the tokenizer does not split
//!   public and system identifiers out of the doctype content;
//! - CDATA sections stay a separate `CDATA` token kind and never match;
//! - an initial state is entered by prefixing the start tag that switches to
//!   it. Runs that no start tag reproduces exactly (a text state whose
//!   appropriate end tag no start tag of that kind names, such as RCDATA
//!   ending at `</xmp>`; the CDATA section state; or input with a lone
//!   surrogate) are skipped and counted.
//!
//! Every input is also run through the differential oracle.

#![cfg(feature = "entity-decode")]

mod oracle;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fhp_core::is_html_whitespace;
use fhp_tokenizer::token::Token;
use fhp_tokenizer::tokenize;
use oracle::Chunking;
use serde_json::{Map, Value, json};

const SUITE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/html5lib-tests/tokenizer"
);
const EXPECTED_FAILURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/html5lib/expected-failures.txt"
);

/// Failures reported in full when the expectations do not match.
const MAX_REPORTED_FAILURES: usize = 25;

/// Start tags that enter each text state, preferred first.
const RCDATA_TAGS: &[&str] = &["title", "textarea"];
const RAWTEXT_TAGS: &[&str] = &["style", "xmp", "iframe", "noembed", "noframes"];
const SCRIPT_TAGS: &[&str] = &["script"];
const PLAINTEXT_TAGS: &[&str] = &["plaintext"];

/// One test from a suite file.
struct Case {
    /// `file#index`, stable for a pinned suite.
    id: String,
    description: String,
    input: String,
    output: Vec<Value>,
    initial_states: Vec<String>,
    last_start_tag: Option<String>,
}

enum Outcome {
    Pass,
    Fail {
        category: &'static str,
        detail: String,
    },
    Skip(&'static str),
}

/// A failing run, as recorded in the expectations file.
struct Failure {
    category: &'static str,
    description: String,
    detail: String,
}

#[test]
fn html5lib_tokenizer_suite() {
    let (cases, mut skipped) = load_cases();
    assert!(
        !cases.is_empty(),
        "no html5lib tokenizer tests found in {SUITE}"
    );

    let mut passed = 0usize;
    let mut failures = BTreeMap::new();
    for (index, case) in cases.iter().enumerate() {
        oracle::check(&case.input, Chunking::Random(index as u64), index);
        for state in &case.initial_states {
            let run_id = format!("{}@{}", case.id, state_label(state));
            match run(case, state) {
                Outcome::Pass => passed += 1,
                Outcome::Fail { category, detail } => {
                    let failure = Failure {
                        category,
                        description: case.description.clone(),
                        detail,
                    };
                    failures.insert(run_id, failure);
                }
                Outcome::Skip(reason) => *skipped.entry(reason).or_default() += 1,
            }
        }
    }

    let mut categories = BTreeMap::<&str, usize>::new();
    for failure in failures.values() {
        *categories.entry(failure.category).or_default() += 1;
    }
    println!(
        "html5lib tokenizer: {passed} passed, {} failed, {} skipped",
        failures.len(),
        skipped.values().sum::<usize>()
    );
    for (category, count) in &categories {
        println!("  failed {count}: {category}");
    }
    for (reason, count) in &skipped {
        println!("  skipped {count}: {reason}");
    }

    if std::env::var_os("FHP_HTML5LIB_BLESS").is_some() {
        write_expected_failures(&failures);
        return;
    }

    // An unclassified failure needs a `classify` rule even when listed, so
    // every accepted divergence has a documented reason.
    let expected = read_expected_failures();
    let new: Vec<_> = failures
        .iter()
        .filter(|(id, failure)| !expected.contains(*id) || failure.category == "unclassified")
        .collect();
    let fixed: Vec<_> = expected
        .iter()
        .filter(|id| !failures.contains_key(*id))
        .collect();
    if new.is_empty() && fixed.is_empty() {
        return;
    }

    let mut report = String::new();
    if !new.is_empty() {
        let _ = writeln!(
            report,
            "{} html5lib runs newly fail or have no classified reason:",
            new.len()
        );
        for (id, failure) in new.iter().take(MAX_REPORTED_FAILURES) {
            let _ = writeln!(
                report,
                "\n{id} [{}]: {}\n{}",
                failure.category, failure.description, failure.detail
            );
        }
    }
    if !fixed.is_empty() {
        let _ = writeln!(
            report,
            "\n{} html5lib runs listed as expected failures now pass:",
            fixed.len()
        );
        for id in fixed.iter().take(MAX_REPORTED_FAILURES) {
            let _ = writeln!(report, "  {id}");
        }
    }
    let _ = write!(
        report,
        "\nIf these changes are intended, re-bless the expectations with \
         FHP_HTML5LIB_BLESS=1 and review the diff."
    );
    panic!("{report}");
}

/// Run `case` from `state`.
fn run(case: &Case, state: &str) -> Outcome {
    let input = preprocess(&case.input);
    let prefix = match enter_state(state, case.last_start_tag.as_deref(), &input) {
        Ok(prefix) => prefix,
        Err(reason) => return Outcome::Skip(reason),
    };

    let html = match prefix {
        Some(tag) => format!("<{tag}>{input}"),
        None => input,
    };
    let mut tokens = tokenize(&html);
    if let Some(tag) = prefix {
        match tokens.first() {
            Some(Token::OpenTag { name, .. }) if name == tag => {
                tokens.remove(0);
            }
            _ => {
                return Outcome::Fail {
                    category: "unclassified",
                    detail: format!("  prefix <{tag}> did not produce its start tag"),
                };
            }
        }
    }

    let actual = coalesce(tokens.iter().map(to_html5lib));
    let expected = coalesce(case.output.iter().map(project_expected));
    if actual == expected {
        return Outcome::Pass;
    }
    Outcome::Fail {
        category: classify(&case.input, &expected, &actual),
        detail: format!(
            "  input    {:?}\n  expected {}\n  actual   {}",
            case.input,
            Value::Array(expected),
            Value::Array(actual)
        ),
    }
}

/// The documented divergence a failing run shows, or `unclassified`.
fn classify(input: &str, expected: &[Value], actual: &[Value]) -> &'static str {
    let is_cdata = |token: &Value| token.get(0).is_some_and(|kind| *kind == "CDATA");
    if actual.iter().any(is_cdata) {
        return "cdata-outside-foreign-content";
    }
    let mut replaced = Value::Array(actual.to_vec());
    replace_nul(&mut replaced);
    if replaced.as_array().is_some_and(|tokens| tokens == expected) {
        return "no-nul-replacement";
    }
    if input.contains("<?") {
        return "processing-instruction";
    }
    "unclassified"
}

/// Replace U+0000 with U+FFFD in every string, as the spec's tokenizer does
/// outside the data state.
fn replace_nul(value: &mut Value) {
    match value {
        Value::String(text) => *text = text.replace('\0', "\u{fffd}"),
        Value::Array(items) => items.iter_mut().for_each(replace_nul),
        Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut item) in entries {
                replace_nul(&mut item);
                map.insert(key.replace('\0', "\u{fffd}"), item);
            }
        }
        _ => {}
    }
}

/// The start tag to prefix so the tokenizer starts in `state` with
/// `last_start_tag` as the appropriate end tag, or why none does exactly.
fn enter_state(
    state: &str,
    last_start_tag: Option<&str>,
    input: &str,
) -> Result<Option<&'static str>, &'static str> {
    let tags = match state {
        "Data state" => return Ok(None),
        "PLAINTEXT state" => return Ok(Some(PLAINTEXT_TAGS[0])),
        "RCDATA state" => RCDATA_TAGS,
        "RAWTEXT state" => RAWTEXT_TAGS,
        "Script data state" => SCRIPT_TAGS,
        "CDATA section state" => return Err("CDATA section state (no foreign content)"),
        _ => return Err("unknown initial state"),
    };
    if let Some(last) = last_start_tag {
        if let Some(&tag) = tags.iter().find(|&&tag| tag == last) {
            return Ok(Some(tag));
        }
    }

    // Another start tag of the same kind is exact as long as the input names
    // neither its end tag nor the appropriate one.
    let lower = input.to_ascii_lowercase();
    let names_end_tag = |name: &str| lower.contains(&format!("</{name}"));
    if last_start_tag.is_some_and(names_end_tag) || names_end_tag(tags[0]) {
        return Err("text state whose appropriate end tag no start tag reproduces");
    }
    Ok(Some(tags[0]))
}

/// Input-stream preprocessing: normalize newlines.
fn preprocess(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

/// A tokenizer token in the suite's format.
fn to_html5lib(token: &Token<'_>) -> Value {
    match token {
        Token::OpenTag {
            name,
            attributes,
            self_closing,
            ..
        } => {
            let mut attrs = Map::new();
            for attr in attributes {
                let value = attr.value.as_deref().unwrap_or_default();
                attrs
                    .entry(attr.name.to_ascii_lowercase())
                    .or_insert_with(|| json!(value));
            }
            let mut tag = vec![
                json!("StartTag"),
                json!(name.to_ascii_lowercase()),
                Value::Object(attrs),
            ];
            if *self_closing {
                tag.push(json!(true));
            }
            Value::Array(tag)
        }
        Token::CloseTag { name, .. } => json!(["EndTag", name.to_ascii_lowercase()]),
        Token::Text { content } => json!(["Character", content]),
        Token::Comment { content } => json!(["Comment", content]),
        Token::Doctype { content } => {
            let name = content
                .split(|ch: char| ch.is_ascii() && is_html_whitespace(ch as u8))
                .find(|part| !part.is_empty())
                .map(str::to_ascii_lowercase);
            json!(["DOCTYPE", name])
        }
        Token::CData { content } => json!(["CDATA", content]),
    }
}

/// An expected token, with a DOCTYPE reduced to its name.
fn project_expected(token: &Value) -> Value {
    match token.as_array().map(Vec::as_slice) {
        Some([kind, name, ..]) if *kind == "DOCTYPE" => json!(["DOCTYPE", name]),
        _ => token.clone(),
    }
}

/// Merge adjacent `Character` tokens.
fn coalesce(tokens: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for token in tokens {
        if let (Some(text), Some(previous)) = (
            character_data(&token),
            merged.last_mut().and_then(character_data_mut),
        ) {
            previous.push_str(text);
            continue;
        }
        merged.push(token);
    }
    merged
}

fn character_data(token: &Value) -> Option<&str> {
    match token.as_array()?.as_slice() {
        [kind, Value::String(data)] if *kind == "Character" => Some(data),
        _ => None,
    }
}

fn character_data_mut(token: &mut Value) -> Option<&mut String> {
    match token.as_array_mut()?.as_mut_slice() {
        [kind, Value::String(data)] if *kind == "Character" => Some(data),
        _ => None,
    }
}

/// Load every test, returning the cases and the counts of tests skipped
/// before they could run.
fn load_cases() -> (Vec<Case>, BTreeMap<&'static str, usize>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(SUITE)
        .unwrap_or_else(|error| panic!("cannot read {SUITE}: {error}"))
        .map(|entry| entry.expect("readable suite entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "test"))
        .collect();
    files.sort();

    let mut cases = Vec::new();
    let mut skipped = BTreeMap::new();
    for path in &files {
        let file = file_name(path);
        let suite: Value = serde_json::from_str(
            &std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{file}: {error}")),
        )
        .unwrap_or_else(|error| panic!("{file}: invalid JSON: {error}"));
        // `xmlViolation.test` uses `xmlViolationTests` and expects the
        // infoset coercions, which a tokenizer does not apply.
        let Some(tests) = suite.get("tests").and_then(Value::as_array) else {
            continue;
        };
        for (index, test) in tests.iter().enumerate() {
            match parse_case(&format!("{file}#{index}"), test) {
                Some(case) => cases.push(case),
                None => {
                    *skipped
                        .entry("input contains a lone surrogate")
                        .or_default() += 1
                }
            }
        }
    }
    (cases, skipped)
}

/// Parse one test, or `None` when its input contains a lone surrogate.
fn parse_case(id: &str, test: &Value) -> Option<Case> {
    let field = |name: &str| {
        test.get(name)
            .unwrap_or_else(|| panic!("{id}: missing `{name}`"))
    };
    let double_escaped = test.get("doubleEscaped") == Some(&Value::Bool(true));
    let mut output = field("output").clone();
    let mut input = field("input").as_str().expect("string input").to_owned();
    if double_escaped {
        input = unescape(&input)?;
        unescape_strings(&mut output)?;
    }
    let initial_states = match test.get("initialStates") {
        Some(states) => states
            .as_array()
            .expect("initialStates is an array")
            .iter()
            .map(|state| state.as_str().expect("state name").to_owned())
            .collect(),
        None => vec!["Data state".to_owned()],
    };
    Some(Case {
        id: id.to_owned(),
        description: field("description").as_str().unwrap_or_default().to_owned(),
        input,
        output: output.as_array().expect("output is an array").clone(),
        initial_states,
        last_start_tag: test
            .get("lastStartTag")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// Undo the suite's second escaping round (`\uHHHH`), or `None` for a lone
/// surrogate, which a Rust string cannot hold.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\\u") {
        out.push_str(&rest[..at]);
        let unit = u32::from_str_radix(rest.get(at + 2..at + 6)?, 16).ok()?;
        rest = &rest[at + 6..];
        let code = if (0xd800..0xdc00).contains(&unit) {
            let low = rest
                .strip_prefix("\\u")
                .and_then(|tail| tail.get(..4))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .filter(|low| (0xdc00..0xe000).contains(low))?;
            rest = &rest[6..];
            0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00)
        } else {
            unit
        };
        out.push(char::from_u32(code)?);
    }
    out.push_str(rest);
    Some(out)
}

fn unescape_strings(value: &mut Value) -> Option<()> {
    match value {
        Value::String(text) => *text = unescape(text)?,
        Value::Array(items) => {
            for item in items {
                unescape_strings(item)?;
            }
        }
        Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut item) in entries {
                unescape_strings(&mut item)?;
                map.insert(unescape(&key)?, item);
            }
        }
        _ => {}
    }
    Some(())
}

/// `Data state` → `Data`, `Script data state` → `Script_data`.
fn state_label(state: &str) -> String {
    state.trim_end_matches(" state").replace(' ', "_")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .expect("file name")
        .to_string_lossy()
        .into_owned()
}

fn read_expected_failures() -> BTreeSet<String> {
    let text = std::fs::read_to_string(EXPECTED_FAILURES)
        .unwrap_or_else(|error| panic!("cannot read {EXPECTED_FAILURES}: {error}"));
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split_whitespace()
                .next()
                .expect("non-empty line")
                .to_owned()
        })
        .collect()
}

fn write_expected_failures(failures: &BTreeMap<String, Failure>) {
    let mut text = String::from(
        "# html5lib tokenizer runs that fail for known reasons; see tests/html5lib.rs.\n\
         # Regenerate with: FHP_HTML5LIB_BLESS=1 cargo test -p fhp-tokenizer --test html5lib\n\
         # Format: <file>#<index>@<initial state>  [<category>]  <description>\n\
         #\n\
         # Categories (see COMPATIBILITY.md):\n\
         #   cdata-outside-foreign-content  `<![CDATA[` is a CDATA section in all content\n\
         #   no-nul-replacement             U+0000 is kept where the spec emits U+FFFD\n\
         #   processing-instruction         `<?` starts a bogus comment, not a PI\n\
         #   unclassified                   needs a reason before it is accepted\n",
    );
    for (id, failure) in failures {
        let _ = writeln!(
            text,
            "{id}  [{}]  {:?}",
            failure.category, failure.description
        );
    }
    std::fs::write(EXPECTED_FAILURES, text)
        .unwrap_or_else(|error| panic!("cannot write {EXPECTED_FAILURES}: {error}"));
}
