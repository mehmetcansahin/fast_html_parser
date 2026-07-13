//! Shared correctness contract for parser comparison benchmarks.
//!
//! Keep this module independent from Criterion so the same fixture checks can
//! run as an integration test.  Benchmark binaries call these checks before
//! starting a timing loop; correctness work is therefore never timed.

#![allow(dead_code)]

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FhpBenchOrder {
    First,
    Middle,
    Last,
}

/// Resolve the benchmark registration order used to expose order/thermal bias.
///
/// Local runs default to the middle position.  Automation can run all three
/// accepted values and compare their distributions.
pub fn fhp_bench_order() -> FhpBenchOrder {
    match std::env::var("FHP_BENCH_ORDER") {
        Ok(value) => parse_fhp_bench_order(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_fhp_bench_order(None),
        Err(error) => panic!("could not read FHP_BENCH_ORDER: {error}"),
    }
}

pub fn parse_fhp_bench_order(value: Option<&str>) -> FhpBenchOrder {
    match value {
        Some("fhp-first") => FhpBenchOrder::First,
        Some("fhp-middle") | None => FhpBenchOrder::Middle,
        Some("fhp-last") => FhpBenchOrder::Last,
        Some(value) => panic!(
            "invalid FHP_BENCH_ORDER={value:?}; expected one of: fhp-first, fhp-middle, fhp-last"
        ),
    }
}

/// Insert the fast-html-parser implementation block among other implementation
/// blocks, then flatten without splitting any block.
///
/// `Middle` uses the lower-middle insertion point. With only one other
/// implementation there is no distinct middle position, so it intentionally
/// produces the same order as `First`.
pub fn order_fhp_blocks<T>(fhp: Vec<T>, mut others: Vec<Vec<T>>, order: FhpBenchOrder) -> Vec<T> {
    if fhp.is_empty() {
        return others.into_iter().flatten().collect();
    }

    let insertion = match order {
        FhpBenchOrder::First => 0,
        FhpBenchOrder::Middle => others.len() / 2,
        FhpBenchOrder::Last => others.len(),
    };
    others.insert(insertion, fhp);
    others.into_iter().flatten().collect()
}

/// Small, portable observables shared by the DOM implementations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservableSignature {
    pub paragraph_count: usize,
    pub link_with_href_count: usize,
    pub div_count: usize,
    pub normalized_text_len: usize,
    pub normalized_text_fnv1a: u64,
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
pub struct SignatureParity {
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
    pub fast_html_parser: ObservableSignature,
    pub scraper: ObservableSignature,
    pub tl: ObservableSignature,
    pub parity: SignatureParity,
}

impl FixtureContract {
    pub const fn expected(self, implementation: DomImplementation) -> ObservableSignature {
        match implementation {
            DomImplementation::FastHtmlParser => self.fast_html_parser,
            DomImplementation::Scraper => self.scraper,
            DomImplementation::Tl => self.tl,
        }
    }
}

const fn signature(
    paragraph_count: usize,
    link_with_href_count: usize,
    div_count: usize,
    normalized_text_len: usize,
    normalized_text_fnv1a: u64,
) -> ObservableSignature {
    ObservableSignature {
        paragraph_count,
        link_with_href_count,
        div_count,
        normalized_text_len,
        normalized_text_fnv1a,
    }
}

/// Checked-in observable contracts for every fixture used in published
/// synthetic or real-world comparisons.
pub const FIXTURE_CONTRACTS: &[FixtureContract] = &[
    FixtureContract {
        id: "synthetic_1kb",
        fast_html_parser: signature(1, 2, 3, 297, 0x6068_0eac_279d_ed2a),
        scraper: signature(1, 2, 3, 297, 0x6068_0eac_279d_ed2a),
        tl: signature(1, 2, 3, 297, 0x6068_0eac_279d_ed2a),
        parity: SignatureParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: true,
            scraper_tl: true,
        },
    },
    FixtureContract {
        id: "synthetic_100kb",
        fast_html_parser: signature(21, 171, 87, 25_181, 0x4bdc_7b80_0bcc_b4d5),
        scraper: signature(21, 171, 87, 24_341, 0x3ccd_e5fe_fa53_af9b),
        tl: signature(21, 171, 87, 25_181, 0x4bdc_7b80_0bcc_b4d5),
        parity: SignatureParity {
            fast_html_parser_scraper: false,
            fast_html_parser_tl: true,
            scraper_tl: false,
        },
    },
    FixtureContract {
        id: "synthetic_5mb",
        fast_html_parser: signature(1_035, 4_228, 4_372, 1_217_079, 0x3911_df29_c108_0a8c),
        scraper: signature(1_035, 4_228, 4_372, 1_138_859, 0xe884_1412_2f0c_5922),
        tl: signature(1_035, 4_228, 4_372, 1_217_079, 0x3911_df29_c108_0a8c),
        parity: SignatureParity {
            fast_html_parser_scraper: false,
            fast_html_parser_tl: true,
            scraper_tl: false,
        },
    },
    FixtureContract {
        id: "realworld_hackernews_34kb",
        fast_html_parser: signature(0, 226, 29, 3_716, 0x7584_70fc_a596_3323),
        scraper: signature(0, 226, 29, 3_716, 0x7584_70fc_a596_3323),
        tl: signature(0, 226, 29, 3_716, 0x7584_70fc_a596_3323),
        parity: SignatureParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: true,
            scraper_tl: true,
        },
    },
    FixtureContract {
        id: "realworld_github_301kb",
        fast_html_parser: signature(3, 130, 141, 9_058, 0xd7cb_84b9_892e_4c49),
        scraper: signature(3, 130, 141, 9_058, 0xd7cb_84b9_892e_4c49),
        tl: signature(3, 130, 141, 9_268, 0xc81b_5b57_a069_7a99),
        parity: SignatureParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: false,
            scraper_tl: false,
        },
    },
    FixtureContract {
        id: "realworld_stackoverflow_415kb",
        fast_html_parser: signature(9, 403, 939, 75_839, 0x047d_3a87_4eb7_c0e3),
        scraper: signature(9, 403, 939, 75_854, 0xf011_2962_54f8_807e),
        tl: signature(9, 402, 939, 68_612, 0xe873_4c69_73e8_487a),
        parity: SignatureParity {
            fast_html_parser_scraper: false,
            fast_html_parser_tl: false,
            scraper_tl: false,
        },
    },
    FixtureContract {
        id: "realworld_wikipedia_590kb",
        fast_html_parser: signature(115, 1_908, 357, 106_232, 0xa79a_55f1_844a_6645),
        scraper: signature(115, 1_908, 357, 106_420, 0x6c96_5a2e_a463_fc8f),
        tl: signature(115, 1_905, 357, 106_232, 0xa79a_55f1_844a_6645),
        parity: SignatureParity {
            fast_html_parser_scraper: false,
            fast_html_parser_tl: false,
            scraper_tl: false,
        },
    },
];

