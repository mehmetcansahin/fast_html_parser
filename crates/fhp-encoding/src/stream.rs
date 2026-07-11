//! Streaming decoder for chunk-based processing.
//!
//! [`DecodingReader`] wraps a [`Read`](std::io::Read) source and decodes
//! bytes on-the-fly into UTF-8. This is useful for large inputs where
//! loading the entire document into memory is undesirable.

use std::io::{self, Read};

use encoding_rs::{CoderResult, Decoder, Encoding};

/// A streaming decoder that wraps a byte source and produces UTF-8 output.
///
/// Reads chunks from the inner reader, decodes them using the configured
/// encoding, and writes UTF-8 bytes into the caller's buffer.
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
    /// Raw bytes read from `inner` but not yet consumed by the decoder.
    raw_buf: Vec<u8>,
    /// Number of valid bytes in `raw_buf`.
    raw_len: usize,
    /// Decoded UTF-8 bytes ready to be returned to the caller.
    decoded_buf: Vec<u8>,
    /// Read cursor into `decoded_buf`.
    decoded_pos: usize,
    /// Number of valid decoded bytes in `decoded_buf`.
    decoded_len: usize,
    /// Whether the inner reader has reached EOF.
    eof: bool,
    /// Whether the decoder accepted the final input and must not be called again.
    finished: bool,
}

/// Default chunk size for reading from the inner source (8 KB).
const CHUNK_SIZE: usize = 8192;

impl<R: Read> DecodingReader<R> {
    /// Create a new streaming decoder with the given encoding.
    pub fn new(inner: R, encoding: &'static Encoding) -> Self {
        Self {
            inner,
            decoder: encoding.new_decoder(),
            raw_buf: vec![0u8; CHUNK_SIZE],
            raw_len: 0,
            decoded_buf: vec![0u8; CHUNK_SIZE * 4], // worst case: 4 bytes per input byte
            decoded_pos: 0,
            decoded_len: 0,
            eof: false,
            finished: false,
        }
    }

    /// Fill the decoded buffer by reading from the inner source and decoding.
    fn fill_decoded(&mut self) -> io::Result<()> {
        // If there's still data in the decoded buffer, don't refill.
        if self.decoded_pos < self.decoded_len {
            return Ok(());
        }

        // Reset decoded buffer.
        self.decoded_pos = 0;
        self.decoded_len = 0;

        if self.finished {
            return Ok(());
        }

        // Decode buffered bytes, reading more whenever a pass produces no
        // output. A single pass can yield zero bytes when the only buffered
        // bytes are an incomplete multi-byte sequence straddling a read
        // boundary (the decoder buffers them internally and emits nothing); in
        // that case we must read more from `inner` and decode again rather than
        // reporting a premature EOF.
        loop {
            let (result, read, written, _had_errors) = self.decoder.decode_to_utf8(
                &self.raw_buf[..self.raw_len],
                &mut self.decoded_buf,
                self.eof,
            );

            // Shift any unconsumed trailing bytes (e.g. an output-full
            // remainder) to the front of the raw buffer.
            if read < self.raw_len {
                self.raw_buf.copy_within(read..self.raw_len, 0);
                self.raw_len -= read;
            } else {
                self.raw_len = 0;
            }

            if self.eof && result == CoderResult::InputEmpty {
                self.finished = true;
            }

            if written > 0 {
                self.decoded_len = written;
                return Ok(());
            }

            // No output this pass. If the input is exhausted, we are done.
            if self.finished {
                return Ok(());
            }

            // Read more, appending after any leftover partial sequence so it
            // can be completed by the following bytes.
            if self.raw_len == self.raw_buf.len() {
                // Buffer full without a complete sequence (pathological): grow
                // it so the next read can make progress.
                self.raw_buf.resize(self.raw_buf.len() * 2, 0);
            }
            let n = self.inner.read(&mut self.raw_buf[self.raw_len..])?;
            if n == 0 {
                self.eof = true;
            } else {
                self.raw_len += n;
            }
        }
    }
}

impl<R: Read> Read for DecodingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.decoded_pos >= self.decoded_len {
            self.fill_decoded()?;
        }

        if self.decoded_pos >= self.decoded_len {
            return Ok(0); // EOF
        }

        let available = self.decoded_len - self.decoded_pos;
        let to_copy = available.min(buf.len());
        buf[..to_copy]
            .copy_from_slice(&self.decoded_buf[self.decoded_pos..self.decoded_pos + to_copy]);
        self.decoded_pos += to_copy;
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
