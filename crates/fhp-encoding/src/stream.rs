//! Streaming decoder for chunk-based processing.
//!
//! [`DecodingReader`] wraps a [`Read`] source and decodes
//! bytes on-the-fly into UTF-8. This is useful for large inputs where
//! loading the entire document into memory is undesirable.
//!
//! [`Read`]: std::io::Read

use std::io::{self, Read};

use encoding_rs::{CoderResult, Decoder, Encoding};

/// A streaming decoder that wraps a byte source and produces UTF-8 output.
///
/// Reads chunks from the inner reader, decodes them using the configured
/// encoding, and writes UTF-8 bytes into the caller's buffer. BOMs and
/// malformed sequences are handled exactly like [`decode`](crate::decode()): a
/// BOM matching the encoding is stripped, and any other BOM is decoded as
/// content with the configured encoding rather than re-sniffed.
///
/// # Example
///
/// ```
/// use fhp_encoding::DecodingReader;
/// use std::io::Read;
///
/// let data = b"Hello, world!";
/// let mut reader = DecodingReader::new(&data[..], encoding_rs::UTF_8);
/// let mut output = String::new();
/// reader.read_to_string(&mut output).unwrap();
/// assert_eq!(output, "Hello, world!");
/// ```
pub struct DecodingReader<R> {
    inner: R,
    decoder: Decoder,
    /// Raw bytes read from `inner`; `raw_buf[raw_pos..raw_len]` is not yet
    /// consumed by the decoder.
    raw_buf: Vec<u8>,
    /// Start of the unconsumed bytes in `raw_buf`.
    raw_pos: usize,
    /// End of the valid bytes in `raw_buf`.
    raw_len: usize,
    /// Decoded output for a caller buffer smaller than `MIN_DIRECT_OUTPUT`
    /// that has not been returned yet.
    pending: [u8; MIN_DIRECT_OUTPUT],
    /// Read cursor into `pending`.
    pending_pos: usize,
    /// Number of valid bytes in `pending`.
    pending_len: usize,
    /// Whether the inner reader has reached EOF.
    eof: bool,
    /// Whether the decoder accepted the final input and must not be called again.
    finished: bool,
}

/// Default chunk size for reading from the inner source (8 KB).
const CHUNK_SIZE: usize = 8192;

/// Smallest output buffer `encoding_rs` accepts when decoding to UTF-8. It
/// always fits at least one decoded character, so a pass into it that writes
/// nothing has consumed all of its input.
const MIN_DIRECT_OUTPUT: usize = 4;

impl<R: Read> DecodingReader<R> {
    /// Create a new streaming decoder with the given encoding.
    pub fn new(inner: R, encoding: &'static Encoding) -> Self {
        Self {
            inner,
            decoder: encoding.new_decoder_with_bom_removal(),
            raw_buf: vec![0u8; CHUNK_SIZE],
            raw_pos: 0,
            raw_len: 0,
            pending: [0; MIN_DIRECT_OUTPUT],
            pending_pos: 0,
            pending_len: 0,
            eof: false,
            finished: false,
        }
    }

    /// Decode into `out` (at least `MIN_DIRECT_OUTPUT` bytes), reading from
    /// the inner source as needed. Returns 0 only at the end of the stream.
    fn decode_into(&mut self, out: &mut [u8]) -> io::Result<usize> {
        debug_assert!(out.len() >= MIN_DIRECT_OUTPUT);
        // A pass can yield zero bytes when the only buffered bytes are an
        // incomplete multi-byte sequence straddling a read boundary (the
        // decoder buffers them internally and emits nothing); then read more
        // from `inner` and decode again rather than reporting a premature EOF.
        while !self.finished {
            let (result, read, written, _had_errors) = self.decoder.decode_to_utf8(
                &self.raw_buf[self.raw_pos..self.raw_len],
                out,
                self.eof,
            );
            self.raw_pos += read;
            if result == CoderResult::InputEmpty {
                self.finished = self.eof;
            }
            if written > 0 {
                return Ok(written);
            }

            debug_assert_eq!(result, CoderResult::InputEmpty);
            if !self.finished {
                let n = self.inner.read(&mut self.raw_buf)?;
                self.raw_pos = 0;
                self.raw_len = n;
                self.eof = n == 0;
            }
        }
        Ok(0)
    }
}

