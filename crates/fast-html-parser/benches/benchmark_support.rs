//! Criterion registration shared by synthetic and real-world comparisons.

#[cfg(feature = "css-selector")]
use crate::contract_support::{
    DomImplementation, SelectorWorkloadContract, assert_selector_workload_counts, fixture_contract,
    intersect_selector_parity, selector_workload_contract,
};
use crate::contract_support::{FhpBenchOrder, FixtureContract, order_dom_parser_blocks};
use criterion::measurement::WallTime;
use criterion::{BatchSize, BenchmarkGroup, BenchmarkId, Criterion, Throughput};

type Group<'a> = BenchmarkGroup<'a, WallTime>;

#[derive(Clone, Copy)]
enum ParseCase {
    FastHtmlParserBuild,
    FastHtmlParserLifecycle,
    FastHtmlParserOwnedBuild,
    FastHtmlParserOwnedLifecycle,
    ScraperBuild,
    ScraperLifecycle,
    TlBuild,
    TlLifecycle,
    TlOwnedBuild,
    TlOwnedLifecycle,
    LolHtmlLifecycle,
}

impl ParseCase {
    const fn namespace(self) -> &'static str {
        match self {
            Self::FastHtmlParserBuild | Self::ScraperBuild => "dom/build",
            Self::FastHtmlParserLifecycle | Self::ScraperLifecycle => "dom/lifecycle",
            Self::TlBuild => "zero_copy/build",
            Self::TlLifecycle => "zero_copy/lifecycle",
            Self::FastHtmlParserOwnedBuild | Self::TlOwnedBuild => "owned/build",
            Self::FastHtmlParserOwnedLifecycle | Self::TlOwnedLifecycle => "owned/lifecycle",
            Self::LolHtmlLifecycle => "streaming/lifecycle",
        }
    }

    const fn implementation(self) -> &'static str {
        match self {
            Self::FastHtmlParserBuild
            | Self::FastHtmlParserLifecycle
            | Self::FastHtmlParserOwnedBuild
            | Self::FastHtmlParserOwnedLifecycle => "fast_html_parser",
            Self::ScraperBuild | Self::ScraperLifecycle => "scraper",
            Self::TlBuild | Self::TlLifecycle | Self::TlOwnedBuild | Self::TlOwnedLifecycle => "tl",
            Self::LolHtmlLifecycle => "lol_html_noop_rewrite",
        }
    }
}

fn register_parse_case(group: &mut Group<'_>, case: ParseCase, html: &str) {
    let id = BenchmarkId::new(case.namespace(), case.implementation());
    match case {
        ParseCase::FastHtmlParserBuild => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| fast_html_parser::HtmlParser::parse(std::hint::black_box(html)).unwrap(),
                BatchSize::LargeInput,
            );
        }),
        ParseCase::FastHtmlParserLifecycle => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| {
                    let doc =
                        fast_html_parser::HtmlParser::parse(std::hint::black_box(html)).unwrap();
                    drop(std::hint::black_box(doc));
                },
                BatchSize::LargeInput,
            );
        }),
        ParseCase::FastHtmlParserOwnedBuild => group.bench_function(id, |b| {
            b.iter_batched(
                || html.to_owned(),
                |owned| fast_html_parser::HtmlParser::parse_owned(owned).unwrap(),
                BatchSize::LargeInput,
            );
        }),
        ParseCase::FastHtmlParserOwnedLifecycle => group.bench_function(id, |b| {
            b.iter_batched(
                || html.to_owned(),
                |owned| {
                    let doc = fast_html_parser::HtmlParser::parse_owned(owned).unwrap();
                    drop(std::hint::black_box(doc));
                },
                BatchSize::LargeInput,
            );
        }),
        ParseCase::ScraperBuild => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| scraper::Html::parse_document(std::hint::black_box(html)),
                BatchSize::LargeInput,
            );
        }),
        ParseCase::ScraperLifecycle => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| {
                    let doc = scraper::Html::parse_document(std::hint::black_box(html));
                    drop(std::hint::black_box(doc));
                },
                BatchSize::LargeInput,
            );
        }),
        ParseCase::TlBuild => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| tl::parse(std::hint::black_box(html), tl::ParserOptions::default()).unwrap(),
                BatchSize::LargeInput,
            );
        }),
        ParseCase::TlLifecycle => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| {
                    let dom = tl::parse(std::hint::black_box(html), tl::ParserOptions::default())
                        .unwrap();
                    drop(std::hint::black_box(dom));
                },
                BatchSize::LargeInput,
            );
        }),
        ParseCase::TlOwnedBuild => group.bench_function(id, |b| {
            b.iter_batched(
                || html.to_owned(),
                |owned| {
                    // SAFETY: the guard owns its input and outlives all DOM references.
                    unsafe { tl::parse_owned(owned, tl::ParserOptions::default()) }.unwrap()
                },
                BatchSize::LargeInput,
            );
        }),
        ParseCase::TlOwnedLifecycle => group.bench_function(id, |b| {
            b.iter_batched(
                || html.to_owned(),
                |owned| {
                    // SAFETY: the guard owns its input and is dropped in this iteration.
                    let dom =
                        unsafe { tl::parse_owned(owned, tl::ParserOptions::default()) }.unwrap();
                    drop(std::hint::black_box(dom));
                },
                BatchSize::LargeInput,
            );
        }),
        ParseCase::LolHtmlLifecycle => group.bench_function(id, |b| {
            let bytes = html.as_bytes();
            b.iter(|| {
                let mut rewriter = lol_html::HtmlRewriter::new(
                    lol_html::Settings {
                        element_content_handlers: vec![lol_html::element!("*", |_element| Ok(()))],
                        ..lol_html::Settings::new()
                    },
                    |_: &[u8]| {},
                );
                rewriter.write(std::hint::black_box(bytes)).unwrap();
                // `end` consumes the rewriter, so this is inherently a lifecycle workload.
                rewriter.end().unwrap();
            });
        }),
    };
}

