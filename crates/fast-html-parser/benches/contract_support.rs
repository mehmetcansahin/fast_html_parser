//! Shared correctness contract for parser comparison benchmarks.
//!
//! Keep this module independent from Criterion so the same fixture checks can
//! run as an integration test.  Benchmark binaries call these checks before
//! starting a timing loop; correctness work is therefore never timed.

#![allow(dead_code)]

use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FhpBenchOrder {
    FhpScraperTl,
    FhpTlScraper,
    ScraperFhpTl,
    ScraperTlFhp,
    TlFhpScraper,
    TlScraperFhp,
}

pub const FHP_BENCH_ORDERS: &[(&str, FhpBenchOrder)] = &[
    ("fhp-scraper-tl", FhpBenchOrder::FhpScraperTl),
    ("fhp-tl-scraper", FhpBenchOrder::FhpTlScraper),
    ("scraper-fhp-tl", FhpBenchOrder::ScraperFhpTl),
    ("scraper-tl-fhp", FhpBenchOrder::ScraperTlFhp),
    ("tl-fhp-scraper", FhpBenchOrder::TlFhpScraper),
    ("tl-scraper-fhp", FhpBenchOrder::TlScraperFhp),
];

/// Resolve the benchmark registration order used to expose order/thermal bias.
///
/// Local runs use the historical FHP/scraper/tl order. Publication runs all
/// six accepted permutations and compare their distributions.
pub fn fhp_bench_order() -> FhpBenchOrder {
    match std::env::var("FHP_BENCH_ORDER") {
        Ok(value) => parse_fhp_bench_order(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_fhp_bench_order(None),
        Err(error) => panic!("could not read FHP_BENCH_ORDER: {error}"),
    }
}

pub fn parse_fhp_bench_order(value: Option<&str>) -> FhpBenchOrder {
    match value {
        Some("fhp-scraper-tl") | None => FhpBenchOrder::FhpScraperTl,
        Some("fhp-tl-scraper") => FhpBenchOrder::FhpTlScraper,
        Some("scraper-fhp-tl") => FhpBenchOrder::ScraperFhpTl,
        Some("scraper-tl-fhp") => FhpBenchOrder::ScraperTlFhp,
        Some("tl-fhp-scraper") => FhpBenchOrder::TlFhpScraper,
        Some("tl-scraper-fhp") => FhpBenchOrder::TlScraperFhp,
        Some(value) => panic!(
            "invalid FHP_BENCH_ORDER={value:?}; expected one of: {}",
            FHP_BENCH_ORDERS
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Apply one of all six DOM parser permutations without splitting an
/// implementation block. Empty blocks are filtered after ordering, so every
/// two-parser comparison observes each binary order three times.
pub fn order_dom_parser_blocks<T>(
    fhp: Vec<T>,
    scraper: Vec<T>,
    tl: Vec<T>,
    order: FhpBenchOrder,
) -> Vec<T> {
    let blocks = match order {
        FhpBenchOrder::FhpScraperTl => [fhp, scraper, tl],
        FhpBenchOrder::FhpTlScraper => [fhp, tl, scraper],
        FhpBenchOrder::ScraperFhpTl => [scraper, fhp, tl],
        FhpBenchOrder::ScraperTlFhp => [scraper, tl, fhp],
        FhpBenchOrder::TlFhpScraper => [tl, fhp, scraper],
        FhpBenchOrder::TlScraperFhp => [tl, scraper, fhp],
    };
    blocks.into_iter().flatten().collect()
}

/// Deterministic digest of the canonical DOM contract stream.
///
/// The stream includes element names, sorted attribute name/value pairs,
/// decoded text nodes, and explicit child boundaries. Comments and doctypes
/// are deliberately outside the benchmark contract. `canonical_bytes` and
/// `node_count` make accidental changes easier to diagnose, while equality is
/// still defined over the complete digest value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalDomDigest {
    pub node_count: u64,
    pub canonical_bytes: u64,
    pub fnv1a_128: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomImplementation {
    FastHtmlParser,
    Scraper,
    Tl,
}

impl DomImplementation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FastHtmlParser => "fast_html_parser",
            Self::Scraper => "scraper",
            Self::Tl => "tl",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DigestParity {
    pub fast_html_parser_scraper: bool,
    pub fast_html_parser_tl: bool,
    pub scraper_tl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectorParity {
    pub fast_html_parser_scraper: bool,
    pub fast_html_parser_tl: bool,
    pub scraper_tl: bool,
}

impl SelectorParity {
    pub const fn all_equal(self) -> bool {
        self.fast_html_parser_scraper && self.fast_html_parser_tl && self.scraper_tl
    }
}

pub const fn intersect_selector_parity(
    fixture: &FixtureContract,
    selector: &SelectorWorkloadContract,
) -> SelectorParity {
    SelectorParity {
        fast_html_parser_scraper: fixture.parity.fast_html_parser_scraper
            && selector.parity.fast_html_parser_scraper,
        fast_html_parser_tl: fixture.parity.fast_html_parser_tl
            && selector.parity.fast_html_parser_tl,
        scraper_tl: fixture.parity.scraper_tl && selector.parity.scraper_tl,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectorWorkloadContract {
    pub fixture_id: &'static str,
    pub id: &'static str,
    pub css: &'static str,
    pub fast_html_parser_count: usize,
    pub scraper_count: usize,
    pub tl_count: usize,
    pub parity: SelectorParity,
}

impl SelectorWorkloadContract {
    pub const fn expected_count(self, implementation: DomImplementation) -> usize {
        match implementation {
            DomImplementation::FastHtmlParser => self.fast_html_parser_count,
            DomImplementation::Scraper => self.scraper_count,
            DomImplementation::Tl => self.tl_count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixtureContract {
    pub id: &'static str,
    pub fast_html_parser: CanonicalDomDigest,
    pub scraper: CanonicalDomDigest,
    pub tl: CanonicalDomDigest,
    pub parity: DigestParity,
}

impl FixtureContract {
    pub const fn expected(self, implementation: DomImplementation) -> CanonicalDomDigest {
        match implementation {
            DomImplementation::FastHtmlParser => self.fast_html_parser,
            DomImplementation::Scraper => self.scraper,
            DomImplementation::Tl => self.tl,
        }
    }
}

const CONTRACTS_JSON: &str = include_str!("../../../benchmarks/contracts.json");

struct BenchmarkContracts {
    fixtures: Vec<FixtureContract>,
    selectors: Vec<SelectorWorkloadContract>,
}

static BENCHMARK_CONTRACTS: OnceLock<BenchmarkContracts> = OnceLock::new();

fn required_object<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> &'a serde_json::Map<String, serde_json::Value> {
    object
        .get(key)
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("benchmark contract field {key:?} must be an object"))
}

fn required_array<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> &'a Vec<serde_json::Value> {
    object
        .get(key)
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("benchmark contract field {key:?} must be an array"))
}

fn required_str(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> &'static str {
    let value = object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("benchmark contract field {key:?} must be a string"));
    Box::leak(value.to_owned().into_boxed_str())
}

fn required_u64(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> u64 {
    object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("benchmark contract field {key:?} must be an unsigned integer"))
}

fn parse_digest(
    digests: &serde_json::Map<String, serde_json::Value>,
    implementation: &str,
) -> CanonicalDomDigest {
    let value = required_object(digests, implementation);
    let fnv = value
        .get("fnv1a_128")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{implementation} fnv1a_128 must be a decimal string"))
        .parse::<u128>()
        .unwrap_or_else(|error| panic!("invalid {implementation} fnv1a_128: {error}"));
    CanonicalDomDigest {
        node_count: required_u64(value, "node_count"),
        canonical_bytes: required_u64(value, "canonical_bytes"),
        fnv1a_128: fnv,
    }
}

fn benchmark_contracts() -> &'static BenchmarkContracts {
    BENCHMARK_CONTRACTS.get_or_init(|| {
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
        let canonical = required_object(root, "canonical_dom");
        let mut fixtures = Vec::new();
        for value in required_array(canonical, "fixtures") {
            let object = value
                .as_object()
                .expect("canonical_dom fixture must be an object");
            let id = required_str(object, "id");
            let digests = required_object(object, "digests");
            let fast_html_parser = parse_digest(digests, "fast_html_parser");
            let scraper = parse_digest(digests, "scraper");
            let tl = parse_digest(digests, "tl");
            fixtures.push(FixtureContract {
                id,
                fast_html_parser,
                scraper,
                tl,
                parity: DigestParity {
                    fast_html_parser_scraper: fast_html_parser == scraper,
                    fast_html_parser_tl: fast_html_parser == tl,
                    scraper_tl: scraper == tl,
                },
            });
        }

        let mut selectors = Vec::new();
        for value in required_array(root, "selectors") {
            let object = value
                .as_object()
                .expect("selector contract must be an object");
            let counts = required_object(object, "counts");
            let fast_html_parser_count = required_u64(counts, "fast_html_parser") as usize;
            let scraper_count = required_u64(counts, "scraper") as usize;
            let tl_count = required_u64(counts, "tl") as usize;
            selectors.push(SelectorWorkloadContract {
                fixture_id: required_str(object, "fixture_id"),
                id: required_str(object, "id"),
                css: required_str(object, "css"),
                fast_html_parser_count,
                scraper_count,
                tl_count,
                parity: SelectorParity {
                    fast_html_parser_scraper: fast_html_parser_count == scraper_count,
                    fast_html_parser_tl: fast_html_parser_count == tl_count,
                    scraper_tl: scraper_count == tl_count,
                },
            });
        }

        assert!(
            !fixtures.is_empty(),
            "benchmark fixture contracts cannot be empty"
        );
        assert!(
            !selectors.is_empty(),
            "benchmark selector contracts cannot be empty"
        );
        BenchmarkContracts {
            fixtures,
            selectors,
        }
    })
}

pub fn fixture_contracts() -> &'static [FixtureContract] {
    &benchmark_contracts().fixtures
}

pub fn selector_workload_contracts() -> &'static [SelectorWorkloadContract] {
    &benchmark_contracts().selectors
}

pub fn fixture_contract(id: &str) -> &'static FixtureContract {
    fixture_contracts()
        .iter()
        .find(|contract| contract.id == id)
        .unwrap_or_else(|| panic!("missing checked-in benchmark contract for fixture {id:?}"))
}

pub fn selector_workload_contract(fixture_id: &str, id: &str) -> &'static SelectorWorkloadContract {
    selector_workload_contracts()
        .iter()
        .find(|contract| contract.fixture_id == fixture_id && contract.id == id)
        .unwrap_or_else(|| {
            panic!(
                "missing checked-in selector contract for fixture {fixture_id:?}, workload {id:?}"
            )
        })
}
impl fmt::Display for CanonicalDomDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "nodes={}, canonical_bytes={}, fnv1a_128={:#034x}",
            self.node_count, self.canonical_bytes, self.fnv1a_128
        )
    }
}

