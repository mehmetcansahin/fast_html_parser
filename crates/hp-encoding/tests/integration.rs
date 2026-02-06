//! Integration tests for encoding detection and conversion.

use hp_encoding::{DecodingReader, decode, decode_or_detect, detect};
use std::io::Read;

// ---------------------------------------------------------------------------
// UTF-8 tests
// ---------------------------------------------------------------------------

#[test]
fn utf8_no_bom() {
    let html = b"<html><body>Hello, world!</body></html>";
    let enc = detect(html);
    assert_eq!(enc.name(), "UTF-8");
    let text = decode(html, enc).unwrap();
    assert!(text.contains("Hello, world!"));
}

#[test]
fn utf8_with_bom() {
    let html = b"\xEF\xBB\xBF<html><body>BOM present</body></html>";
    let enc = detect(html);
    assert_eq!(enc.name(), "UTF-8");
    let text = decode(html, enc).unwrap();
    assert!(text.contains("BOM present"));
    // BOM itself should be stripped.
    assert!(!text.starts_with('\u{FEFF}'));
}

#[test]
fn utf8_multibyte_characters() {
    let html = "<html><body>Merhaba dünya! 你好世界 🌍</body></html>".as_bytes();
    let enc = detect(html);
    assert_eq!(enc.name(), "UTF-8");
    let text = decode(html, enc).unwrap();
    assert!(text.contains("dünya"));
    assert!(text.contains("你好世界"));
    assert!(text.contains("🌍"));
}

// ---------------------------------------------------------------------------
// ISO-8859-1 / Latin-1 tests (encoding_rs maps to windows-1252)
// ---------------------------------------------------------------------------

#[test]
fn latin1_cafe() {
    // "café" in ISO-8859-1 — 0xE9 is 'é'.
    let html: Vec<u8> =
        b"<html><head><meta charset=\"iso-8859-1\"></head><body>caf\xe9</body></html>".to_vec();
    let enc = detect(&html);
    // encoding_rs maps iso-8859-1 → windows-1252.
    assert_eq!(enc.name(), "windows-1252");
    let text = decode(&html, enc).unwrap();
    assert!(text.contains("café"));
}

#[test]
fn latin1_accented_chars() {
    // German umlauts: ä=0xE4, ö=0xF6, ü=0xFC, ß=0xDF
    let mut html = b"<meta charset=\"iso-8859-1\">".to_vec();
    html.extend_from_slice(&[0xE4, 0xF6, 0xFC, 0xDF]);
    let (text, enc) = decode_or_detect(&html).unwrap();
    assert_eq!(enc.name(), "windows-1252");
    assert!(text.contains("äöüß"));
}

// ---------------------------------------------------------------------------
// Windows-1252 tests
// ---------------------------------------------------------------------------

#[test]
fn windows_1252_smart_quotes() {
    // Windows-1252 "smart quotes": 0x93 = ", 0x94 = "
    let mut html = b"<meta charset=\"windows-1252\">".to_vec();
    html.extend_from_slice(&[0x93, b'H', b'e', b'l', b'l', b'o', 0x94]);
    let (text, enc) = decode_or_detect(&html).unwrap();
    assert_eq!(enc.name(), "windows-1252");
    assert!(text.contains('\u{201C}')); // "
    assert!(text.contains('\u{201D}')); // "
}

// ---------------------------------------------------------------------------
// Windows-1254 (Turkish) tests
// ---------------------------------------------------------------------------

#[test]
fn windows_1254_turkish_chars() {
    // Windows-1254 Turkish: ş=0xFE, ğ=0xF0, ı=0xFD, ö=0xF6, ü=0xFC, ç=0xE7
    let mut html = b"<meta charset=\"windows-1254\"><body>".to_vec();
    html.extend_from_slice(&[0xFE, 0xF0, 0xFD, 0xF6, 0xFC, 0xE7]); // şğıöüç
    html.extend_from_slice(b"</body>");
    let (text, enc) = decode_or_detect(&html).unwrap();
    assert_eq!(enc.name(), "windows-1254");
    assert!(text.contains('ş'));
    assert!(text.contains('ğ'));
    assert!(text.contains('ı'));
    assert!(text.contains('ö'));
    assert!(text.contains('ü'));
    assert!(text.contains('ç'));
}

