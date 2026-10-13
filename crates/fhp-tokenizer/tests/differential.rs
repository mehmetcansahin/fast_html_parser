//! Differential, invariant, and metamorphic checks across every tokenizer
//! entry point. The `tokenizer_differential` fuzz target runs the same oracle
//! on arbitrary input.

mod oracle;

use oracle::{Chunking, SplitMix64, check};

/// Inputs that exercise every tokenizer mode, its terminators, and its EOF
/// handling.
const EDGE_CASES: &[&str] = &[
    "",
    "plain text",
    "<div>hello</div>",
    "<p class=\"a\" id='b' data-x=1 hidden>t</p>",
    "<a href=\"x>y\" title='a\"b'>link</a>",
    "<a b=c/>",
    "<a b=\"c\"/>",
    "<a b/>",
    "<br/><br /><br/ >",
    "<a =x=\"1\" x\"y=2 'q=3>",
    "<a b/=c>",
    "<a B=1 b=2 \u{e9}=3 \u{c9}=4>",
    // Duplicates after the inline duplicate tracker spills to its hash set.
    "<a a1 a2 a3 a4 a5 a6 a7 a8 a9 a10 A9=x a10=y a1=z>",
    "<a/b/c>",
    "<div\n\tclass\n=\n\"x\"\n>",
    "a < b > c",
    "<",
    "</",
    "</>",
    "a</>b",
    "a<</>b",
    "</ x>",
    "</3>",
    "<?xml version=\"1.0\"?>",
    "<!x>",
    "<!>",
    "<!->",
    "<!-",
    "<!DOC",
    "<!DOCTYPE html>",
    "<!doctype html PUBLIC \"-//W3C//DTD\" 'x>y'>",
    "<!DOCTYPEhtml>",
    "<!DOCTYPE",
    "<!---->",
    "<!----->",
    "<!-->",
    "<!--->",
    "<!-- a --!> b",
    "<!--a--!-->",
    "<!--!>",
    "<!---!>",
    "<!--a-- >-->",
    "<!--eof",
    "<!--eof-",
    "<!--eof--",
    "<!--eof--!",
    "<![CDATA[a]]b]]>",
    "<![CDATA[x<y>z",
    "<![CDATA[]]]>",
    "<![CDATA",
    "<script>if (a < b && c > d) {}</script>",
    "<script><!--<script>x</script>y</script>z",
    "<script><!--x--></script>",
    "<script><!--></script>",
    "<script></scriptx></script>",
    "<SCRIPT>x</sCrIpT >y",
    "<script>unterminated",
    "<script>x</script",
    "<style>p > a { content: '</p>' }</style>",
    "<style></style\t>",
    "<title>a &amp; <b> &lt;/title&gt;</title>",
    "<textarea>&lt;</textarea x=\"</textarea>\">after",
    "<title>x</title",
    "<title>x</titl",
    "<xmp><b></xmp>",
    "<iframe><p></iframe>",
    "<noembed><i></noembed>",
    "<noframes><u></noframes>",
    "<plaintext></plaintext><b>&amp;",
    "a &amp; b &lt c &#65; &#x41; &#x110000; &#0; &notit; &notin; &amp",
    "<a href=\"?a=1&amp;b=2&copy=3&copy;\">x</a>",
    "&#x00000000000000000000041;",
    "&#9999999999999999999;",
    "<p title=\"\u{e7}\u{11f}\u{fc}\">\u{65e5}\u{672c}\u{8a9e}&nbsp;\u{1f642}</p>",
    "<\u{e9}>",
    "<div a=\"unterminated",
    "<div a='x' ",
    "<div",
    "\u{0}<\u{0}>",
];

