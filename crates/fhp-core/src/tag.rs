/// Known HTML tag names interned as a `u8` discriminant.
///
/// Comparison becomes a single integer compare instead of a string compare.
/// The first 14 variants (0..14) are void elements — `is_void()` exploits this
/// for a branch-free check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Tag {
    // ── Void elements (discriminants 0..14) ──
    /// `<area>`
    Area = 0,
    /// `<base>`
    Base,
    /// `<br>`
    Br,
    /// `<col>`
    Col,
    /// `<embed>`
    Embed,
    /// `<hr>`
    Hr,
    /// `<img>`
    Img,
    /// `<input>`
    Input,
    /// `<link>`
    Link,
    /// `<meta>`
    Meta,
    /// `<param>`
    Param,
    /// `<source>`
    Source,
    /// `<track>`
    Track,
    /// `<wbr>`
    Wbr,

    // ── Common elements (discriminants 14..) ──
    /// `<a>`
    A,
    /// `<abbr>`
    Abbr,
    /// `<article>`
    Article,
    /// `<aside>`
    Aside,
    /// `<b>`
    B,
    /// `<body>`
    Body,
    /// `<button>`
    Button,
    /// `<div>`
    Div,
    /// `<em>`
    Em,
    /// `<footer>`
    Footer,
    /// `<form>`
    Form,
    /// `<h1>`
    H1,
    /// `<h2>`
    H2,
    /// `<h3>`
    H3,
    /// `<h4>`
    H4,
    /// `<h5>`
    H5,
    /// `<h6>`
    H6,
    /// `<head>`
    Head,
    /// `<header>`
    Header,
    /// `<html>`
    Html,
    /// `<i>`
    I,
    /// `<iframe>`
    Iframe,
    /// `<li>`
    Li,
    /// `<main>`
    Main,
    /// `<nav>`
    Nav,
    /// `<ol>`
    Ol,
    /// `<p>`
    P,
    /// `<pre>`
    Pre,
    /// `<script>`
    Script,
    /// `<section>`
    Section,
    /// `<select>`
    Select,
    /// `<span>`
    Span,
    /// `<strong>`
    Strong,
    /// `<style>`
    Style,
    /// `<table>`
    Table,
    /// `<tbody>`
    Tbody,
    /// `<td>`
    Td,
    /// `<textarea>`
    Textarea,
    /// `<th>`
    Th,
    /// `<thead>`
    Thead,
    /// `<title>`
    Title,
    /// `<tr>`
    Tr,
    /// `<ul>`
    Ul,
    /// `<video>`
    Video,
    /// `<xmp>`
    Xmp,
    /// `<noembed>`
    Noembed,
    /// `<noframes>`
    Noframes,
    /// `<plaintext>`
    Plaintext,

    /// Any tag name not in the known set.
    Unknown = 255,
}

/// Number of void element variants (discriminants `0..VOID_COUNT`).
const VOID_COUNT: u8 = 14;

/// Every known tag with its lowercase name, in discriminant order.
const KNOWN_TAGS: [(&[u8], Tag); 62] = [
    (b"area", Tag::Area),
    (b"base", Tag::Base),
    (b"br", Tag::Br),
    (b"col", Tag::Col),
    (b"embed", Tag::Embed),
    (b"hr", Tag::Hr),
    (b"img", Tag::Img),
    (b"input", Tag::Input),
    (b"link", Tag::Link),
    (b"meta", Tag::Meta),
    (b"param", Tag::Param),
    (b"source", Tag::Source),
    (b"track", Tag::Track),
    (b"wbr", Tag::Wbr),
    (b"a", Tag::A),
    (b"abbr", Tag::Abbr),
    (b"article", Tag::Article),
    (b"aside", Tag::Aside),
    (b"b", Tag::B),
    (b"body", Tag::Body),
    (b"button", Tag::Button),
    (b"div", Tag::Div),
    (b"em", Tag::Em),
    (b"footer", Tag::Footer),
    (b"form", Tag::Form),
    (b"h1", Tag::H1),
    (b"h2", Tag::H2),
    (b"h3", Tag::H3),
    (b"h4", Tag::H4),
    (b"h5", Tag::H5),
    (b"h6", Tag::H6),
    (b"head", Tag::Head),
    (b"header", Tag::Header),
    (b"html", Tag::Html),
    (b"i", Tag::I),
    (b"iframe", Tag::Iframe),
    (b"li", Tag::Li),
    (b"main", Tag::Main),
    (b"nav", Tag::Nav),
    (b"ol", Tag::Ol),
    (b"p", Tag::P),
    (b"pre", Tag::Pre),
    (b"script", Tag::Script),
    (b"section", Tag::Section),
    (b"select", Tag::Select),
    (b"span", Tag::Span),
    (b"strong", Tag::Strong),
    (b"style", Tag::Style),
    (b"table", Tag::Table),
    (b"tbody", Tag::Tbody),
    (b"td", Tag::Td),
    (b"textarea", Tag::Textarea),
    (b"th", Tag::Th),
    (b"thead", Tag::Thead),
    (b"title", Tag::Title),
    (b"tr", Tag::Tr),
    (b"ul", Tag::Ul),
    (b"video", Tag::Video),
    (b"xmp", Tag::Xmp),
    (b"noembed", Tag::Noembed),
    (b"noframes", Tag::Noframes),
    (b"plaintext", Tag::Plaintext),
];

