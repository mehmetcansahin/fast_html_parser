//! SIMD-accelerated HTML tokenizer.
//!
//! Uses a two-stage pipeline inspired by simdjson:
//!
//! 1. **Structural indexing** (SIMD): scan input in 64-byte blocks, produce
//!    per-delimiter bitmasks, then apply quote-aware masking.
//! 2. **Token extraction** (scalar): walk the structural index to emit tokens.
//!
//! This module implements stage 1 (structural indexer).

/// Structural character indexer — SIMD-powered bitmask generation.
pub mod structural;
