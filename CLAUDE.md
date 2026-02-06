# HTML Parser — SIMD-Optimized

## Proje Planı
Detaylı plan: docs/PLAN.md dosyasında. Her faz için bu plana sadık kal.

## Kurallar
- Rust edition 2024 kullan
- Her public fonksiyona `///` doc comment yaz
- `#[inline]` ve `#[inline(always)]` kullanımına dikkat et — sadece hot path'lerde
- `unsafe` bloklar SAFETY comment ile açıklanmalı
- Her yeni modül için birim test yaz
- `cargo clippy -- -D warnings` her zaman temiz olmalı
- `cargo fmt` her zaman uygulanmış olmalı

## Kod Stili
- Error handling: `thiserror` ile custom error tipleri
- Naming: snake_case fonksiyonlar, PascalCase tipler
- SIMD kodları: her intrinsic'in ne yaptığını yorumla
- Benchmark: her yeni özellik criterion benchmark ile ölçülmeli

## Mevcut Durum
- [x] Faz 0: SIMD Abstraksiyon Katmanı
- [x] Faz 1: SIMD Tokenizer
- [x] Faz 2: Arena DOM Tree
- [x] Faz 3: Selector Engine
- [x] Faz 4: Encoding
- [x] Faz 5: Async/Streaming
- [x] Faz 6: API & Yayın

## Çalışma Prensibi
Her faz tamamlandığında:
1. Tüm testler geçmeli: `cargo test --workspace`
2. Clippy temiz olmalı: `cargo clippy --workspace -- -D warnings`
3. Benchmark çalıştır: `cargo bench`
4. CLAUDE.md'deki durumu güncelle
5. Git commit at: `git add -A && git commit -m "phase X: description"`