/// Multiplier of the multiply-shift hash that gives every known name of at
/// most eight bytes its own [`TAG_TABLE`] slot. Building the table fails at
/// compile time if a change to [`KNOWN_TAGS`] introduces a collision.
const TAG_HASH_MULTIPLIER: u64 = 0x3e7a_1eea_9267_bafd;

/// Known names of at most eight bytes, packed by [`pack_name`] and stored at
/// their hash slot. `plaintext` is the only longer name and is matched
/// directly.
static TAG_TABLE: TagTable = TagTable::build();

struct TagTable {
    keys: [u64; 256],
    tags: [Tag; 256],
}

impl TagTable {
    const fn build() -> Self {
        let mut keys = [0u64; 256];
        let mut tags = [Tag::Unknown; 256];
        let mut index = 0;
        while index < KNOWN_TAGS.len() {
            let (name, tag) = KNOWN_TAGS[index];
            if name.len() <= 8 {
                let key = pack_name(name);
                let slot = tag_slot(key);
                assert!(keys[slot] == 0, "tag name hash collision");
                keys[slot] = key;
                tags[slot] = tag;
            }
            index += 1;
        }
        Self { keys, tags }
    }
}

/// Pack up to eight bytes into a little-endian `u64`.
const fn pack_name(name: &[u8]) -> u64 {
    let mut key = 0u64;
    let mut index = 0;
    while index < name.len() {
        key |= (name[index] as u64) << (8 * index);
        index += 1;
    }
    key
}

#[inline(always)]
const fn tag_slot(key: u64) -> usize {
    (key.wrapping_mul(TAG_HASH_MULTIPLIER) >> 56) as usize
}

impl Tag {
    /// Look up a tag by its raw byte name using a compile-time perfect hash.
    ///
    /// Case-insensitive: the name is lowercased while it is packed into one
    /// `u64`, which then selects a single table slot to compare against.
    #[inline]
    pub fn from_bytes(name: &[u8]) -> Tag {
        match name.len() {
            1..=8 => {
                let mut key = 0u64;
                for (index, &byte) in name.iter().enumerate() {
                    key |= u64::from(byte.to_ascii_lowercase()) << (8 * index);
                }
                let slot = tag_slot(key);
                // A name containing NUL packs like a shorter one; comparing the
                // packed length with the input length rejects it.
                let packed_len = (64 - TAG_TABLE.keys[slot].leading_zeros()).div_ceil(8);
                if TAG_TABLE.keys[slot] == key && packed_len as usize == name.len() {
                    TAG_TABLE.tags[slot]
                } else {
                    Tag::Unknown
                }
            }
            9 if name.eq_ignore_ascii_case(b"plaintext") => Tag::Plaintext,
            _ => Tag::Unknown,
        }
    }

    /// Returns `true` if this is a void element (self-closing, no children).
    ///
    /// Branch-free: exploits the fact that void elements occupy discriminants
    /// `0..14`.
    #[inline(always)]
    pub const fn is_void(self) -> bool {
        (self as u8) < VOID_COUNT
    }