fn semantic_parse_cases(order: FhpBenchOrder) -> Vec<ParseCase> {
    let mut cases = order_dom_parser_blocks(
        vec![
            ParseCase::FastHtmlParserBuild,
            ParseCase::FastHtmlParserLifecycle,
            ParseCase::FastHtmlParserOwnedBuild,
            ParseCase::FastHtmlParserOwnedLifecycle,
        ],
        vec![ParseCase::ScraperBuild, ParseCase::ScraperLifecycle],
        vec![
            ParseCase::TlBuild,
            ParseCase::TlLifecycle,
            ParseCase::TlOwnedBuild,
            ParseCase::TlOwnedLifecycle,
        ],
        order,
    );
    // Keep the streaming rewriter outside the three-parser DOM permutation: it
    // is a distinct lifecycle workload and has no comparable materialized tree.
    cases.push(ParseCase::LolHtmlLifecycle);
    cases
}

#[cfg(feature = "css-selector")]
fn direct_parse_cases(
    implementations: &[DomImplementation],
    order: FhpBenchOrder,
) -> Vec<ParseCase> {
    let mut fhp = Vec::new();
    let mut scraper = Vec::new();
    let mut tl = Vec::new();
    for implementation in implementations {
        let cases = match implementation {
            DomImplementation::FastHtmlParser => vec![
                ParseCase::FastHtmlParserBuild,
                ParseCase::FastHtmlParserLifecycle,
            ],
            DomImplementation::Scraper => {
                vec![ParseCase::ScraperBuild, ParseCase::ScraperLifecycle]
            }
            DomImplementation::Tl => vec![ParseCase::TlBuild, ParseCase::TlLifecycle],
        };
        if *implementation == DomImplementation::FastHtmlParser {
            fhp = cases;
        } else if *implementation == DomImplementation::Scraper {
            scraper = cases;
        } else {
            tl = cases;
        }
    }
    order_dom_parser_blocks(fhp, scraper, tl, order)
}

#[cfg(feature = "css-selector")]
fn parse_contract_equal_sets(
    contract: &FixtureContract,
) -> Vec<(&'static str, Vec<DomImplementation>)> {
    if contract.parity.fast_html_parser_scraper
        && contract.parity.fast_html_parser_tl
        && contract.parity.scraper_tl
    {
        return vec![(
            "all_dom",
            vec![
                DomImplementation::FastHtmlParser,
                DomImplementation::Scraper,
                DomImplementation::Tl,
            ],
        )];
    }

    let mut sets = Vec::new();
    if contract.parity.fast_html_parser_scraper {
        sets.push((
            "fhp_scraper",
            vec![
                DomImplementation::FastHtmlParser,
                DomImplementation::Scraper,
            ],
        ));
    }
    if contract.parity.fast_html_parser_tl {
        sets.push((
            "fhp_tl",
            vec![DomImplementation::FastHtmlParser, DomImplementation::Tl],
        ));
    }
    if contract.parity.scraper_tl {
        sets.push((
            "scraper_tl",
            vec![DomImplementation::Scraper, DomImplementation::Tl],
        ));
    }
    sets
}

