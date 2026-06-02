//! Regression test: an unterminated raw-text element (`<script>`/`<style>`)
//! must not let the streaming residual buffer grow without bound.

use fhp_tokenizer::streaming::{MAX_RAW_TEXT_RESIDUAL, StreamTokenizer};

#[test]
fn unterminated_script_does_not_grow_residual_unbounded() {
    let mut tok = StreamTokenizer::new();
    let chunk = vec![b'a'; 64 * 1024]; // 64 KiB of script body, no `</script>`

    tok.feed(b"<script>");
    // Feed well past the hard cap.
    let total_chunks = (MAX_RAW_TEXT_RESIDUAL / chunk.len()) * 2 + 4;
    for _ in 0..total_chunks {
        tok.feed(&chunk);
    }

    assert!(
        tok.buffered_len() <= MAX_RAW_TEXT_RESIDUAL + chunk.len(),
        "residual {} exceeded the hard cap {} (+1 chunk)",
        tok.buffered_len(),
        MAX_RAW_TEXT_RESIDUAL
    );
}
