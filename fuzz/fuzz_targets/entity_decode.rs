#![no_main]

use std::borrow::Cow;

use fhp_tokenizer::entity::{
    decode_attribute_entities, decode_attribute_entities_into, decode_entities,
    decode_entities_into,
};
use libfuzzer_sys::fuzz_target;

/// `decode_entities_into` or `decode_attribute_entities_into`.
type DecodeInto = fn(&str, &mut String) -> bool;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let text = decode_entities(&input);
    let attribute = decode_attribute_entities(&input);

    assert!(text.len() <= input.len().saturating_mul(4));
    assert!(attribute.len() <= input.len().saturating_mul(4));
    assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    assert!(std::str::from_utf8(attribute.as_bytes()).is_ok());

    // The buffer API appends exactly what the `Cow` API returns owned, and
    // leaves the buffer untouched when it returns the input borrowed.
    const PREFIX: &str = "prefix";
    let pairs: [(Cow<'_, str>, DecodeInto); 2] = [
        (text, decode_entities_into),
        (attribute, decode_attribute_entities_into),
    ];
    for (decoded, into) in pairs {
        let mut buffer = String::from(PREFIX);
        let appended = into(&input, &mut buffer);
        match decoded {
            Cow::Borrowed(_) => assert!(!appended && buffer == PREFIX),
            Cow::Owned(expected) => {
                assert!(appended);
                assert_eq!(buffer[PREFIX.len()..], expected);
            }
        }
    }
});