#[test]
fn windows_1254_via_http_equiv() {
    let mut html =
        b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1254\">".to_vec();
    html.extend_from_slice(&[0xFE]); // ş
    let (text, enc) = decode_or_detect(&html).unwrap();
    assert_eq!(enc.name(), "windows-1254");
    assert!(text.contains('ş'));
}

// ---------------------------------------------------------------------------
// UTF-16 tests
// ---------------------------------------------------------------------------

#[test]
fn utf16le_bom() {
    // UTF-16 LE BOM + "<p>Hi</p>" in UTF-16 LE.
    let mut bytes = vec![0xFF, 0xFE]; // BOM
    for &ch in b"<p>Hi</p>" {
        bytes.push(ch);
        bytes.push(0x00);
    }
    let enc = detect(&bytes);
    assert_eq!(enc.name(), "UTF-16LE");
    let text = decode(&bytes, enc).unwrap();
    assert!(text.contains("Hi"), "text: {text}");
}

#[test]
fn utf16be_bom() {
    // UTF-16 BE BOM + "<p>Hi</p>" in UTF-16 BE.
    let mut bytes = vec![0xFE, 0xFF]; // BOM
    for &ch in b"<p>Hi</p>" {
        bytes.push(0x00);
        bytes.push(ch);
    }
    let enc = detect(&bytes);
    assert_eq!(enc.name(), "UTF-16BE");
    let text = decode(&bytes, enc).unwrap();
    assert!(text.contains("Hi"), "text: {text}");
}

#[test]
fn utf16le_turkish_no_ascii_content() {
    // UTF-16 LE BOM + Turkish chars.
    let mut bytes = vec![0xFF, 0xFE]; // BOM
    let text_chars = ['ş', 'ğ', 'ı', 'ö', 'ü', 'ç'];
    for ch in &text_chars {
        let mut buf = [0u16; 2];
        let encoded = ch.encode_utf16(&mut buf);
        for unit in encoded.iter() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let enc = detect(&bytes);
    assert_eq!(enc.name(), "UTF-16LE");
    let text = decode(&bytes, enc).unwrap();
    assert!(text.contains('ş'));
    assert!(text.contains('ğ'));
}

// ---------------------------------------------------------------------------
// Meta charset detection tests
// ---------------------------------------------------------------------------

#[test]
fn meta_charset_detected() {
    let html = b"<html><head><meta charset=\"windows-1252\"></head><body>ok</body></html>";
    assert_eq!(detect(html).name(), "windows-1252");
}

#[test]
fn meta_charset_mixed_case() {
    let html = b"<HTML><HEAD><META CHARSET=\"Windows-1252\"></HEAD></HTML>";
    assert_eq!(detect(html).name(), "windows-1252");
}

#[test]
fn meta_charset_with_other_attrs() {
    let html = b"<meta name=\"viewport\" content=\"width=device-width\"><meta charset=\"utf-8\">";
    assert_eq!(detect(html).name(), "UTF-8");
}

// ---------------------------------------------------------------------------
// Fallback / edge cases
// ---------------------------------------------------------------------------

#[test]
fn no_encoding_info_fallback_utf8() {
    let html = b"<html><body>Plain HTML without any encoding hints</body></html>";
    assert_eq!(detect(html).name(), "UTF-8");
}

#[test]
fn empty_input_fallback() {
    assert_eq!(detect(b"").name(), "UTF-8");
    let (text, enc) = decode_or_detect(b"").unwrap();
    assert_eq!(text, "");
    assert_eq!(enc.name(), "UTF-8");
}

#[test]
fn just_bom_no_content() {
    let html = b"\xEF\xBB\xBF";
    let enc = detect(html);
    assert_eq!(enc.name(), "UTF-8");
    let text = decode(html, enc).unwrap();
    assert_eq!(text, "");
}

// ---------------------------------------------------------------------------
// Streaming tests
// ---------------------------------------------------------------------------

#[test]
fn streaming_decode_utf8() {
    let html = b"<html><body>Hello streaming!</body></html>";
    let mut reader = DecodingReader::new(&html[..], encoding_rs::UTF_8);
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
    assert!(output.contains("Hello streaming!"));
}

#[test]
fn streaming_decode_windows_1254() {
    // ş=0xFE, ğ=0xF0 in Windows-1254.
    let data: &[u8] = &[0xFE, 0xF0];
    let mut reader = DecodingReader::new(data, encoding_rs::WINDOWS_1254);
    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
    assert_eq!(output, "şğ");
}