    /// Returns `true` if this is a raw text element.
    ///
    /// Raw text elements' content must not be entity-escaped during
    /// serialization, and the tokenizer enters a special raw-text mode for
    /// them. [`Tag::Plaintext`] is deliberately excluded: it has no end tag
    /// and keeps its own tokenizer mode.
    #[inline(always)]
    pub const fn is_raw_text(self) -> bool {
        matches!(
            self,
            Tag::Script | Tag::Style | Tag::Iframe | Tag::Xmp | Tag::Noembed | Tag::Noframes
        )
    }

    /// Returns `true` if this is an RCDATA element (`<title>`, `<textarea>`).
    ///
    /// RCDATA is tokenized like raw text except that character references are
    /// decoded. Markup other than the matching end tag remains text.
    #[inline(always)]
    pub const fn is_rcdata(self) -> bool {
        matches!(self, Tag::Title | Tag::Textarea)
    }

    /// Returns the canonical lowercase tag name, or `None` for [`Tag::Unknown`].
    #[inline]
    pub const fn as_str(self) -> Option<&'static str> {
        match self {
            Tag::Area => Some("area"),
            Tag::Base => Some("base"),
            Tag::Br => Some("br"),
            Tag::Col => Some("col"),
            Tag::Embed => Some("embed"),
            Tag::Hr => Some("hr"),
            Tag::Img => Some("img"),
            Tag::Input => Some("input"),
            Tag::Link => Some("link"),
            Tag::Meta => Some("meta"),
            Tag::Param => Some("param"),
            Tag::Source => Some("source"),
            Tag::Track => Some("track"),
            Tag::Wbr => Some("wbr"),
            Tag::A => Some("a"),
            Tag::Abbr => Some("abbr"),
            Tag::Article => Some("article"),
            Tag::Aside => Some("aside"),
            Tag::B => Some("b"),
            Tag::Body => Some("body"),
            Tag::Button => Some("button"),
            Tag::Div => Some("div"),
            Tag::Em => Some("em"),
            Tag::Footer => Some("footer"),
            Tag::Form => Some("form"),
            Tag::H1 => Some("h1"),
            Tag::H2 => Some("h2"),
            Tag::H3 => Some("h3"),
            Tag::H4 => Some("h4"),
            Tag::H5 => Some("h5"),
            Tag::H6 => Some("h6"),
            Tag::Head => Some("head"),
            Tag::Header => Some("header"),
            Tag::Html => Some("html"),
            Tag::I => Some("i"),
            Tag::Iframe => Some("iframe"),
            Tag::Li => Some("li"),
            Tag::Main => Some("main"),
            Tag::Nav => Some("nav"),
            Tag::Ol => Some("ol"),
            Tag::P => Some("p"),
            Tag::Pre => Some("pre"),
            Tag::Script => Some("script"),
            Tag::Section => Some("section"),
            Tag::Select => Some("select"),
            Tag::Span => Some("span"),
            Tag::Strong => Some("strong"),
            Tag::Style => Some("style"),
            Tag::Table => Some("table"),
            Tag::Tbody => Some("tbody"),
            Tag::Td => Some("td"),
            Tag::Textarea => Some("textarea"),
            Tag::Th => Some("th"),
            Tag::Thead => Some("thead"),
            Tag::Title => Some("title"),
            Tag::Tr => Some("tr"),
            Tag::Ul => Some("ul"),
            Tag::Video => Some("video"),
            Tag::Xmp => Some("xmp"),
            Tag::Noembed => Some("noembed"),
            Tag::Noframes => Some("noframes"),
            Tag::Plaintext => Some("plaintext"),
            Tag::Unknown => None,
        }
    }
}

impl core::fmt::Display for Tag {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.as_str() {
            Some(s) => f.write_str(s),
            None => f.write_str("(unknown)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tag_lookup() {
        assert_eq!(Tag::from_bytes(b"div"), Tag::Div);
        assert_eq!(Tag::from_bytes(b"span"), Tag::Span);
        assert_eq!(Tag::from_bytes(b"a"), Tag::A);
        assert_eq!(Tag::from_bytes(b"textarea"), Tag::Textarea);
        assert_eq!(Tag::from_bytes(b"PlainText"), Tag::Plaintext);
        assert_eq!(Tag::from_bytes(b"noframes"), Tag::Noframes);
    }

