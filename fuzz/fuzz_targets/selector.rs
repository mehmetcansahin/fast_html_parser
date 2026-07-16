#![no_main]

use fhp_selector::{CompiledSelector, Selectable};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(separator) = data.iter().position(|byte| *byte == 0) else {
        return;
    };
    let selector = String::from_utf8_lossy(&data[..separator]);
    let html = String::from_utf8_lossy(&data[separator + 1..]);

    let Ok(compiled) = CompiledSelector::new(&selector) else {
        return;
    };
    let Ok(document) = fhp_tree::parse(&html) else {
        return;
    };
    let Ok(selection) = document.select_compiled(&compiled) else {
        return;
    };

    for node in selection.iter() {
        let _ = document.get(node.id());
    }
});