/// Checked-in result counts for every published selector workload. These are
/// separate from whole-document signatures because selector support can differ
/// even when the basic fixture observables are identical.
pub const SELECTOR_WORKLOAD_CONTRACTS: &[SelectorWorkloadContract] = &[
    SelectorWorkloadContract {
        fixture_id: "synthetic_100kb",
        id: "tag_p",
        css: "p",
        fast_html_parser_count: 21,
        scraper_count: 21,
        tl_count: 21,
        parity: SelectorParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: true,
            scraper_tl: true,
        },
    },
    SelectorWorkloadContract {
        fixture_id: "synthetic_100kb",
        id: "class_card",
        css: ".card",
        fast_html_parser_count: 20,
        scraper_count: 20,
        tl_count: 20,
        parity: SelectorParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: true,
            scraper_tl: true,
        },
    },
    SelectorWorkloadContract {
        fixture_id: "synthetic_100kb",
        id: "descendant_div_p",
        css: "div p",
        fast_html_parser_count: 21,
        scraper_count: 21,
        tl_count: 0,
        parity: SelectorParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: false,
            scraper_tl: false,
        },
    },
    SelectorWorkloadContract {
        fixture_id: "realworld_wikipedia_590kb",
        id: "link_with_href",
        css: "a[href]",
        fast_html_parser_count: 1_908,
        scraper_count: 1_908,
        tl_count: 1_905,
        parity: SelectorParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: false,
            scraper_tl: false,
        },
    },
    SelectorWorkloadContract {
        fixture_id: "realworld_wikipedia_590kb",
        id: "class_mw_body",
        css: "main.mw-body",
        fast_html_parser_count: 1,
        scraper_count: 1,
        tl_count: 1,
        parity: SelectorParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: true,
            scraper_tl: true,
        },
    },
    SelectorWorkloadContract {
        fixture_id: "realworld_wikipedia_590kb",
        id: "descendant_table_td",
        css: "table td",
        fast_html_parser_count: 60,
        scraper_count: 60,
        tl_count: 0,
        parity: SelectorParity {
            fast_html_parser_scraper: true,
            fast_html_parser_tl: false,
            scraper_tl: false,
        },
    },
];

pub fn fixture_contract(id: &str) -> &'static FixtureContract {
    FIXTURE_CONTRACTS
        .iter()
        .find(|contract| contract.id == id)
        .unwrap_or_else(|| panic!("missing checked-in benchmark contract for fixture {id:?}"))
}

pub fn selector_workload_contract(fixture_id: &str, id: &str) -> &'static SelectorWorkloadContract {
    SELECTOR_WORKLOAD_CONTRACTS
        .iter()
        .find(|contract| contract.fixture_id == fixture_id && contract.id == id)
        .unwrap_or_else(|| {
            panic!(
                "missing checked-in selector contract for fixture {fixture_id:?}, workload {id:?}"
            )
        })
}

impl fmt::Display for ObservableSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "p={}, a[href]={}, div={}, text_len={}, text_fnv1a={:#018x}",
            self.paragraph_count,
            self.link_with_href_count,
            self.div_count,
            self.normalized_text_len,
            self.normalized_text_fnv1a
        )
    }
}

