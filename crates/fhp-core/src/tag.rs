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

    /// Any tag name not in the known set.
    Unknown = 255,
}

/// The maximum byte-length of any known tag name (`"textarea"` = 8).
const MAX_KNOWN_TAG_LEN: usize = 8;

/// Number of void element variants (discriminants `0..VOID_COUNT`).
const VOID_COUNT: u8 = 14;

/// Compile-time perfect-hash map from **lowercase** tag name to [`Tag`].
static TAG_MAP: phf::Map<&'static [u8], Tag> = phf::phf_map! {
    b"area"     => Tag::Area,
    b"base"     => Tag::Base,
    b"br"       => Tag::Br,
    b"col"      => Tag::Col,
    b"embed"    => Tag::Embed,
    b"hr"       => Tag::Hr,
    b"img"      => Tag::Img,
    b"input"    => Tag::Input,
    b"link"     => Tag::Link,
    b"meta"     => Tag::Meta,
    b"param"    => Tag::Param,
    b"source"   => Tag::Source,
    b"track"    => Tag::Track,
    b"wbr"      => Tag::Wbr,
    b"a"        => Tag::A,
    b"abbr"     => Tag::Abbr,
    b"article"  => Tag::Article,
    b"aside"    => Tag::Aside,
    b"b"        => Tag::B,
    b"body"     => Tag::Body,
    b"button"   => Tag::Button,
    b"div"      => Tag::Div,
    b"em"       => Tag::Em,
    b"footer"   => Tag::Footer,
    b"form"     => Tag::Form,
    b"h1"       => Tag::H1,
    b"h2"       => Tag::H2,
    b"h3"       => Tag::H3,
    b"h4"       => Tag::H4,
    b"h5"       => Tag::H5,
    b"h6"       => Tag::H6,
    b"head"     => Tag::Head,
    b"header"   => Tag::Header,
    b"html"     => Tag::Html,
    b"i"        => Tag::I,
    b"iframe"   => Tag::Iframe,
    b"li"       => Tag::Li,
    b"main"     => Tag::Main,
    b"nav"      => Tag::Nav,
    b"ol"       => Tag::Ol,
    b"p"        => Tag::P,
    b"pre"      => Tag::Pre,
    b"script"   => Tag::Script,
    b"section"  => Tag::Section,
    b"select"   => Tag::Select,
    b"span"     => Tag::Span,
    b"strong"   => Tag::Strong,
    b"style"    => Tag::Style,
    b"table"    => Tag::Table,
    b"tbody"    => Tag::Tbody,
    b"td"       => Tag::Td,
    b"textarea" => Tag::Textarea,
    b"th"       => Tag::Th,
    b"thead"    => Tag::Thead,
    b"title"    => Tag::Title,
    b"tr"       => Tag::Tr,
    b"ul"       => Tag::Ul,
    b"video"    => Tag::Video,
};

impl Tag {
    /// Look up a tag by its raw byte name using a compile-time perfect hash.
    ///
    /// Case-insensitive: the input is lowercased inline on a stack buffer
    /// before the PHF lookup, avoiding any heap allocation.
    #[inline]
    pub fn from_bytes(name: &[u8]) -> Tag {
        if name.is_empty() || name.len() > MAX_KNOWN_TAG_LEN {
            return Tag::Unknown;
        }

        // Lowercase on the stack — MAX_KNOWN_TAG_LEN is 8 so this is tiny.
        let mut buf = [0u8; MAX_KNOWN_TAG_LEN];
        let lower = &mut buf[..name.len()];
        for (dst, &src) in lower.iter_mut().zip(name) {
            *dst = src.to_ascii_lowercase();
        }

        TAG_MAP.get(lower).copied().unwrap_or(Tag::Unknown)
    }

    /// Returns `true` if this is a void element (self-closing, no children).
    ///
    /// Branch-free: exploits the fact that void elements occupy discriminants
    /// `0..14`.
    #[inline(always)]
    pub const fn is_void(self) -> bool {
        (self as u8) < VOID_COUNT
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
    }

    #[test]
    fn case_insensitive_lookup() {
        assert_eq!(Tag::from_bytes(b"DIV"), Tag::Div);
        assert_eq!(Tag::from_bytes(b"Div"), Tag::Div);
        assert_eq!(Tag::from_bytes(b"TEXTAREA"), Tag::Textarea);
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
