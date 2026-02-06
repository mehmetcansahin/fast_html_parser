//! Structural character indexer for HTML input.
//!
//! Scans input in 64-byte blocks using SIMD dispatch, producing per-delimiter
//! u64 bitmasks. Quote-aware masking (prefix XOR) ensures that delimiters
//! inside quoted attribute values are not treated as structural.
//!
//! This is stage 1 of the two-stage tokenizer pipeline (see crate docs).

use hp_simd::dispatch::{SimdOps, ops};

/// Size of each processing block in bytes.
const BLOCK_SIZE: usize = 64;

/// Per-block bitmasks for each HTML delimiter type.
///
/// Bit `i` of a mask is set if the byte at position `i` within the block
/// matches that delimiter. After quote-aware masking, non-structural
/// positions (inside quoted strings) are cleared from the relevant masks.
#[derive(Clone, Debug, Default)]
pub struct BlockBitmaps {
    /// `<` positions.
    pub lt: u64,
    /// `>` positions.
    pub gt: u64,
    /// `&` positions.
    pub amp: u64,
    /// `"` positions.
    pub quot: u64,
    /// `'` positions.
    pub apos: u64,
    /// `=` positions.
    pub eq: u64,
    /// `/` positions.
    pub slash: u64,
    /// Positions inside quoted strings (both `"` and `'`).
    /// Set bits indicate bytes that are *not* structural.
    pub in_string: u64,
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

    /// OR of all delimiter masks for a block (excluding `in_string`).
    fn combined_mask(block: &BlockBitmaps) -> u64 {
        block.lt | block.gt | block.amp | block.quot | block.apos | block.eq | block.slash
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
    if (block.lt >> bit) & 1 == 1 {
        b'<'
    } else if (block.gt >> bit) & 1 == 1 {
        b'>'
    } else if (block.amp >> bit) & 1 == 1 {
        b'&'
    } else if (block.quot >> bit) & 1 == 1 {
        b'"'
    } else if (block.apos >> bit) & 1 == 1 {
        b'\''
    } else if (block.eq >> bit) & 1 == 1 {
        b'='
    } else {
        b'/'
    }
}

/// SIMD-powered structural character indexer.
///
/// Scans input in 64-byte blocks, producing bitmasks for each delimiter
/// type. Quote-aware masking ensures delimiters inside `"..."` or `'...'`
/// are not flagged as structural.
///
/// # Example
///
/// ```
/// use hp_tokenizer::structural::StructuralIndexer;
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
    /// The input is processed in 64-byte blocks. For each block, a
    /// [`BlockBitmaps`] is produced with bitmasks for every HTML delimiter.
    /// Quote-aware masking is then applied to clear non-structural positions.
    pub fn index(&self, input: &[u8]) -> StructuralIndex {
        let block_count = input.len().div_ceil(BLOCK_SIZE);
        let mut bitmaps = Vec::with_capacity(block_count);

        for chunk in input.chunks(BLOCK_SIZE) {
            // SAFETY: dispatch function pointers are initialized from backends
            // that match the detected CPU features.
            let compute = self.dispatch.compute_byte_mask;
            let lt = unsafe { compute(chunk, b'<') };
            let gt = unsafe { compute(chunk, b'>') };
            let amp = unsafe { compute(chunk, b'&') };
            let quot = unsafe { compute(chunk, b'"') };
            let apos = unsafe { compute(chunk, b'\'') };
            let eq = unsafe { compute(chunk, b'=') };
            let slash = unsafe { compute(chunk, b'/') };

            bitmaps.push(BlockBitmaps {
                lt,
                gt,
                amp,
                quot,
                apos,
                eq,
                slash,
                in_string: 0,
            });
        }

        Self::compute_string_masks(&mut bitmaps);

        StructuralIndex {
            bitmaps,
            len: input.len(),
        }
    }

    /// Compute quote-aware string masks using prefix XOR.
    ///
    /// Two passes:
    /// 1. Double-quote (`"`) regions via prefix XOR + carry.
    /// 2. Single-quote (`'`) regions, ignoring positions already inside
    ///    double-quoted strings.
    ///
    /// After both passes, non-structural delimiters inside strings are
    /// cleared from `lt`, `gt`, `amp`, `eq`, `slash`, and the non-boundary
    /// quote masks.
    fn compute_string_masks(bitmaps: &mut [BlockBitmaps]) {
        // Pass 1: double-quote regions.
        let mut carry_dq = false;
        for block in bitmaps.iter_mut() {
            let flipped = prefix_xor(block.quot);
            let dq_in = if carry_dq { !flipped } else { flipped };
            carry_dq ^= (block.quot.count_ones() % 2) == 1;
            // Store dq_in temporarily in `in_string`.
            block.in_string = dq_in;
        }

        // Pass 2: single-quote regions (excluding positions inside dquot).
        let mut carry_sq = false;
        for block in bitmaps.iter_mut() {
            let dq_in = block.in_string;
            // Ignore single quotes that fall inside double-quoted strings.
            let effective_apos = block.apos & !dq_in;
            let flipped = prefix_xor(effective_apos);
            let sq_in = if carry_sq { !flipped } else { flipped };
            carry_sq ^= (effective_apos.count_ones() % 2) == 1;

            // Combine both quote regions.
            block.in_string = dq_in | sq_in;

            // Mask non-structural delimiters inside strings.
            block.lt &= !block.in_string;
            block.gt &= !block.in_string;
            block.amp &= !block.in_string;
            block.eq &= !block.in_string;
            block.slash &= !block.in_string;

            // Mask non-boundary quotes (apos inside dquot, quot inside squot).
            block.apos &= !dq_in;
            block.quot &= !sq_in;
        }
    }
}