/// Fragments the document generator concatenates.
const FRAGMENTS: &[&str] = &[
    "<div>",
    "</div>",
    "<p class=\"a b\">",
    "</p>",
    "<a href='x'>",
    "<A HREF=x>",
    "</a>",
    "<br/>",
    "<img src=x alt=\"a>b\">",
    "<input value='\"' checked/>",
    "<script>",
    "</script>",
    "</SCRIPT >",
    "<!--<script>",
    "<style>",
    "</style>",
    "<title>",
    "</title>",
    "<textarea>",
    "</textarea >",
    "<xmp>",
    "</xmp>",
    "<iframe>",
    "</iframe>",
    "<plaintext>",
    "<!--",
    "-->",
    "--!>",
    "<!---->",
    "<![CDATA[",
    "]]>",
    "<!DOCTYPE html>",
    "<!doctype",
    "<?",
    "</>",
    "</ ",
    "<!",
    "&amp;",
    "&lt",
    "&#x41;",
    "&#65",
    "&#x110000;",
    "&notin;",
    "&notit;",
    "&",
    "<",
    "</",
    ">",
    "/>",
    "\"",
    "'",
    "=",
    "/",
    " ",
    "\n",
    "\t",
    "a",
    "text",
    "x=\"",
    "y='",
    "z=",
    "\u{e9}",
    "\u{65e5}\u{672c}",
    "\u{1f642}",
    "\u{0}",
];

#[test]
fn edge_cases_agree_across_entry_points() {
    for case in EDGE_CASES {
        check(case, Chunking::Fixed(1), 0);
        // Pair every block offset with a different random chunking.
        for shift in 1..64 {
            check(case, Chunking::Random(shift as u64), shift);
        }
    }
}

#[test]
fn generated_documents_agree_across_entry_points() {
    let mut rng = SplitMix64(0x5eed);
    for _ in 0..3000 {
        let fragments = 1 + rng.next() as usize % 40;
        let mut html = String::new();
        for _ in 0..fragments {
            html.push_str(FRAGMENTS[rng.next() as usize % FRAGMENTS.len()]);
        }
        let seed = rng.next();
        check(&html, Chunking::Random(seed), seed as usize);
    }
}

/// Streaming flushes data text longer than its buffer bound in pieces; a
/// character reference near any flush point must still decode whole.
#[test]
fn long_text_runs_agree_across_entry_points() {
    const PIECES: &[&str] = &[
        "&amp;",
        "&amp",
        "&notin;",
        "&notit;",
        "&#x41;",
        "&#65",
        "&CounterClockwiseContourIntegral;",
        "&#x0000000041;",
        " < ",
        "\u{1f642}",
    ];
    let mut rng = SplitMix64(0x7e47);
    for prefix in ["", "<p>"] {
        let mut html = String::from(prefix);
        while html.len() < 20_000 {
            for _ in 0..1 + rng.next() % 40 {
                html.push('x');
            }
            html.push_str(PIECES[rng.next() as usize % PIECES.len()]);
        }
        for chunking in [
            Chunking::Fixed(1),
            Chunking::Fixed(61),
            Chunking::Random(rng.next()),
        ] {
            check(&html, chunking, 0);
        }
    }

    // Numeric references longer than the buffer bound.
    for reference in ["&#", "&#x"] {
        let html = format!("a{reference}{}; b", "9".repeat(5000));
        check(&html, Chunking::Fixed(1), 0);
        check(&html, Chunking::Random(5000), 0);
    }
}

#[test]
fn testdata_pages_agree_across_entry_points() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata");
    let mut pages: Vec<_> = std::fs::read_dir(dir)
        .expect("testdata directory exists")
        .map(|entry| entry.expect("readable testdata entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "html"))
        .collect();
    pages.sort();
    assert!(!pages.is_empty(), "no testdata pages found in {dir}");

    for (index, page) in pages.iter().enumerate() {
        let bytes = std::fs::read(page).expect("readable testdata page");
        let html = String::from_utf8_lossy(&bytes);
        let seed = index as u64 + 1;
        check(&html, Chunking::Random(seed), 1 + (index * 13) % 63);
    }
}