pub fn register_parse_groups(
    c: &mut Criterion,
    suite: &str,
    benchmark_fixture_id: &str,
    html: &str,
    contract: &FixtureContract,
    order: FhpBenchOrder,
) {
    let mut absolute = c.benchmark_group(format!(
        "{suite}/{benchmark_fixture_id}/parse/semantic_reference"
    ));
    absolute.throughput(Throughput::Bytes(html.len() as u64));
    for case in semantic_parse_cases(order) {
        register_parse_case(&mut absolute, case, html);
    }
    absolute.finish();

    #[cfg(feature = "css-selector")]
    {
        for (parity_id, implementations) in parse_contract_equal_sets(contract) {
            let mut direct = c.benchmark_group(format!(
                "{suite}/{benchmark_fixture_id}/parse/contract_equal/{parity_id}"
            ));
            direct.throughput(Throughput::Bytes(html.len() as u64));
            for case in direct_parse_cases(&implementations, order) {
                register_parse_case(&mut direct, case, html);
            }
            direct.finish();
        }
    }
    #[cfg(not(feature = "css-selector"))]
    let _ = contract;
}

#[cfg(feature = "css-selector")]
#[derive(Clone, Copy)]
enum SelectorBenchCase {
    FastHtmlParserCompile,
    FastHtmlParserEvaluate,
    ScraperCompile,
    ScraperEvaluate,
    TlCompile,
    TlEvaluate,
}

#[cfg(feature = "css-selector")]
impl SelectorBenchCase {
    const fn namespace(self) -> &'static str {
        match self {
            Self::FastHtmlParserCompile | Self::ScraperCompile | Self::TlCompile => "compile",
            Self::FastHtmlParserEvaluate | Self::ScraperEvaluate | Self::TlEvaluate => {
                "evaluate_materialized"
            }
        }
    }

    const fn implementation(self) -> DomImplementation {
        match self {
            Self::FastHtmlParserCompile | Self::FastHtmlParserEvaluate => {
                DomImplementation::FastHtmlParser
            }
            Self::ScraperCompile | Self::ScraperEvaluate => DomImplementation::Scraper,
            Self::TlCompile | Self::TlEvaluate => DomImplementation::Tl,
        }
    }
}

#[cfg(feature = "css-selector")]
#[allow(clippy::too_many_arguments)]
fn register_selector_case(
    group: &mut Group<'_>,
    case: SelectorBenchCase,
    css: &str,
    fhp_doc: &fast_html_parser::Document,
    fhp_selector: &fast_html_parser::CompiledSelector,
    scraper_doc: &scraper::Html,
    scraper_selector: &scraper::Selector,
    tl_dom: &tl::VDom<'_>,
    tl_selector: &tl::queryselector::Selector<'_>,
) {
    use fast_html_parser::Selectable;

    let id = BenchmarkId::new(case.namespace(), case.implementation().as_str());
    match case {
        SelectorBenchCase::FastHtmlParserCompile => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| fast_html_parser::CompiledSelector::new(std::hint::black_box(css)).unwrap(),
                BatchSize::SmallInput,
            );
        }),
        SelectorBenchCase::FastHtmlParserEvaluate => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| fhp_doc.select_compiled(fhp_selector).unwrap(),
                BatchSize::LargeInput,
            );
        }),
        SelectorBenchCase::ScraperCompile => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| scraper::Selector::parse(std::hint::black_box(css)).unwrap(),
                BatchSize::SmallInput,
            );
        }),
        SelectorBenchCase::ScraperEvaluate => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| scraper_doc.select(scraper_selector).collect::<Vec<_>>(),
                BatchSize::LargeInput,
            );
        }),
        SelectorBenchCase::TlCompile => group.bench_function(id, |b| {
            b.iter_batched(
                || (),
                |_| {
                    tl::parse_query_selector(std::hint::black_box(css)).expect("validated selector")
                },
                BatchSize::SmallInput,
            );
        }),
        SelectorBenchCase::TlEvaluate => group.bench_function(id, |b| {
            b.iter_batched(
                || tl_selector.clone(),
                |selector| {
                    tl::queryselector::QuerySelectorIterator::new(selector, tl_dom.parser(), tl_dom)
                        .collect::<Vec<_>>()
                },
                BatchSize::LargeInput,
            );
        }),
    };
}

