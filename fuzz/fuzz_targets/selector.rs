#![no_main]

use fhp_selector::matcher::match_selector_list;
use fhp_selector::parser::parse_selector;
use fhp_selector::{CompiledSelector, Selectable};
use fhp_tree::node::{NodeFlags, NodeId};
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

    // Selection pre-filters candidates before matching; it must return
    // exactly the elements a plain matcher scan accepts, in document order.
    let list = parse_selector(&selector).expect("the selector compiled");
    let arena = document.arena();
    let expected: Vec<NodeId> = document
        .root()
        .descendants()
        .filter(|&node| {
            let n = arena.get(node);
            n.depth > 0
                && !n
                    .flags
                    .has(NodeFlags::IS_TEXT | NodeFlags::IS_COMMENT | NodeFlags::IS_DOCTYPE)
        })
        .filter(|&node| match_selector_list(arena, node, &list))
        .collect();
    let actual: Vec<NodeId> = selection.iter().map(|node| node.id()).collect();
    assert_eq!(actual, expected, "{selector:?}");
    assert_eq!(
        document
            .select_first_compiled(&compiled)
            .ok()
            .flatten()
            .map(|node| node.id()),
        expected.first().copied()
    );
});
