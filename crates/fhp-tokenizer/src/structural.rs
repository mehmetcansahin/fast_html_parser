//! Structural character indexer for HTML input.
//!
//! Scans input in 64-byte blocks using SIMD dispatch, producing u64 bitmasks
//! for the four delimiters the extraction stage dispatches on: `<`, `>`, `"`
//! and `'`. The index is context-free: delimiters inside quoted attribute
//! values, comments, or raw text are indexed too, and the scalar extraction
//! stage decides which of them are structural.
//!
//! This is stage 1 (SIMD delimiter index) of the two-stage tokenizer
//! pipeline (see crate docs).

use fhp_simd::dispatch::{SimdOps, ops};

/// Size of each processing block in bytes.
const BLOCK_SIZE: usize = 64;

/// Per-block bitmasks for each delimiter the extraction stage dispatches on.
///
/// Bit `i` of a mask is set if the byte at position `i` within the block
/// matches that delimiter.
#[derive(Clone, Debug, Default)]
pub struct BlockBitmaps {
    /// `<` positions.
    pub lt: u64,
    /// `>` positions.
    pub gt: u64,
    /// `"` positions.
    pub quot: u64,
    /// `'` positions.
    pub apos: u64,
}

/// Result of structural indexing: a sequence of [`BlockBitmaps`] covering
/// the entire input.
pub struct StructuralIndex {
    bitmaps: Vec<BlockBitmaps>,
    len: usize,
}

impl StructuralIndex {
    /// Iterate over all structural delimiter positions in input order.
    pub fn iter_delimiters(&self) -> DelimiterIter<'_> {
        let mut iter = DelimiterIter {
            bitmaps: &self.bitmaps,
            block_idx: 0,
            combined: 0,
            len: self.len,
        };
        // Load the first block's combined mask.
        if !self.bitmaps.is_empty() {
            iter.combined = Self::combined_mask(&self.bitmaps[0]);
        }
        iter
    }

    /// Estimated number of tokens — used for `Vec` pre-allocation.
    ///
    /// Heuristic: roughly one token per pair of `<`/`>` delimiters,
    /// plus text segments between them.
    pub fn estimated_token_count(&self) -> usize {
        let total_lt: u32 = self.bitmaps.iter().map(|b| b.lt.count_ones()).sum();
        // Each `<` roughly corresponds to one tag token. Add 1 for trailing text.
        total_lt as usize + 1
    }

    /// Total input length in bytes.
    pub fn input_len(&self) -> usize {
        self.len
    }

    /// Number of 64-byte blocks.
    pub fn block_count(&self) -> usize {
        self.bitmaps.len()
    }

    /// Access the bitmaps for a specific block.
    pub fn block(&self, index: usize) -> &BlockBitmaps {
        &self.bitmaps[index]
    }

    /// OR of all delimiter masks for a block.
    fn combined_mask(block: &BlockBitmaps) -> u64 {
        block.lt | block.gt | block.quot | block.apos
    }
}

/// A structural delimiter found by the indexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelimiterEntry {
    /// Absolute byte offset in the input.
    pub pos: usize,
    /// The delimiter byte at this position.
    pub byte: u8,
}

/// Iterator over structural delimiter positions, yielded in ascending order.
pub struct DelimiterIter<'a> {
    bitmaps: &'a [BlockBitmaps],
    block_idx: usize,
    combined: u64,
    len: usize,
}

impl<'a> Iterator for DelimiterIter<'a> {
    type Item = DelimiterEntry;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.combined != 0 {
                let bit = self.combined.trailing_zeros() as usize;
                // Clear lowest set bit.
                self.combined &= self.combined - 1;

                let pos = self.block_idx * BLOCK_SIZE + bit;
                if pos >= self.len {
                    return None;
                }

                let block = &self.bitmaps[self.block_idx];
                let byte = determine_byte(block, bit);
                return Some(DelimiterEntry { pos, byte });
            }

            // Advance to next non-empty block.
            self.block_idx += 1;
            if self.block_idx >= self.bitmaps.len() {
                return None;
            }
            self.combined = StructuralIndex::combined_mask(&self.bitmaps[self.block_idx]);
        }
    }
}

/// Determine which delimiter byte is at bit position `bit` within `block`.
#[inline]
fn determine_byte(block: &BlockBitmaps, bit: usize) -> u8 {
    let mask = 1u64 << bit;
    if block.lt & mask != 0 {
        b'<'
    } else if block.gt & mask != 0 {
        b'>'
    } else if block.quot & mask != 0 {
        b'"'
    } else {
        b'\''
    }
}