    #[test]
    fn case_insensitive_lookup() {
        assert_eq!(Tag::from_bytes(b"DIV"), Tag::Div);
        assert_eq!(Tag::from_bytes(b"Div"), Tag::Div);
        assert_eq!(Tag::from_bytes(b"TEXTAREA"), Tag::Textarea);
    }

    #[test]
    fn every_known_tag_round_trips_in_any_case() {
        for (index, (name, tag)) in KNOWN_TAGS.iter().enumerate() {
            assert_eq!(
                *tag as usize, index,
                "KNOWN_TAGS must be in discriminant order"
            );
            assert_eq!(tag.as_str().map(str::as_bytes), Some(*name));
            assert_eq!(Tag::from_bytes(name), *tag);
            assert_eq!(Tag::from_bytes(&name.to_ascii_uppercase()), *tag);
        }
        assert_eq!(KNOWN_TAGS.len(), Tag::Plaintext as usize + 1);
    }

    #[test]
    fn near_misses_are_unknown() {
        for name in [
            &b"di"[..],
            b"divv",
            b"d\0v",
            b"div\0",
            b"\0",
            b"a\0",
            b"\0\0\0\0\0\0\0\0",
            b"plaintex",
            b"plaintexts",
            b"plaintex\0",
            b"textarea1",
            b"h7",
            b"\xC4\xB0mg",
        ] {
            assert_eq!(Tag::from_bytes(name), Tag::Unknown, "{name:?}");
        }
    }

    #[test]
    fn unknown_tag() {
        assert_eq!(Tag::from_bytes(b"custom-element"), Tag::Unknown);
        assert_eq!(Tag::from_bytes(b""), Tag::Unknown);
        assert_eq!(Tag::from_bytes(b"verylongtagname"), Tag::Unknown);
    }

    #[test]
    fn void_elements() {
        assert!(Tag::Br.is_void());
        assert!(Tag::Img.is_void());
        assert!(Tag::Input.is_void());
        assert!(Tag::Hr.is_void());
        assert!(Tag::Meta.is_void());
        assert!(Tag::Link.is_void());
        assert!(Tag::Area.is_void());
        assert!(Tag::Base.is_void());
        assert!(Tag::Col.is_void());
        assert!(Tag::Embed.is_void());
        assert!(Tag::Param.is_void());
        assert!(Tag::Source.is_void());
        assert!(Tag::Track.is_void());
        assert!(Tag::Wbr.is_void());
    }

    #[test]
    fn non_void_elements() {
        assert!(!Tag::Div.is_void());
        assert!(!Tag::Span.is_void());
        assert!(!Tag::A.is_void());
        assert!(!Tag::P.is_void());
        assert!(!Tag::Unknown.is_void());
    }

    #[test]
    fn tag_display() {
        assert_eq!(Tag::Div.to_string(), "div");
        assert_eq!(Tag::Unknown.to_string(), "(unknown)");
    }

    #[test]
    fn raw_text_elements() {
        assert!(Tag::Script.is_raw_text());
        assert!(Tag::Style.is_raw_text());
        assert!(Tag::Iframe.is_raw_text());
        assert!(Tag::Xmp.is_raw_text());
        assert!(Tag::Noembed.is_raw_text());
        assert!(Tag::Noframes.is_raw_text());
        assert!(!Tag::Plaintext.is_raw_text());
        assert!(!Tag::Div.is_raw_text());
        assert!(!Tag::Textarea.is_raw_text());
        assert!(!Tag::Title.is_raw_text());
        assert!(!Tag::Unknown.is_raw_text());
    }

    #[test]
    fn rcdata_elements() {
        assert!(Tag::Textarea.is_rcdata());
        assert!(Tag::Title.is_rcdata());
        assert!(!Tag::Script.is_rcdata());
        assert!(!Tag::Div.is_rcdata());
    }

    #[test]
    fn all_void_tags_have_discriminant_below_14() {
        let void_tags = [
            Tag::Area,
            Tag::Base,
            Tag::Br,
            Tag::Col,
            Tag::Embed,
            Tag::Hr,
            Tag::Img,
            Tag::Input,
            Tag::Link,
            Tag::Meta,
            Tag::Param,
            Tag::Source,
            Tag::Track,
            Tag::Wbr,
        ];
        for tag in void_tags {
            assert!(
                (tag as u8) < 14,
                "{tag:?} has discriminant {} >= 14",
                tag as u8
            );
        }
    }
}