impl Default for StructuralIndexer {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute prefix XOR of all bits in a `u64`.
///
/// After this operation, bit `i` equals the XOR of the original bits
/// `0..=i`. This effectively creates a toggle mask: each set bit in the
/// input flips all subsequent bits, turning quote positions into
/// inside/outside-string regions.
///
/// On x86_64 with PCLMULQDQ this could be a single `clmul` instruction;
/// here we use a portable cascade of shifts.
#[inline]
fn prefix_xor(mut x: u64) -> u64 {
    x ^= x << 1;
    x ^= x << 2;
    x ^= x << 4;
    x ^= x << 8;
    x ^= x << 16;
    x ^= x << 32;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // prefix_xor unit tests
    // ---------------------------------------------------------------

    #[test]
    fn prefix_xor_single_bit() {
        // Bit 2 set → bits 2..63 should be set after prefix XOR.
        let result = prefix_xor(1 << 2);
        for i in 0..64 {
            let bit = (result >> i) & 1;
            if i < 2 {
                assert_eq!(bit, 0, "bit {i} should be 0");
            } else {
                assert_eq!(bit, 1, "bit {i} should be 1");
            }
        }
    }

    #[test]
    fn prefix_xor_two_bits() {
        // Bits 2 and 5 → bits 2..4 should be 1, bits 5..63 should be 0.
        let result = prefix_xor((1 << 2) | (1 << 5));
        for i in 0..64 {
            let bit = (result >> i) & 1;
            if (2..5).contains(&i) {
                assert_eq!(bit, 1, "bit {i} should be 1");
            } else {
                assert_eq!(bit, 0, "bit {i} should be 0");
            }
        }
    }

    #[test]
    fn prefix_xor_zero() {
        assert_eq!(prefix_xor(0), 0);
    }

    // ---------------------------------------------------------------
    // StructuralIndexer — basic HTML
    // ---------------------------------------------------------------

    #[test]
    fn basic_html_tag() {
        let input = b"<div>hello</div>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();

        // Expected structural delimiters:
        // 0: '<', 4: '>', 10: '<', 11: '/', 15: '>'
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
                    pos: 11,
                    byte: b'/'
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

        // '<' at 0, '=' at 10, '"' at 11, '"' at 15, '>' at 16,
        // '<' at 20, '/' at 21, '>' at 25
        // Note: "foo" is inside quotes, but the content "foo" has no delimiters.
        let bytes: Vec<u8> = delims.iter().map(|d| d.byte).collect();
        assert!(bytes.contains(&b'<'));
        assert!(bytes.contains(&b'>'));
        assert!(bytes.contains(&b'='));
        assert!(bytes.contains(&b'"'));
    }

    // ---------------------------------------------------------------
    // Quote-aware masking
    // ---------------------------------------------------------------

    #[test]
    fn delimiters_inside_double_quotes_masked() {
        // The < and > inside the attribute value should NOT appear.
        let input = b"<a title=\"x > y < z\">link</a>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();
        let positions: Vec<usize> = delims.iter().map(|d| d.pos).collect();

        // '<' at 0 (opening tag)
        assert!(positions.contains(&0));
        // '>' at 20 (closing >)
        assert!(positions.contains(&20));
        // '<' at 25 (closing tag </a>)
        assert!(positions.contains(&25));

        // The '>' at 12 and '<' at 16 should NOT be present (inside quotes).
        assert!(!positions.contains(&12), "> inside quotes should be masked");
        assert!(!positions.contains(&16), "< inside quotes should be masked");
    }

    #[test]
    fn delimiters_inside_single_quotes_masked() {
        let input = b"<a title='x > y'>link</a>";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let delims: Vec<_> = index.iter_delimiters().collect();
        let positions: Vec<usize> = delims.iter().map(|d| d.pos).collect();

        // '>' at 12 inside single quotes should NOT be present.
        assert!(
            !positions.contains(&12),
            "> inside single quotes should be masked"
        );
    }

    #[test]
    fn single_quote_inside_double_quotes_ignored() {
        // The ' inside "it's" should not affect masking.
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
                DelimiterEntry { pos: 3, byte: b'/' },
                DelimiterEntry { pos: 4, byte: b'>' },
            ]
        );
    }

    #[test]
    fn entity_reference() {
        let input = b"a &amp; b";
        let indexer = StructuralIndexer::new();
        let index = indexer.index(input);

        let amp_entries: Vec<_> = index.iter_delimiters().filter(|d| d.byte == b'&').collect();
        assert_eq!(amp_entries.len(), 1);
        assert_eq!(amp_entries[0].pos, 2);
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

        // Verify delimiters inside "test" are masked.
        // The attribute value "test" contains no delimiters, so nothing to mask.
        // But the = and " around it should be structural.
        assert!(
            delims.iter().any(|d| d.byte == b'='),
            "should find = in attribute"
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

        // The '<' at offset 60 should be masked (inside double-quoted string).
        assert!(
            !lt_positions.contains(&60),
            "< at offset 60 should be masked (inside quoted string)"
        );

        // The opening '<' at 0 and closing '<' should be present.
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