impl<R: Read> Read for DecodingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        if self.pending_pos == self.pending_len {
            if buf.len() >= MIN_DIRECT_OUTPUT {
                // Decode straight into the caller's buffer.
                return self.decode_into(buf);
            }
            let mut pending = [0; MIN_DIRECT_OUTPUT];
            self.pending_len = self.decode_into(&mut pending)?;
            self.pending = pending;
            self.pending_pos = 0;
        }

        let available = &self.pending[self.pending_pos..self.pending_len];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        self.pending_pos += to_copy;
        Ok(to_copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_utf8() {
        let data = b"Hello, world!";
        let mut reader = DecodingReader::new(&data[..], encoding_rs::UTF_8);
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        assert_eq!(output, "Hello, world!");
    }

    #[test]
    fn stream_windows_1254_turkish() {
        // ş=0xFE, ğ=0xF0, ı=0xFD in Windows-1254
        let data: &[u8] = &[0xFE, 0xF0, 0xFD];
        let mut reader = DecodingReader::new(data, encoding_rs::WINDOWS_1254);
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        assert_eq!(output, "\u{015F}\u{011F}\u{0131}"); // ş, ğ, ı
    }

    #[test]
    fn stream_empty() {
        let data: &[u8] = b"";
        let mut reader = DecodingReader::new(data, encoding_rs::UTF_8);
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        assert_eq!(output, "");
    }

    #[test]
    fn stream_large_input() {
        // Create input larger than CHUNK_SIZE to test multi-chunk decoding.
        let data = "abcdefgh".repeat(2000); // 16KB
        let mut reader = DecodingReader::new(data.as_bytes(), encoding_rs::UTF_8);
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        assert_eq!(output, data);
    }

    #[test]
    fn stream_small_read_buf() {
        let data = b"Hello, world!";
        let mut reader = DecodingReader::new(&data[..], encoding_rs::UTF_8);
        let mut output = Vec::new();
        let mut buf = [0u8; 3]; // Read 3 bytes at a time.
        loop {
            let n = reader.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert_eq!(String::from_utf8(output).unwrap(), "Hello, world!");
    }

    /// Inner reader that yields a single byte per `read` call, forcing
    /// multi-byte sequences to straddle read boundaries.
    struct OneByteReader<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl Read for OneByteReader<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.data[self.pos];
            self.pos += 1;
            Ok(1)
        }
    }

    #[test]
    fn stream_multibyte_split_across_reads() {
        // Multi-byte UTF-8 chars whose bytes arrive one at a time. A lead byte
        // leaves a partial sequence buffered; the reader must read more rather
        // than report a premature EOF.
        let text = "cafe\u{301} gu\u{308}naydin nai\u{308}ve \u{1F600}";
        let reader = OneByteReader {
            data: text.as_bytes(),
            pos: 0,
        };
        let mut out = String::new();
        DecodingReader::new(reader, encoding_rs::UTF_8)
            .read_to_string(&mut out)
            .unwrap();
        assert_eq!(out, text);
    }

    #[test]
    fn stream_multibyte_across_chunk_boundary() {
        // A multi-byte char positioned so its bytes straddle the 8 KB raw-read
        // boundary must not be dropped.
        let mut text = "a".repeat(CHUNK_SIZE - 1);
        text.push('\u{20AC}'); // euro sign: 3 UTF-8 bytes, splits across 8192
        text.push_str(&"b".repeat(100));
        let mut out = String::new();
        DecodingReader::new(text.as_bytes(), encoding_rs::UTF_8)
            .read_to_string(&mut out)
            .unwrap();
        assert_eq!(out, text);
    }

    #[test]
    fn stream_malformed_utf8_at_eof_is_replaced_once() {
        let data: &[u8] = &[0xFF];
        let mut reader = DecodingReader::new(data, encoding_rs::UTF_8);
        let mut output = String::new();

        reader.read_to_string(&mut output).unwrap();

        assert_eq!(output, "\u{FFFD}");
        let mut byte = [0u8; 1];
        assert_eq!(reader.read(&mut byte).unwrap(), 0);
        assert_eq!(reader.read(&mut byte).unwrap(), 0);
    }

    fn read_with_buffer_len(data: &[u8], encoding: &'static Encoding, len: usize) -> Vec<u8> {
        let mut reader = DecodingReader::new(OneByteReader { data, pos: 0 }, encoding);
        let mut output = Vec::new();
        let mut buf = vec![0u8; len];
        loop {
            let n = reader.read(&mut buf).unwrap();
            if n == 0 {
                return output;
            }
            output.extend_from_slice(&buf[..n]);
        }
    }

    #[test]
    fn stream_any_buffer_len_yields_the_same_text() {
        let text = "a\u{e9}\u{20AC}\u{1F600}b \u{131}\u{15F}";
        for len in 1..=9 {
            let output = read_with_buffer_len(text.as_bytes(), encoding_rs::UTF_8, len);
            assert_eq!(String::from_utf8(output).unwrap(), text, "len={len}");
        }
        // Windows-1254: each byte decodes to a 1- to 3-byte UTF-8 sequence.
        let legacy: &[u8] = &[b'a', 0xFE, 0xF0, 0xFD, 0x80, b'z'];
        for len in 1..=5 {
            let output = read_with_buffer_len(legacy, encoding_rs::WINDOWS_1254, len);
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "a\u{15F}\u{11F}\u{131}\u{20AC}z",
                "len={len}"
            );
        }
    }

    #[test]
    fn stream_matches_decode_for_boms() {
        let cases: [(&[u8], &'static Encoding); 6] = [
            (b"\xEF\xBB\xBFa", encoding_rs::UTF_8),
            (b"\xEF\xBB\xBFa", encoding_rs::WINDOWS_1252),
            (b"\xFF\xFEa\x00", encoding_rs::UTF_16LE),
            (b"\xFF\xFEa\x00", encoding_rs::UTF_8),
            (b"\xFE\xFF\x00a", encoding_rs::UTF_16BE),
            (b"\xFE\xFF\x00a", encoding_rs::UTF_16LE),
        ];
        for (data, encoding) in cases {
            let mut output = String::new();
            DecodingReader::new(data, encoding)
                .read_to_string(&mut output)
                .unwrap();
            assert_eq!(
                output,
                crate::decode(data, encoding),
                "data={data:?}, encoding={}",
                encoding.name()
            );
        }
    }

    #[test]
    fn stream_strips_a_bom_split_across_reads() {
        let output = read_with_buffer_len(b"\xEF\xBB\xBFok", encoding_rs::UTF_8, 8);
        assert_eq!(output, b"ok");
    }

    #[test]
    fn stream_empty_read_does_not_touch_the_source() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("source must not be read"))
            }
        }

        let mut reader = DecodingReader::new(FailingReader, encoding_rs::UTF_8);
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        assert!(reader.read(&mut [0u8; 8]).is_err());
    }

    #[test]
    fn stream_truncated_utf8_at_eof_is_replaced_once() {
        let data: &[u8] = &[0xE2, 0x82];
        let mut reader = DecodingReader::new(data, encoding_rs::UTF_8);
        let mut output = String::new();

        reader.read_to_string(&mut output).unwrap();

        assert_eq!(output, "\u{FFFD}");
        let mut byte = [0u8; 1];
        assert_eq!(reader.read(&mut byte).unwrap(), 0);
    }
}