/// SIMD-powered structural character indexer.
///
/// Scans input in 64-byte blocks, producing one bitmask per delimiter
/// (`<`, `>`, `"`, `'`). Every occurrence is indexed; the extraction stage
/// decides which positions are structural.
///
/// # Example
///
/// ```
/// use fhp_tokenizer::structural::StructuralIndexer;
///
/// let indexer = StructuralIndexer::new();
/// let index = indexer.index(b"<div class=\"foo\">bar</div>");
///
/// let delimiters: Vec<_> = index.iter_delimiters().collect();
/// assert!(delimiters.len() > 0);
/// ```
pub struct StructuralIndexer {
    dispatch: &'static SimdOps,
}

impl StructuralIndexer {
    /// Create a new indexer using the auto-detected SIMD backend.
    pub fn new() -> Self {
        Self { dispatch: ops() }
    }

    /// Scan `input` and produce a [`StructuralIndex`].
    ///
    /// The input is processed in 64-byte blocks. Each block's
    /// [`BlockBitmaps`] comes from a single SIMD `compute_all_masks` pass.
    pub fn index(&self, input: &[u8]) -> StructuralIndex {
        let block_count = input.len().div_ceil(BLOCK_SIZE);
        let mut bitmaps = Vec::with_capacity(block_count);
        let compute = self.dispatch.compute_all_masks;

        for chunk in input.chunks(BLOCK_SIZE) {
            // SAFETY: dispatch function pointers are initialized from backends
            // that match the detected CPU features.
            let masks = unsafe { compute(chunk) };
            bitmaps.push(BlockBitmaps {
                lt: masks.lt,
                gt: masks.gt,
                quot: masks.quot,
                apos: masks.apos,
            });
        }

        StructuralIndex {
            bitmaps,
            len: input.len(),
        }
    }
}

