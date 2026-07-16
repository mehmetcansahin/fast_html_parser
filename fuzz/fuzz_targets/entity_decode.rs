#![no_main]

use fhp_tokenizer::entity::{decode_attribute_entities, decode_entities};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let text = decode_entities(&input);
    let attribute = decode_attribute_entities(&input);

    assert!(text.len() <= input.len().saturating_mul(4));
    assert!(attribute.len() <= input.len().saturating_mul(4));
    assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    assert!(std::str::from_utf8(attribute.as_bytes()).is_ok());
});