/// FNV-1a offset basis for the 64-bit contract hash.
const FNV1A_64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime for the 64-bit contract hash.
const FNV1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV1A_64_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV1A_64_PRIME)
    })
}

/// Normalize parser text without erasing word order.
///
/// Unicode whitespace runs are collapsed to a single ASCII space. Entity
/// decoding is intentionally not done here: FHP and scraper already expose
/// decoded text, while the `tl` adapter decodes its source text exactly once.
pub fn normalized_text_signature(text: &str) -> (usize, u64) {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(ch);
        }
    }

    (normalized.len(), fnv1a_64(normalized.as_bytes()))
}

#[cfg(feature = "css-selector")]
pub fn fast_html_parser_signature(html: &str) -> ObservableSignature {
    let doc = fast_html_parser::HtmlParser::parse(html)
        .unwrap_or_else(|error| panic!("fast-html-parser failed contract parse: {error}"));
    fast_html_parser_document_signature(&doc)
}

#[cfg(feature = "css-selector")]
pub fn fast_html_parser_owned_signature(html: &str) -> ObservableSignature {
    let doc = fast_html_parser::HtmlParser::parse_owned(html.to_owned())
        .unwrap_or_else(|error| panic!("fast-html-parser failed owned contract parse: {error}"));
    fast_html_parser_document_signature(&doc)
}

#[cfg(feature = "css-selector")]
fn fast_html_parser_document_signature(doc: &fast_html_parser::Document) -> ObservableSignature {
    use fast_html_parser::Selectable;

    let text = doc.root().text_content();
    let (normalized_text_len, normalized_text_fnv1a) = normalized_text_signature(&text);

    ObservableSignature {
        paragraph_count: doc.select("p").expect("valid contract selector").len(),
        link_with_href_count: doc
            .select("a[href]")
            .expect("valid contract selector")
            .len(),
        div_count: doc.select("div").expect("valid contract selector").len(),
        normalized_text_len,
        normalized_text_fnv1a,
    }
}

pub fn scraper_signature(html: &str) -> ObservableSignature {
    let doc = scraper::Html::parse_document(html);
    let paragraph = scraper::Selector::parse("p").expect("valid contract selector");
    let link_with_href = scraper::Selector::parse("a[href]").expect("valid contract selector");
    let div = scraper::Selector::parse("div").expect("valid contract selector");
    let text = doc.root_element().text().collect::<String>();
    let (normalized_text_len, normalized_text_fnv1a) = normalized_text_signature(&text);

    ObservableSignature {
        paragraph_count: doc.select(&paragraph).count(),
        link_with_href_count: doc.select(&link_with_href).count(),
        div_count: doc.select(&div).count(),
        normalized_text_len,
        normalized_text_fnv1a,
    }
}

pub fn tl_signature(html: &str) -> ObservableSignature {
    let dom = tl::parse(html, tl::ParserOptions::default())
        .unwrap_or_else(|error| panic!("tl failed contract parse: {error}"));
    tl_dom_signature(&dom)
}

pub fn tl_owned_signature(html: &str) -> ObservableSignature {
    // SAFETY: `VDomGuard` owns the input allocation and is kept alive while
    // its borrowed DOM is observed.
    let guard = unsafe { tl::parse_owned(html.to_owned(), tl::ParserOptions::default()) }
        .unwrap_or_else(|error| panic!("tl failed owned contract parse: {error}"));
    tl_dom_signature(guard.get_ref())
}

fn tl_dom_signature(dom: &tl::VDom<'_>) -> ObservableSignature {
    let parser = dom.parser();
    let mut text = String::new();
    for handle in dom.children() {
        if let Some(node) = handle.get(parser) {
            text.push_str(&node.inner_text(parser));
        }
    }
    let decoded = fhp_tokenizer::entity::decode_entities(&text);
    let (normalized_text_len, normalized_text_fnv1a) = normalized_text_signature(&decoded);

    ObservableSignature {
        paragraph_count: dom
            .query_selector("p")
            .expect("valid contract selector")
            .count(),
        link_with_href_count: dom
            .query_selector("a[href]")
            .expect("valid contract selector")
            .count(),
        div_count: dom
            .query_selector("div")
            .expect("valid contract selector")
            .count(),
        normalized_text_len,
        normalized_text_fnv1a,
    }
}

pub fn assert_signature(
    fixture: &str,
    implementation: &str,
    actual: ObservableSignature,
    expected: ObservableSignature,
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
            fast_html_parser_signature(html),
        ),
        (DomImplementation::Scraper, scraper_signature(html)),
        (DomImplementation::Tl, tl_signature(html)),
    ] {
        assert_signature(
            contract.id,
            implementation.as_str(),
            actual,
            contract.expected(implementation),
        );
    }

    assert_signature(
        contract.id,
        "fast_html_parser_owned",
        fast_html_parser_owned_signature(html),
        contract.fast_html_parser,
    );
    assert_signature(
        contract.id,
        "tl_owned",
        tl_owned_signature(html),
        contract.tl,
    );
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
