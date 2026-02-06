//! Encoding detection example.
//!
//! Demonstrates parsing raw bytes with different encodings.
//!
//! Run: `cargo run --example encoding_detect --features encoding`

use hp_parser::HtmlParser;
use hp_parser::encoding::detect;

fn main() {
    // UTF-8 (default)
    println!("=== UTF-8 ===");
    let utf8 = b"<p>Hello, world!</p>";
    let enc = detect(utf8);
    println!("Detected: {}", enc.name());
    let doc = HtmlParser::parse_bytes(utf8).unwrap();
    println!("Text: {}", doc.root().text_content());

    // UTF-8 with BOM
    println!("\n=== UTF-8 BOM ===");
    let bom_html = b"\xEF\xBB\xBF<p>BOM content</p>";
    let enc = detect(bom_html);
    println!("Detected: {}", enc.name());
    let doc = HtmlParser::parse_bytes(bom_html).unwrap();
    println!("Text: {}", doc.root().text_content());

    // Windows-1254 (Turkish) via meta charset
    println!("\n=== Windows-1254 ===");
    let mut w1254 = b"<meta charset=\"windows-1254\"><p>Merhaba d".to_vec();
    w1254.push(0xFC); // 'ü' in windows-1254
    w1254.extend_from_slice(b"nya</p>");
    let enc = detect(&w1254);
    println!("Detected: {}", enc.name());
    let doc = HtmlParser::parse_bytes(&w1254).unwrap();
    println!("Text: {}", doc.root().text_content());

    // UTF-16 LE with BOM
    println!("\n=== UTF-16 LE ===");
    let mut utf16le = vec![0xFF, 0xFE]; // BOM
    for &ch in b"<p>UTF-16</p>" {
        utf16le.push(ch);
        utf16le.push(0x00);
    }
    let enc = detect(&utf16le);
    println!("Detected: {}", enc.name());
    let doc = HtmlParser::parse_bytes(&utf16le).unwrap();
    println!("Text: {}", doc.root().text_content());
}
