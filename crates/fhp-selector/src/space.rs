//! HTML space-character helpers used by class-token matching and indexing.

/// Return whether `byte` is one of HTML's five ASCII space characters.
///
/// This intentionally excludes vertical tab (`U+000B`), which Rust's
/// [`u8::is_ascii_whitespace`] includes but HTML's space-separated token
/// definition does not.
#[inline(always)]
pub(crate) const fn is_html_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0C' | b'\r')
}

/// Iterate over the non-empty HTML space-separated tokens in `value`.
#[inline]
pub(crate) fn split_html_space(value: &str) -> impl Iterator<Item = &str> {
    HtmlSpaceTokens { value, position: 0 }
}

struct HtmlSpaceTokens<'a> {
    value: &'a str,
    position: usize,
}

impl<'a> Iterator for HtmlSpaceTokens<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.value.as_bytes();
        while self.position < bytes.len() && is_html_space(bytes[self.position]) {
            self.position += 1;
        }
        if self.position == bytes.len() {
            return None;
        }

        let start = self.position;
        while self.position < bytes.len() && !is_html_space(bytes[self.position]) {
            self.position += 1;
        }
        Some(&self.value[start..self.position])
    }
}

/// Return whether an HTML space-separated value contains `needle` exactly.
#[inline]
pub(crate) fn contains_html_space_token(value: &str, needle: &str) -> bool {
    !needle.is_empty() && split_html_space(value).any(|token| token == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_only_on_html_space_characters() {
        assert_eq!(
            split_html_space(" a\tb\nc\x0Cd\re ").collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(
            split_html_space("a\u{000b}b").collect::<Vec<_>>(),
            ["a\u{000b}b"]
        );
        assert_eq!(
            split_html_space("a\u{2003}b").collect::<Vec<_>>(),
            ["a\u{2003}b"]
        );
    }
}