impl Default for StructuralIndexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // StructuralIndexer — basic HTML
    // ---------------------------------------------------------------

    #[test]
    fn basic_html_tag() {
        let input = b"<div>hello</div>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();

        // Expected delimiters: 0: '<', 4: '>', 10: '<', 15: '>'. The `/` of
        // the end tag is not an indexed delimiter.
        assert_eq!(
            delims,
            vec![
                DelimiterEntry { pos: 0, byte: b'<' },
                DelimiterEntry { pos: 4, byte: b'>' },
                DelimiterEntry {
                    pos: 10,
                    byte: b'<'
                },
                DelimiterEntry {
                    pos: 15,
                    byte: b'>'
                },
            ]
        );
    }

    #[test]
    fn html_with_attributes() {
        let input = b"<div class=\"foo\">bar</div>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();

        // Only `<`, `>`, `"` and `'` are indexed; `=` and `/` are not.
        let entries: Vec<(usize, u8)> = delims.iter().map(|d| (d.pos, d.byte)).collect();
        assert_eq!(
            entries,
            vec![
                (0, b'<'),
                (11, b'"'),
                (15, b'"'),
                (16, b'>'),
                (20, b'<'),
                (25, b'>'),
            ]
        );
    }

    // ---------------------------------------------------------------
    // Delimiter indexing (quote semantics belong to the parser)
    // ---------------------------------------------------------------

    #[test]
    fn delimiters_inside_double_quotes_are_indexed() {
        // The indexer yields ALL delimiter positions, including those inside
        // an attribute value. Quote semantics (a `>` inside a value does not
        // close the tag) are the parser's job, not the indexer's — see the
        // `tests/quote_handling.rs` integration tests.
        let input = b"<a title=\"x > y < z\">link</a>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let positions: Vec<usize> = index.iter_delimiters().map(|d| d.pos).collect();

        // Structural delimiters of the tag itself.
        assert!(positions.contains(&0)); // opening '<'
        assert!(positions.contains(&20)); // closing '>'
        assert!(positions.contains(&25)); // '<' of </a>

        // Delimiters inside the quoted value are also indexed now.
        assert!(positions.contains(&12), "> inside quotes is indexed");
        assert!(positions.contains(&16), "< inside quotes is indexed");
    }

    #[test]
    fn delimiters_inside_single_quotes_are_indexed() {
        let input = b"<a title='x > y'>link</a>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let positions: Vec<usize> = index.iter_delimiters().map(|d| d.pos).collect();

        // '>' at 12 inside single quotes is indexed; the parser decides it is
        // not a tag-closing '>'.
        assert!(positions.contains(&12), "> inside single quotes is indexed");
    }

    #[test]
    fn single_quote_inside_double_quotes_ignored() {
        // The ' inside "it's" is indexed like any other quote byte; it does
        // not hide the tag-closing '>'.
        let input = b"<div title=\"it's\">text</div>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();
        let bytes: Vec<u8> = delims.iter().map(|d| d.byte).collect();

        // Should have structural delimiters for the tag structure.
        assert!(bytes.contains(&b'<'));
        assert!(bytes.contains(&b'>'));
        // The tag should close properly — '>' after the closing quote.
        let gt_positions: Vec<usize> = delims
            .iter()
            .filter(|d| d.byte == b'>')
            .map(|d| d.pos)
            .collect();
        assert!(
            gt_positions.contains(&17),
            "closing > at 17 should be structural"
        );
    }

    // ---------------------------------------------------------------
    // Edge cases
    // ---------------------------------------------------------------

    #[test]
    fn empty_input() {
        let indexer = StructuralIndexer::new();
        let index = indexer.index(b"");

        assert_eq!(index.input_len(), 0);
        assert_eq!(index.block_count(), 0);
        assert_eq!(index.iter_delimiters().count(), 0);
        assert_eq!(index.estimated_token_count(), 1);
    }

    #[test]
    fn text_only_no_delimiters() {
        let input = b"hello world this is plain text without any tags";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        assert_eq!(index.iter_delimiters().count(), 0);
        assert_eq!(index.estimated_token_count(), 1);
    }

    #[test]
    fn tag_only() {
        let input = b"<br/>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();
        assert_eq!(
            delims,
            vec![
                DelimiterEntry { pos: 0, byte: b'<' },
                DelimiterEntry { pos: 4, byte: b'>' },
            ]
        );
    }

    // ---------------------------------------------------------------
    // Long input (crosses 64-byte block boundaries)
    // ---------------------------------------------------------------

    #[test]
    fn long_input_multiple_blocks() {
        // Build a 1000+ byte input with tags at various positions.
        let mut input = Vec::with_capacity(1200);
        // Fill with text.
        input.extend_from_slice(&[b'x'; 100]);
        // Tag at offset 100.
        input.extend_from_slice(b"<div>");
        // More text.
        input.extend_from_slice(&[b'y'; 200]);
        // Tag at offset 305.
        input.extend_from_slice(b"<span class=\"test\">");
        // More text.
        input.extend_from_slice(&[b'z'; 700]);
        // Closing tags.
        input.extend_from_slice(b"</span></div>");

        let indexer = StructuralIndexer::new();
        let index = indexer.index(&input);

        assert!(input.len() > 1000);
        assert!(index.block_count() > 15);

        let delims: Vec<_> = index.iter_delimiters().collect();

        // Verify '<' at offset 100.
        assert!(
            delims.iter().any(|d| d.pos == 100 && d.byte == b'<'),
            "should find < at offset 100"
        );

        // The quotes around the attribute value are indexed.
        assert!(
            delims.iter().any(|d| d.pos == 317 && d.byte == b'"'),
            "should find opening quote at offset 317"
        );
        assert!(
            delims.iter().any(|d| d.pos == 322 && d.byte == b'"'),
            "should find closing quote at offset 322"
        );

        // Verify there are closing tags at the end.
        let last_lt = delims.iter().rev().find(|d| d.byte == b'<');
        assert!(last_lt.is_some());
    }

    #[test]
    fn long_input_with_quotes_spanning_blocks() {
        // Attribute value that spans a 64-byte block boundary.
        let mut input = Vec::new();
        input.extend_from_slice(b"<div data=\"");
        // Pad to 60 bytes total (attribute value starts at offset 11).
        while input.len() < 60 {
            input.push(b'a');
        }
        // Add a '<' inside the string at offset 60 (inside a block boundary region).
        input.push(b'<');
        // Continue the string value past the 64-byte boundary.
        while input.len() < 80 {
            input.push(b'b');
        }
        // Close the attribute and tag.
        input.extend_from_slice(b"\">end</div>");

        let indexer = StructuralIndexer::new();
        let index = indexer.index(&input);

        let delims: Vec<_> = index.iter_delimiters().collect();
        let lt_positions: Vec<usize> = delims
            .iter()
            .filter(|d| d.byte == b'<')
            .map(|d| d.pos)
            .collect();

        // The '<' at offset 60 (inside the value, across a block boundary) is
        // indexed; the parser, tracking quote state, will not treat it as a
        // tag start.
        assert!(
            lt_positions.contains(&60),
            "< at offset 60 is indexed (parser handles quote state)"
        );

        // The opening '<' at 0 is present too.
        assert!(lt_positions.contains(&0), "opening < should be structural");
    }

    // ---------------------------------------------------------------
    // estimated_token_count
    // ---------------------------------------------------------------

    #[test]
    fn estimated_token_count_basic() {
        let input = b"<div>hello</div><p>world</p>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        // 4 '<' characters → estimate = 5.
        assert_eq!(index.estimated_token_count(), 5);
    }

    // ---------------------------------------------------------------
    // StructuralIndex API
    // ---------------------------------------------------------------

    #[test]
    fn block_api() {
        let input = b"<div>text</div>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        assert_eq!(index.input_len(), 15);
        assert_eq!(index.block_count(), 1);

        let block = index.block(0);
        assert_ne!(block.lt, 0, "should have < bits set");
        assert_ne!(block.gt, 0, "should have > bits set");
    }
}
