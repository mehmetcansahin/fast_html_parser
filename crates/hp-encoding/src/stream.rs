//! Streaming decoder for chunk-based processing.
//!
//! [`DecodingReader`] wraps a [`Read`](std::io::Read) source and decodes
//! bytes on-the-fly into UTF-8. This is useful for large inputs where
//! loading the entire document into memory is undesirable.

use std::io::{self, Read};

use encoding_rs::{Decoder, Encoding};

/// A streaming decoder that wraps a byte source and produces UTF-8 output.
///
/// Reads chunks from the inner reader, decodes them using the configured
/// encoding, and writes UTF-8 bytes into the caller's buffer.
///
/// # Example
///
/// ```
/// use hp_encoding::DecodingReader;
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

        if self.eof && self.raw_len == 0 {
            return Ok(());
        }

        // Read more raw bytes if needed.
        if self.raw_len == 0 && !self.eof {
            let n = self.inner.read(&mut self.raw_buf)?;
            if n == 0 {
                self.eof = true;
            } else {
                self.raw_len = n;
            }
        }

        // Decode the raw buffer.
        let (result, read, written, _had_errors) = self.decoder.decode_to_utf8(
            &self.raw_buf[..self.raw_len],
            &mut self.decoded_buf,
            self.eof,
        );

        // Shift unconsumed raw bytes to the front.
        if read < self.raw_len {
            self.raw_buf.copy_within(read..self.raw_len, 0);
            self.raw_len -= read;
        } else {
            self.raw_len = 0;
        }

        self.decoded_len = written;

        // If output_full, we need the caller to drain before decoding more.
        if let encoding_rs::CoderResult::OutputFull = result {
            // That's fine — caller will read, then we'll decode more.
        }

        Ok(())
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
}