/// FNV-1a offset basis for the 128-bit contract digest.
const FNV1A_128_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
/// FNV-1a prime for the 128-bit contract digest.
const FNV1A_128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

struct CanonicalDomEncoder {
    hash: u128,
    canonical_bytes: u64,
    node_count: u64,
}

impl CanonicalDomEncoder {
    fn new() -> Self {
        Self {
            hash: FNV1A_128_OFFSET,
            canonical_bytes: 0,
            node_count: 0,
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

    fn begin_document(&mut self) {
        self.node_count += 1;
        self.field(b'D', &[]);
    }

    fn end_document(&mut self) {
        self.field(b'd', &[]);
    }

    fn begin_element(&mut self, tag: &str, attrs: &mut Vec<(String, String)>) {
        self.node_count += 1;
        let tag = tag.to_ascii_lowercase();
        self.field(b'E', tag.as_bytes());

        for (name, _) in attrs.iter_mut() {
            name.make_ascii_lowercase();
        }
        attrs.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        self.field(b'#', &(attrs.len() as u64).to_le_bytes());
        for (name, value) in attrs {
            self.field(b'A', name.as_bytes());
            self.field(b'V', value.as_bytes());
        }
    }

    fn end_element(&mut self) {
        self.field(b'e', &[]);
    }

    fn text(&mut self, text: &str) {
        self.node_count += 1;
        self.field(b'T', text.as_bytes());
    }

    fn finish(self) -> CanonicalDomDigest {
        CanonicalDomDigest {
            node_count: self.node_count,
            canonical_bytes: self.canonical_bytes,
            fnv1a_128: self.hash,
        }
    }
}

#[cfg(feature = "css-selector")]
pub fn fast_html_parser_digest(html: &str) -> CanonicalDomDigest {
    let doc = fast_html_parser::HtmlParser::parse(html)
        .unwrap_or_else(|error| panic!("fast-html-parser failed contract parse: {error}"));
    fast_html_parser_document_digest(&doc)
}

#[cfg(feature = "css-selector")]
pub fn fast_html_parser_owned_digest(html: &str) -> CanonicalDomDigest {
    let doc = fast_html_parser::HtmlParser::parse_owned(html.to_owned())
        .unwrap_or_else(|error| panic!("fast-html-parser failed owned contract parse: {error}"));
    fast_html_parser_document_digest(&doc)
}

#[cfg(feature = "css-selector")]
fn fast_html_parser_document_digest(doc: &fast_html_parser::Document) -> CanonicalDomDigest {
    fn encode_node(
        doc: &fast_html_parser::Document,
        node_id: fast_html_parser::NodeId,
        encoder: &mut CanonicalDomEncoder,
    ) {
        let node = doc.get(node_id);
        if node.is_text() {
            encoder.text(node.text());
            return;
        }
        if node.is_comment() || node.is_doctype() {
            return;
        }

        let arena = doc.arena();
        let tag = node
            .tag()
            .as_str()
            .or_else(|| arena.unknown_tag_name(node.id()))
            .expect("non-root benchmark element must have a tag name");
        let mut attrs = node
            .attrs()
            .iter()
            .map(|attr| {
                (
                    arena.attr_name(attr).to_owned(),
                    arena.attr_value(attr).unwrap_or("").to_owned(),
                )
            })
            .collect::<Vec<_>>();
        encoder.begin_element(tag, &mut attrs);
        for child in node.children() {
            encode_node(doc, child, encoder);
        }
        encoder.end_element();
    }

    let mut encoder = CanonicalDomEncoder::new();
    encoder.begin_document();
    for child in doc.root().children() {
        encode_node(doc, child, &mut encoder);
    }
    encoder.end_document();
    encoder.finish()
}

pub fn scraper_digest(html: &str) -> CanonicalDomDigest {
    let doc = scraper::Html::parse_document(html);
    let mut encoder = CanonicalDomEncoder::new();
    encoder.begin_document();

    // `scraper` does not re-export ego-tree's traversal edge type. Traverse in
    // document order and keep the open element ids inferred from NodeRef::id.
    let mut open = Vec::new();
    for node in doc.tree.root().descendants().skip(1) {
        let mut ancestors = node
            .ancestors()
            .filter(|ancestor| ancestor.value().is_element())
            .map(|ancestor| ancestor.id())
            .collect::<Vec<_>>();
        ancestors.reverse();

        let shared = open
            .iter()
            .zip(&ancestors)
            .take_while(|(left, right)| left == right)
            .count();
        for _ in shared..open.len() {
            encoder.end_element();
        }
        open.truncate(shared);

        match node.value() {
            scraper::Node::Element(element) => {
                let mut attrs = element
                    .attrs
                    .iter()
                    .map(|(name, value)| {
                        let name = name.prefix.as_ref().map_or_else(
                            || name.local.to_string(),
                            |prefix| format!("{prefix}:{}", name.local),
                        );
                        (name, value.to_string())
                    })
                    .collect::<Vec<_>>();
                encoder.begin_element(element.name(), &mut attrs);
                open.push(node.id());
            }
            scraper::Node::Text(text) => encoder.text(text),
            _ => {}
        }
    }
    for _ in 0..open.len() {
        encoder.end_element();
    }
    encoder.end_document();
    encoder.finish()
}

pub fn tl_digest(html: &str) -> CanonicalDomDigest {
    let dom = tl::parse(html, tl::ParserOptions::default())
        .unwrap_or_else(|error| panic!("tl failed contract parse: {error}"));
    tl_dom_digest(&dom)
}

pub fn tl_owned_digest(html: &str) -> CanonicalDomDigest {
    // SAFETY: `VDomGuard` owns the input allocation and is kept alive while
    // its borrowed DOM is observed.
    let guard = unsafe { tl::parse_owned(html.to_owned(), tl::ParserOptions::default()) }
        .unwrap_or_else(|error| panic!("tl failed owned contract parse: {error}"));
    tl_dom_digest(guard.get_ref())
}

fn tl_dom_digest(dom: &tl::VDom<'_>) -> CanonicalDomDigest {
    fn encode_node(
        node: &tl::Node<'_>,
        parser: &tl::Parser<'_>,
        decode_text_entities: bool,
        encoder: &mut CanonicalDomEncoder,
    ) {
        match node {
            tl::Node::Tag(tag) => {
                let name = tag.name().as_utf8_str();
                let mut attrs = tag
                    .attributes()
                    .iter()
                    .map(|(name, value)| {
                        let value = value.unwrap_or_default();
                        (
                            name.into_owned(),
                            fhp_tokenizer::entity::decode_attribute_entities(&value).into_owned(),
                        )
                    })
                    .collect::<Vec<_>>();
                encoder.begin_element(&name, &mut attrs);

                let child_decode = !matches!(
                    name.as_ref(),
                    "script" | "style" | "iframe" | "xmp" | "noembed" | "noframes" | "plaintext"
                );
                for child in tag.children().top().iter() {
                    if let Some(child) = child.get(parser) {
                        encode_node(child, parser, child_decode, encoder);
                    }
                }
                encoder.end_element();
            }
            tl::Node::Raw(text) => {
                let text = text.as_utf8_str();
                if decode_text_entities {
                    let decoded = fhp_tokenizer::entity::decode_entities(&text);
                    encoder.text(&decoded);
                } else {
                    encoder.text(&text);
                }
            }
            tl::Node::Comment(_) => {}
        }
    }

    let parser = dom.parser();
    let mut encoder = CanonicalDomEncoder::new();
    encoder.begin_document();
    for handle in dom.children() {
        if let Some(node) = handle.get(parser) {
            encode_node(node, parser, true, &mut encoder);
        }
    }
    encoder.end_document();
    encoder.finish()
}

pub fn assert_digest(
    fixture: &str,
    implementation: &str,
    actual: CanonicalDomDigest,
    expected: CanonicalDomDigest,
) {
    assert_eq!(
        actual, expected,
        "benchmark contract mismatch for {implementation} on {fixture}\nactual:   {actual}\nexpected: {expected}"
    );
}

pub fn assert_fixture_contract_parity(contract: &FixtureContract) {
    assert_eq!(
        contract.parity.fast_html_parser_scraper,
        contract.fast_html_parser == contract.scraper,
        "stale fast-html-parser/scraper parity for {}",
        contract.id
    );
    assert_eq!(
        contract.parity.fast_html_parser_tl,
        contract.fast_html_parser == contract.tl,
        "stale fast-html-parser/tl parity for {}",
        contract.id
    );
    assert_eq!(
        contract.parity.scraper_tl,
        contract.scraper == contract.tl,
        "stale scraper/tl parity for {}",
        contract.id
    );
}

pub fn assert_selector_contract_parity(contract: &SelectorWorkloadContract) {
    assert_eq!(
        contract.parity.fast_html_parser_scraper,
        contract.fast_html_parser_count == contract.scraper_count,
        "stale selector FHP/scraper parity for {}/{}",
        contract.fixture_id,
        contract.id
    );
    assert_eq!(
        contract.parity.fast_html_parser_tl,
        contract.fast_html_parser_count == contract.tl_count,
        "stale selector FHP/tl parity for {}/{}",
        contract.fixture_id,
        contract.id
    );
    assert_eq!(
        contract.parity.scraper_tl,
        contract.scraper_count == contract.tl_count,
        "stale selector scraper/tl parity for {}/{}",
        contract.fixture_id,
        contract.id
    );
}

#[cfg(feature = "css-selector")]
pub fn selector_workload_counts(html: &str, css: &str) -> [(DomImplementation, usize); 3] {
    use fast_html_parser::Selectable;

    let fhp_doc = fast_html_parser::HtmlParser::parse(html).unwrap();
    let fhp_selector = fast_html_parser::CompiledSelector::new(css).unwrap();
    let scraper_doc = scraper::Html::parse_document(html);
    let scraper_selector = scraper::Selector::parse(css).unwrap();
    let tl_dom = tl::parse(html, tl::ParserOptions::default()).unwrap();
    let tl_selector = tl::parse_query_selector(css).expect("valid benchmark selector");

    [
        (
            DomImplementation::FastHtmlParser,
            fhp_doc.select_compiled(&fhp_selector).unwrap().len(),
        ),
        (
            DomImplementation::Scraper,
            scraper_doc
                .select(&scraper_selector)
                .collect::<Vec<_>>()
                .len(),
        ),
        (
            DomImplementation::Tl,
            tl::queryselector::QuerySelectorIterator::new(tl_selector, tl_dom.parser(), &tl_dom)
                .collect::<Vec<_>>()
                .len(),
        ),
    ]
}

pub fn assert_selector_workload_counts(
    contract: &SelectorWorkloadContract,
    actual: &[(DomImplementation, usize)],
) {
    assert_selector_contract_parity(contract);
    for implementation in [
        DomImplementation::FastHtmlParser,
        DomImplementation::Scraper,
        DomImplementation::Tl,
    ] {
        let actual_count = actual
            .iter()
            .find(|(candidate, _)| *candidate == implementation)
            .unwrap_or_else(|| panic!("missing {} selector count", implementation.as_str()))
            .1;
        assert_eq!(
            actual_count,
            contract.expected_count(implementation),
            "selector benchmark contract mismatch for {}/{}/{} ({:?})",
            contract.fixture_id,
            contract.id,
            implementation.as_str(),
            contract.css
        );
    }
}

#[cfg(feature = "css-selector")]
pub fn validate_selector_workload_contract(contract: &SelectorWorkloadContract, html: &str) {
    let actual = selector_workload_counts(html, contract.css);
    assert_selector_workload_counts(contract, &actual);
}

#[cfg(feature = "css-selector")]
pub fn validate_fixture_contract(contract: &FixtureContract, html: &str) {
    assert_fixture_contract_parity(contract);
    for (implementation, actual) in [
        (
            DomImplementation::FastHtmlParser,
            fast_html_parser_digest(html),
        ),
        (DomImplementation::Scraper, scraper_digest(html)),
        (DomImplementation::Tl, tl_digest(html)),
    ] {
        assert_digest(
            contract.id,
            implementation.as_str(),
            actual,
            contract.expected(implementation),
        );
    }

    assert_digest(
        contract.id,
        "fast_html_parser_owned",
        fast_html_parser_owned_digest(html),
        contract.fast_html_parser,
    );
    assert_digest(contract.id, "tl_owned", tl_owned_digest(html), contract.tl);
}

/// Validate the streaming comparison's contract: a no-op rewrite must invoke
/// the element handler and reproduce the fixture byte-for-byte.
pub fn validate_lol_html_passthrough(fixture: &str, html: &str) {
    use std::cell::Cell;

    let element_count = Cell::new(0usize);
    let mut output = Vec::with_capacity(html.len());
    {
        let mut rewriter = lol_html::HtmlRewriter::new(
            lol_html::Settings {
                element_content_handlers: vec![lol_html::element!("*", |_element| {
                    element_count.set(element_count.get() + 1);
                    Ok(())
                })],
                ..lol_html::Settings::new()
            },
            |chunk: &[u8]| output.extend_from_slice(chunk),
        );
        rewriter
            .write(html.as_bytes())
            .unwrap_or_else(|error| panic!("lol_html failed contract write on {fixture}: {error}"));
        rewriter
            .end()
            .unwrap_or_else(|error| panic!("lol_html failed contract end on {fixture}: {error}"));
    }

    assert!(
        element_count.get() > 0,
        "lol_html contract did not observe any elements on {fixture}"
    );
    assert_eq!(
        output,
        html.as_bytes(),
        "lol_html no-op rewrite changed fixture bytes on {fixture}"
    );
}
