#![no_main]

//! Runs the tokenizer oracle shared with `fhp-tokenizer/tests/differential.rs`.
//!
//! Input layout: `[shift, chunking, html...]`. `shift % 64` is the block
//! shift; a `chunking` byte below 128 is a fixed chunk size, otherwise it
//! seeds random chunk lengths.

#[path = "../../crates/fhp-tokenizer/tests/oracle/mod.rs"]
mod oracle;

use libfuzzer_sys::fuzz_target;
use oracle::Chunking;

fuzz_target!(|data: &[u8]| {
    let [shift, chunking, html @ ..] = data else {
        return;
    };
    let chunking = if *chunking < 0x80 {
        Chunking::Fixed(usize::from(*chunking).max(1))
    } else {
        Chunking::Random(u64::from(*chunking))
    };
    oracle::check(
        &String::from_utf8_lossy(html),
        chunking,
        usize::from(*shift),
    );
});