#[cfg(feature = "css-selector")]
fn selector_cases(
    implementations: &[DomImplementation],
    order: FhpBenchOrder,
) -> Vec<SelectorBenchCase> {
    let mut fhp = Vec::new();
    let mut scraper = Vec::new();
    let mut tl = Vec::new();
    for implementation in implementations {
        let cases = match implementation {
            DomImplementation::FastHtmlParser => vec![
                SelectorBenchCase::FastHtmlParserCompile,
                SelectorBenchCase::FastHtmlParserEvaluate,
            ],
            DomImplementation::Scraper => vec![
                SelectorBenchCase::ScraperCompile,
                SelectorBenchCase::ScraperEvaluate,
            ],
            DomImplementation::Tl => {
                vec![SelectorBenchCase::TlCompile, SelectorBenchCase::TlEvaluate]
            }
        };
        if *implementation == DomImplementation::FastHtmlParser {
            fhp = cases;
        } else if *implementation == DomImplementation::Scraper {
            scraper = cases;
        } else {
            tl = cases;
        }
    }
    order_dom_parser_blocks(fhp, scraper, tl, order)
}

#[cfg(feature = "css-selector")]
fn selector_contract_equal_sets(
    fixture: &FixtureContract,
    selector: &SelectorWorkloadContract,
) -> Vec<(&'static str, Vec<DomImplementation>)> {
    let parity = intersect_selector_parity(fixture, selector);

    if parity.all_equal() {
        return vec![(
            "all_dom",
            vec![
                DomImplementation::FastHtmlParser,
                DomImplementation::Scraper,
                DomImplementation::Tl,
            ],
        )];
    }

    let mut sets = Vec::new();
    if parity.fast_html_parser_scraper {
        sets.push((
            "fhp_scraper",
            vec![
                DomImplementation::FastHtmlParser,
                DomImplementation::Scraper,
            ],
        ));
    }
    if parity.fast_html_parser_tl {
        sets.push((
            "fhp_tl",
            vec![DomImplementation::FastHtmlParser, DomImplementation::Tl],
        ));
    }
    if parity.scraper_tl {
        sets.push((
            "scraper_tl",
            vec![DomImplementation::Scraper, DomImplementation::Tl],
        ));
    }
    sets
}

#[cfg(feature = "css-selector")]
pub fn register_selector_groups(
    c: &mut Criterion,
    suite: &str,
    benchmark_fixture_id: &str,
    html: &str,
    contract_fixture_id: &str,
    selectors: &[(&str, &str)],
    order: FhpBenchOrder,
) {
    use fast_html_parser::Selectable;

    let fixture = fixture_contract(contract_fixture_id);
    for &(selector_id, css) in selectors {
        let contract = selector_workload_contract(contract_fixture_id, selector_id);
        assert_eq!(
            css, contract.css,
            "selector source drift for {contract_fixture_id}/{selector_id}"
        );
        let fhp_doc = fast_html_parser::HtmlParser::parse(html).unwrap();
        let fhp_selector = fast_html_parser::CompiledSelector::new(css).unwrap();
        let scraper_doc = scraper::Html::parse_document(html);
        let scraper_selector = scraper::Selector::parse(css).unwrap();
        let tl_dom = tl::parse(html, tl::ParserOptions::default()).unwrap();
        let tl_selector = tl::parse_query_selector(css).expect("valid benchmark selector");

        let counts = [
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
                tl::queryselector::QuerySelectorIterator::new(
                    tl_selector.clone(),
                    tl_dom.parser(),
                    &tl_dom,
                )
                .collect::<Vec<_>>()
                .len(),
            ),
        ];
        assert_selector_workload_counts(contract, &counts);

        let implementations = [
            DomImplementation::FastHtmlParser,
            DomImplementation::Scraper,
            DomImplementation::Tl,
        ];
        let mut absolute = c.benchmark_group(format!(
            "{suite}/{benchmark_fixture_id}/selector/{selector_id}/semantic_reference"
        ));
        for case in selector_cases(&implementations, order) {
            register_selector_case(
                &mut absolute,
                case,
                css,
                &fhp_doc,
                &fhp_selector,
                &scraper_doc,
                &scraper_selector,
                &tl_dom,
                &tl_selector,
            );
        }
        absolute.finish();

        for (parity_id, implementations) in selector_contract_equal_sets(fixture, contract) {
            let mut direct = c.benchmark_group(format!(
                "{suite}/{benchmark_fixture_id}/selector/{selector_id}/contract_equal/{parity_id}"
            ));
            for case in selector_cases(&implementations, order) {
                register_selector_case(
                    &mut direct,
                    case,
                    css,
                    &fhp_doc,
                    &fhp_selector,
                    &scraper_doc,
                    &scraper_selector,
                    &tl_dom,
                    &tl_selector,
                );
            }
            direct.finish();
        }
    }
}
