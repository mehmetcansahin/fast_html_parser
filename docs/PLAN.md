# 🦀⚡ Rust HTML Parser — SIMD-Optimized Geliştirme Planı

## Tasarım Felsefesi

> **"Her CPU cycle hesap verir."**
> Portable scalar fallback her zaman çalışır. SIMD katmanı runtime dispatch ile
> devreye girer ve aynı algoritmayı 8-32x paralel çalıştırır.

## Proje Özeti

| Özellik | Karar |
|---|---|
| **Amaç** | Web scraping, bleeding-edge hız |
| **SIMD** | Opsiyonel feature, runtime dispatch |
| **Platformlar** | x86_64 (SSE4.2 / AVX2) + ARM (NEON) |
| **Bellek** | Zero-copy, arena-alloc, cache-line aligned |
| **Trade-off** | Hız > API ergonomisi |
| **Dağıtım** | crates.io library |

---

## Neden Mevcut Rust Parser'lar Yeterli Değil?

| Crate | Zayıf Noktası |
|---|---|
| `html5ever` | Tam spec uyumu nedeniyle yavaş, çok fazla allocation |
| `scraper` | `html5ever` üzerine kurulu, selector engine optimize değil |
| `tl` | Hızlı ama SIMD yok, XPath yok, bozuk HTML toleransı sınırlı |
| `lol_html` | Streaming odaklı, tree oluşturmuyor, selector desteği minimal |
| `quick-xml` | XML parser, HTML toleransı yok |

**Hedef:** `tl`'nin hızını SIMD ile aşmak + `scraper`'ın özellik zenginliğini sunmak.

---

## Workspace Yapısı

```
html-parser/
├── Cargo.toml                      # [workspace]
├── crates/
│   ├── hp-core/                    # Ortak tipler, interned tags, entity tablosu
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── tag.rs              # Interned tag enum (u8 → tag mapping)
│   │       ├── entity.rs           # PHF entity tablosu
│   │       └── error.rs
│   ├── hp-simd/                    # SIMD abstraksiyon katmanı
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── dispatch.rs         # Runtime feature detection + dispatch
│   │       ├── scalar.rs           # Portable fallback
│   │       ├── sse42.rs            # SSE4.2 intrinsics
│   │       ├── avx2.rs             # AVX2 intrinsics
│   │       └── neon.rs             # ARM NEON intrinsics
│   ├── hp-tokenizer/               # SIMD-accelerated tokenizer
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scanner.rs          # SIMD byte scanner
│   │       ├── state_machine.rs    # Branchless state transitions
│   │       ├── token.rs            # Token tipleri
│   │       └── streaming.rs        # Chunk-based feed API
│   ├── hp-tree/                    # Arena-based DOM tree
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── arena.rs            # Cache-aligned arena allocator
│   │       ├── node.rs             # Flat node layout
│   │       ├── builder.rs          # Tree construction
│   │       └── traverse.rs         # Iterator'lar
│   ├── hp-selector/                # CSS selector + XPath engine
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── css/
│   │       │   ├── parser.rs       # Selector → AST
│   │       │   ├── matcher.rs      # Right-to-left matching
│   │       │   └── bloom.rs        # Bloom filter acceleration
│   │       └── xpath/
│   │           ├── parser.rs
│   │           └── eval.rs
│   ├── hp-encoding/                # Encoding detection + dönüşüm
│   └── fast-html-parser/          # Facade crate
│       └── src/
│           ├── lib.rs              # Public API
│           └── config.rs           # Builder pattern
├── benches/
│   ├── tokenizer_bench.rs
│   ├── tree_bench.rs
│   ├── selector_bench.rs
│   └── e2e_bench.rs
├── fuzz/
│   ├── fuzz_tokenizer.rs
│   └── fuzz_selector.rs
└── testdata/
    ├── small_1kb.html
    ├── medium_100kb.html
    ├── large_5mb.html
    └── malformed/
```

---

## Faz 0 — SIMD Abstraksiyon Katmanı (~2 hafta)

Tüm pipeline buna bağlı. Önce bu yazılmalı.

### 0.1 Runtime Feature Detection + Dispatch

```rust
// hp-simd/src/dispatch.rs

/// Desteklenen SIMD seviyesi
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimdLevel {
    Scalar,   // Her zaman çalışır
    Sse42,    // x86_64 — 128-bit
    Avx2,     // x86_64 — 256-bit
    Neon,     // aarch64 — 128-bit
}

/// Uygulama başında bir kez çağrılır, sonuç static'te cache'lenir.
/// std::arch::is_x86_feature_detected! runtime'da CPUID ile kontrol eder.
pub fn detect() -> SimdLevel {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") { return SimdLevel::Avx2; }
        if is_x86_feature_detected!("sse4.2") { return SimdLevel::Sse42; }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // NEON aarch64'te her zaman mevcut
        return SimdLevel::Neon;
    }
    SimdLevel::Scalar
}

/// Function pointer tabanlı dispatch (vtable overhead yok)
pub struct SimdOps {
    pub find_delimiters: unsafe fn(haystack: &[u8]) -> DelimiterResult,
    pub classify_bytes:  unsafe fn(input: &[u8], lut: &[u8; 16]) -> Vec<u8>,
}

static SIMD_OPS: OnceLock<SimdOps> = OnceLock::new();

pub fn ops() -> &'static SimdOps {
    SIMD_OPS.get_or_init(|| match detect() {
        SimdLevel::Avx2  => SimdOps { find_delimiters: avx2::find_delimiters,  .. },
        SimdLevel::Sse42 => SimdOps { find_delimiters: sse42::find_delimiters, .. },
        SimdLevel::Neon  => SimdOps { find_delimiters: neon::find_delimiters,  .. },
        SimdLevel::Scalar => SimdOps { find_delimiters: scalar::find_delimiters, .. },
    })
}
```

### 0.2 Temel SIMD Operasyonları

Her backend (scalar, SSE4.2, AVX2, NEON) için implement edilecek **4 temel operasyon:**

#### Op 1: Multi-Delimiter Scan
Bir byte slice içinde `<`, `>`, `&`, `"`, `'`, `=`, `/`, `\n` gibi birden fazla delimiter'ı **tek geçişte** bul.

```rust
// AVX2 versiyonu — 32 byte paralel
#[target_feature(enable = "avx2")]
unsafe fn find_delimiters_avx2(input: &[u8]) -> DelimiterResult {
    // '<' = 0x3C, '>' = 0x3E, '&' = 0x26, '"' = 0x22
    let lt = _mm256_set1_epi8(b'<' as i8);
    let gt = _mm256_set1_epi8(b'>' as i8);
    let amp = _mm256_set1_epi8(b'&' as i8);
    let quot = _mm256_set1_epi8(b'"' as i8);

    let mut offset = 0;
    while offset + 32 <= input.len() {
        let chunk = _mm256_loadu_si256(input[offset..].as_ptr() as *const __m256i);

        let cmp_lt = _mm256_cmpeq_epi8(chunk, lt);
        let cmp_gt = _mm256_cmpeq_epi8(chunk, gt);
        let cmp_amp = _mm256_cmpeq_epi8(chunk, amp);
        let cmp_quot = _mm256_cmpeq_epi8(chunk, quot);

        let combined = _mm256_or_si256(
            _mm256_or_si256(cmp_lt, cmp_gt),
            _mm256_or_si256(cmp_amp, cmp_quot),
        );

        let mask = _mm256_movemask_epi8(combined) as u32;
        if mask != 0 {
            let pos = offset + mask.trailing_zeros() as usize;
            return DelimiterResult::Found { pos, byte: input[pos] };
        }
        offset += 32;
    }
    // Kalan byte'lar için scalar fallback
    scalar::find_delimiters(&input[offset..]).offset_by(offset)
}
```

**Scalar fallback** aynı mantığı byte-by-byte yapar. `memchr` bile burada yetersiz çünkü tek seferde yalnızca 1-3 byte arıyor — biz 4-8 delimiter'ı paralel arıyoruz.

#### Op 2: Byte Classification (Shuffle-based LUT)
Her byte'ı kategoriye ayır: whitespace, alpha, digit, delimiter, diğer.

```rust
// VPSHUFB tabanlı — 32 byte'ı tek instruction'da sınıflandır
#[target_feature(enable = "avx2")]
unsafe fn classify_bytes_avx2(input: &[u8], lut_lo: __m256i, lut_hi: __m256i) -> __m256i {
    let chunk = _mm256_loadu_si256(input.as_ptr() as *const __m256i);
    let nibble_lo = _mm256_and_si256(chunk, _mm256_set1_epi8(0x0F));
    let nibble_hi = _mm256_and_si256(
        _mm256_srli_epi16(chunk, 4),
        _mm256_set1_epi8(0x0F)
    );
    let class_lo = _mm256_shuffle_epi8(lut_lo, nibble_lo);
    let class_hi = _mm256_shuffle_epi8(lut_hi, nibble_hi);
    _mm256_and_si256(class_lo, class_hi)
}
```

Bu teknik `simdjson`'un kullandığı yaklaşım — byte classification'ı O(1) instruction'da yapar.

#### Op 3: Whitespace Skip
Art arda gelen whitespace'leri 32-byte bloklar halinde atla.

#### Op 4: Tag Name Compare
Interned tag isimleriyle karşılaştırmayı SIMD ile yap (8-16 byte'lık tag isimleri tek `_mm_cmpeq_epi8` ile).

### 0.3 Benchmark Altyapısı
- Her operasyonun scalar vs SSE4.2 vs AVX2 vs NEON karşılaştırması
- `criterion` ile cycle-level ölçüm
- `perf stat` / `cachegrind` entegrasyonu için Makefile target'ları

---

## Faz 1 — SIMD-Accelerated Tokenizer (~4 hafta)

### 1.1 Mimari: İki Aşamalı Pipeline

Tokenizer iki aşamadan oluşur — bu `simdjson`'un kanıtladığı en hızlı yaklaşım:

```
Aşama 1 (SIMD): Structural Index Oluşturma
  Input bytes → delimiter pozisyonlarının bit index'i
  32-64 byte/cycle throughput

Aşama 2 (Scalar): Token Extraction
  Structural index → Token stream
  Branch-heavy ama input çok küçülmüş durumda
```

#### Aşama 1: Structural Character Indexing

HTML'deki "structural character"lar: `<`, `>`, `/`, `=`, `"`, `'`, `&`

```rust
/// SIMD ile structural character pozisyonlarını bitset olarak üret.
/// Her 64-byte blok için bir u64 bitmask döner.
pub struct StructuralIndexer {
    dispatch: &'static SimdOps,
}

impl StructuralIndexer {
    pub fn index(&self, input: &[u8]) -> StructuralIndex {
        let mut bitmaps = StructuralBitmaps::with_capacity(input.len() / 64 + 1);

        // SIMD: 64 byte'lık bloklar halinde tara
        // Her blok için hangi pozisyonlarda '<', '>', '"' vb. var → u64 bitmask
        for (i, chunk) in input.chunks(64).enumerate() {
            let lt_mask  = (self.dispatch.find_byte_mask)(chunk, b'<');
            let gt_mask  = (self.dispatch.find_byte_mask)(chunk, b'>');
            let amp_mask = (self.dispatch.find_byte_mask)(chunk, b'&');
            let quot_mask = (self.dispatch.find_byte_mask)(chunk, b'"');

            bitmaps.push(BlockBitmaps {
                lt: lt_mask,
                gt: gt_mask,
                amp: amp_mask,
                quot: quot_mask,
                in_string: 0,  // Aşama 1.5'te hesaplanacak
            });
        }

        // String literal masking: quote'lar arasındaki delimiter'ları iptal et
        self.compute_string_masks(&mut bitmaps);

        StructuralIndex { bitmaps, len: input.len() }
    }
}
```

#### Aşama 1.5: Quote-Aware Masking (Kritik!)
Attribute value'lar içindeki `<`, `>` gibi karakterler structural değil. Quote pairing'i `prefix XOR sum` (carry-less multiplication) ile SIMD'de çözülür:

```rust
/// Her 64-byte blok için in-string mask hesapla.
/// '"' pozisyonlarının cumulative XOR'u string içi/dışı durumunu verir.
fn compute_string_masks(&self, bitmaps: &mut [BlockBitmaps]) {
    let mut in_string = false;
    for block in bitmaps.iter_mut() {
        // prefix_xor: clmul instruction ile O(1)'de hesaplanır (x86)
        // veya scalar'da bit manipulation ile
        let mut mask = block.quot;
        let flipped = prefix_xor(mask);
        if in_string { block.in_string = !flipped; }
        else { block.in_string = flipped; }
        in_string ^= (mask.count_ones() % 2) == 1;

        // String içindeki structural char'ları iptal et
        block.lt  &= !block.in_string;
        block.gt  &= !block.in_string;
        block.amp &= !block.in_string;
    }
}
```

#### Aşama 2: Token Extraction

Structural index üzerinde scalar loop — ama input artık sadece delimiter pozisyonları, çok küçük.

```rust
pub fn extract_tokens<'a>(input: &'a [u8], index: &StructuralIndex) -> Vec<Token<'a>> {
    let mut tokens = Vec::with_capacity(index.estimated_token_count());
    let mut pos = 0;

    // Branchless state machine: state geçişleri lookup table ile
    let mut state = State::Data;

    for delim in index.iter_delimiters() {
        // Text between delimiters
        if delim.pos > pos {
            // ... text token emit
        }

        // State transition: LUT[current_state][delimiter_byte] → (new_state, action)
        let (new_state, action) = STATE_TABLE[state as usize][delim.byte_class as usize];
        match action {
            Action::EmitOpenTag  => { /* ... */ },
            Action::EmitCloseTag => { /* ... */ },
            Action::EmitAttr     => { /* ... */ },
            Action::None         => {},
        }
        state = new_state;
        pos = delim.pos + 1;
    }

    tokens
}
```

### 1.2 Branchless State Machine

Branch misprediction HTML parsing'de büyük performans kaybı yaratır. Çözüm:

```rust
/// State × ByteClass → (NewState, Action)
/// Compile-time'da oluşturulan 2D lookup table.
/// Branch yok — sadece array index.
const STATE_TABLE: [[Transition; 16]; 16] = {
    let mut table = [[Transition::NOOP; 16]; 16];

    // Data state
    table[State::Data as usize][ByteClass::Lt as usize] =
        Transition { state: State::TagOpen, action: Action::FlushText };
    table[State::Data as usize][ByteClass::Amp as usize] =
        Transition { state: State::EntityRef, action: Action::None };

    // TagOpen state
    table[State::TagOpen as usize][ByteClass::Alpha as usize] =
        Transition { state: State::TagName, action: Action::StartTag };
    table[State::TagOpen as usize][ByteClass::Slash as usize] =
        Transition { state: State::EndTagOpen, action: Action::None };
    table[State::TagOpen as usize][ByteClass::Bang as usize] =
        Transition { state: State::MarkupDecl, action: Action::None };

    // ... tüm geçişler compile-time'da doldurulur
    table
};

struct Transition {
    state: State,
    action: Action,
}
```

### 1.3 Interned Tag Names

String karşılaştırma yerine `u8` karşılaştırma — 10-50x daha hızlı:

```rust
// hp-core/src/tag.rs

/// Bilinen HTML tag'leri. u8 olarak saklanır → comparison tek instruction.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tag {
    // Void elements
    Area = 0, Base, Br, Col, Embed, Hr, Img, Input, Link, Meta,
    Param, Source, Track, Wbr,
    // Common elements
    A, Abbr, Article, Aside, B, Body, Button, Div, Em, Footer,
    Form, H1, H2, H3, H4, H5, H6, Head, Header, Html, I, Iframe,
    Li, Main, Nav, Ol, P, Pre, Script, Section, Select, Span,
    Strong, Style, Table, Tbody, Td, Textarea, Th, Thead, Title,
    Tr, Ul, Video,
    // Bilinmeyen tag'ler
    Unknown = 255,
}

impl Tag {
    /// PHF perfect hash ile O(1) lookup.
    /// Büyük/küçük harf duyarsız (lowercase'e çevirmeden!).
    #[inline(always)]
    pub fn from_bytes(name: &[u8]) -> Tag {
        // Önce length-based fast reject
        if name.len() > MAX_KNOWN_TAG_LEN { return Tag::Unknown; }

        // PHF lookup (compile-time generated)
        TAG_PHF_MAP.get(name).copied().unwrap_or(Tag::Unknown)
    }

    /// Void element mi? Branch-free bit check.
    #[inline(always)]
    pub const fn is_void(self) -> bool {
        // İlk 14 tag void — tek comparison yeterli
        (self as u8) < 14
    }
}
```

### 1.4 Entity Decoding — Lazy Cow

Entity olmayan string'lere zero allocation, entity olanlar `Cow::Owned`:

```rust
/// Entity decode — sadece '&' bulunursa allocation yapar.
#[inline]
pub fn decode_entities<'a>(input: &'a str) -> Cow<'a, str> {
    // SIMD ile '&' var mı kontrolü — yoksa anında dön
    if !contains_amp_simd(input.as_bytes()) {
        return Cow::Borrowed(input);
    }
    // Varsa scalar decode
    Cow::Owned(decode_entities_slow(input))
}
```

### 1.5 Streaming / Chunk-Based Feed

```rust
pub struct StreamTokenizer {
    state: State,
    residual: ArrayVec<u8, 64>,  // Chunk sınırında kalan kısmi token (stack-alloc)
    structural_carry: bool,       // Önceki chunk'tan kalan string state
}

impl StreamTokenizer {
    /// Chunk besle, callback ile token'ları emit et.
    /// Allocation: SIFIR (callback inline'lanır, token'lar stack'te yaşar)
    #[inline]
    pub fn feed<'a, F>(&mut self, chunk: &'a [u8], mut emit: F)
    where
        F: FnMut(Token<'a>),
    {
        // Residual + yeni chunk birleştir (gerekirse)
        // SIMD structural index → token extraction → callback
    }
}
```

**Faz 1 Çıktısı:** 2-4 GB/s tokenizer throughput (AVX2'de). Scalar fallback ~500 MB/s.

---

## Faz 2 — Cache-Optimized Arena DOM Tree (~3 hafta)

### 2.1 Cache-Line Aligned Arena

```rust
/// 64-byte aligned arena. Her Node tam bir cache line'a oturur.
/// False sharing yok, prefetch friendly.
#[repr(C, align(64))]
pub struct Node {
    // === İlk 32 byte: sık erişilen veriler (hot) ===
    pub tag: Tag,                    //  1 byte
    pub flags: NodeFlags,            //  1 byte  (is_void, has_children, has_attrs)
    pub depth: u16,                  //  2 byte
    pub parent: NodeId,              //  4 byte  (u32 — 4 milyar node yeterli)
    pub first_child: NodeId,         //  4 byte
    pub next_sibling: NodeId,        //  4 byte
    pub last_child: NodeId,          //  4 byte
    pub prev_sibling: NodeId,        //  4 byte
    pub text_offset: u32,            //  4 byte  (text slab'daki pozisyon)
    pub text_len: u32,               //  4 byte

    // === İkinci 32 byte: nadir erişilen veriler (cold) ===
    pub attr_offset: u32,            //  4 byte  (attribute slab'daki pozisyon)
    pub attr_count: u8,              //  1 byte
    pub _padding: [u8; 27],          // 27 byte  (toplam 64 byte)
}

pub struct Arena {
    nodes: Vec<Node>,                // Cache-friendly contiguous memory
    text_slab: Vec<u8>,              // Tüm text content burada (tek allocation)
    attr_slab: Vec<Attribute>,       // Tüm attribute'lar burada (tek allocation)
}

/// Node referansı — u32 yeterli, usize'dan yarı boyut.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const NULL: NodeId = NodeId(u32::MAX);
}
```

### 2.2 Flat Attribute Storage

Her node için ayrı `Vec<Attribute>` yerine tek slab:

```rust
/// Attribute'lar tek contiguous buffer'da.
/// Node.attr_offset + Node.attr_count ile O(1) erişim.
pub struct Attribute<'a> {
    pub name: &'a str,         // Zero-copy — input buffer'a referans
    pub value: Cow<'a, str>,   // Entity decode gerekirse Owned
}

impl Arena {
    #[inline]
    pub fn attrs(&self, node: NodeId) -> &[Attribute] {
        let n = &self.nodes[node.0 as usize];
        let start = n.attr_offset as usize;
        let end = start + n.attr_count as usize;
        &self.attr_slab[start..end]
    }
}
```

### 2.3 Tree Builder — Branchless Implicit Close

```rust
/// HTML'de bazı tag'ler öncekini otomatik kapatır.
/// Örn: <p> içinde <p> gelirse önceki <p> kapanır.
/// Bu kuralları LUT ile branchless uygula.
const IMPLICIT_CLOSE: [[bool; 64]; 64] = {
    let mut table = [[false; 64]; 64];
    // <p> açıkken <p> gelirse kapat
    table[Tag::P as usize][Tag::P as usize] = true;
    // <li> açıkken <li> gelirse kapat
    table[Tag::Li as usize][Tag::Li as usize] = true;
    // <td> açıkken <td> veya <th> gelirse kapat
    table[Tag::Td as usize][Tag::Td as usize] = true;
    table[Tag::Td as usize][Tag::Th as usize] = true;
    // ... diğer kurallar
    table
};
```

### 2.4 Traversal Iterator'ları — Allocation-Free

```rust
/// Depth-first traversal — stack yerine tree link'leri kullanır, allocation yok.
pub struct DepthFirst<'a> {
    arena: &'a Arena,
    current: NodeId,
    root: NodeId,
}

impl<'a> Iterator for DepthFirst<'a> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.current == NodeId::NULL { return None; }
        let node = &self.arena.nodes[self.current.0 as usize];
        let result = self.current;

        // Çocuk varsa → ilk çocuğa git
        // Yoksa → sibling'e git
        // O da yoksa → parent'a çıkıp sibling ara
        self.current = if node.first_child != NodeId::NULL {
            node.first_child
        } else {
            self.next_non_child(result)
        };

        Some(result)
    }
}
```

**Faz 2 Çıktısı:** Cache-line aligned, zero-allocation traversal, 64-byte node layout.

---

## Faz 3 — Selector Engine (~4 hafta)

### 3.1 CSS Selector — Compiled + Cached

```rust
/// Selector'ı bir kez parse et, NFA/bytecode'a derle, tekrar kullan.
pub struct CompiledSelector {
    /// Bytecode: her instruction 8 byte
    instructions: Vec<SelectorOp>,
    /// Bloom filter seed: ancestor check hızlandırma
    bloom_hashes: Vec<u32>,
}

#[repr(u8)]
enum SelectorOp {
    MatchTag(Tag),                       // tag == X?
    MatchClass { hash: u32 },            // class bloom check → exact match
    MatchId { offset: u32, len: u16 },   // id == interned string?
    MatchAttr { name_hash: u32 },        // attr varlık kontrolü
    MatchAttrValue { op: AttrOp, .. },   // attr value comparison
    Combinator(CombinatorKind),          // >, +, ~, descendant
    PseudoNthChild { a: i32, b: i32 },   // :nth-child(an+b)
    PseudoNot { offset: u16 },           // :not() — iç selector'a jump
    Accept,                              // Eşleşme başarılı
    Reject,                              // Eşleşme başarısız
}
```

### 3.2 Right-to-Left Matching + Bloom Filter

```rust
/// Bloom filter: tree build sırasında her node'un ancestor chain'ini hash'le.
/// Selector matching'de "div .foo" sorgusu geldiğinde, önce bloom filter
/// ile "ancestor'da div var mı?" kontrolü yap — %99 false positive olmadan elenebilir.
pub struct AncestorBloom {
    bits: [u64; 4],  // 256-bit bloom filter — stack allocated
}

impl AncestorBloom {
    #[inline(always)]
    pub fn may_contain(&self, hash: u32) -> bool {
        let bit = hash as usize % 256;
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        self.bits[word] & mask != 0
    }
}
```

### 3.3 XPath Alt Kümesi

Scraping'de en çok kullanılan XPath operasyonları:

```rust
pub enum XPathExpr<'a> {
    /// //tag
    DescendantByTag(Tag),
    /// //tag[@attr='value']
    DescendantByAttr { tag: Tag, attr: &'a str, value: &'a str },
    /// /path/to/tag
    AbsolutePath(Vec<PathStep<'a>>),
    /// //tag[contains(@attr, 'substr')]
    ContainsPredicate { tag: Tag, attr: &'a str, substr: &'a str },
    /// //tag[position()=N]
    PositionPredicate { tag: Tag, pos: usize },
    /// //tag/text()
    TextExtract(Box<XPathExpr<'a>>),
}
```

### 3.4 Convenience API

```rust
impl Document<'_> {
    fn select(&self, css: &str) -> Result<Selection, SelectorError>;
    fn xpath(&self, expr: &str) -> Result<Selection, XPathError>;
    fn find_tag(&self, tag: Tag) -> Selection;              // O(n) scan
    fn find_by_id(&self, id: &str) -> Option<NodeId>;       // Hash lookup — O(1)
    fn find_by_class(&self, class: &str) -> Selection;      // Bloom pre-filter
    fn find_by_attr(&self, name: &str, value: &str) -> Selection;
}

impl Selection<'_> {
    fn first(&self) -> Option<NodeRef>;
    fn iter(&self) -> impl Iterator<Item = NodeRef>;
    fn text(&self) -> String;
    fn attr(&self, name: &str) -> Option<&str>;
    fn inner_html(&self) -> String;
    fn select(&self, css: &str) -> Result<Selection, SelectorError>; // Chaining
}
```

**Faz 3 Çıktısı:** Compiled selector, bloom-filtered matching, XPath desteği.

---

## Faz 4 — Encoding Katmanı (~1-2 hafta)

### 4.1 Tasarım

```rust
/// Encoding pipeline: raw bytes → UTF-8 → tokenizer
/// encoding_rs zaten SIMD-optimized, tekrar yazmaya gerek yok.
pub struct EncodingDetector<'a> {
    input: &'a [u8],
}

impl<'a> EncodingDetector<'a> {
    pub fn detect(&self) -> &'static encoding_rs::Encoding {
        // 1. BOM check (3 byte)
        // 2. İlk 1KB'da <meta charset="..."> pre-scan (SIMD ile '<' ve 'm' ara)
        // 3. Fallback: UTF-8
    }
}
```

`encoding_rs` Mozilla/Servo tarafından SIMD-optimize edilmiş durumda. Bunu sarmalayıp tokenizer'a entegre etmek yeterli.

**Faz 4 Çıktısı:** Otomatik encoding detection, SIMD-powered dönüşüm.

---

## Faz 5 — Async + Streaming Entegrasyonu (~2-3 hafta)

### 5.1 Async Trait Abstraction

```rust
/// Runtime-agnostic async reader trait.
/// tokio::AsyncRead veya async-std::AsyncRead bunu impl eder.
pub trait AsyncChunkReader {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> io::Result<usize>;
}

/// Streaming parse — her chunk geldiğinde tree'yi kademeli inşa et.
pub struct AsyncParser<R: AsyncChunkReader> {
    reader: R,
    tokenizer: StreamTokenizer,
    builder: TreeBuilder,
    config: ParserConfig,
}

impl<R: AsyncChunkReader> AsyncParser<R> {
    pub async fn parse(mut self) -> Result<Document, ParseError> {
        let mut buf = AlignedBuf::new(64 * 1024); // 64KB aligned buffer

        loop {
            let n = self.reader.read_chunk(&mut buf).await?;
            if n == 0 { break; }

            self.tokenizer.feed(&buf[..n], |token| {
                self.builder.process(token);
            });
        }

        Ok(self.builder.finish())
    }
}
```

### 5.2 Early Termination

```rust
/// Aranan element bulunduğunda parse'ı durduran wrapper.
pub struct EarlyStopParser<'a> {
    inner: StreamTokenizer,
    builder: TreeBuilder,
    predicate: Box<dyn Fn(&Node) -> bool + 'a>,
    found: Option<NodeId>,
}

impl EarlyStopParser<'_> {
    pub fn feed(&mut self, chunk: &[u8]) -> ParseStatus {
        self.inner.feed(chunk, |token| {
            let node_id = self.builder.process(token);
            if let Some(id) = node_id {
                let node = self.builder.arena.get(id);
                if (self.predicate)(node) {
                    self.found = Some(id);
                    return; // Early termination
                }
            }
        });

        if self.found.is_some() {
            ParseStatus::Found(self.found.unwrap())
        } else {
            ParseStatus::NeedMore
        }
    }
}
```

**Faz 5 Çıktısı:** Tokio/async-std uyumlu streaming parser, early termination.

---

## Faz 6 — API, Benchmark ve Yayın (~2 hafta)

### 6.1 Feature Flags

```toml
[features]
default = ["css-selector", "entity-decode"]
simd = []                                   # SIMD acceleration (runtime dispatch)
css-selector = []
xpath = ["dep:hp-selector"]
encoding = ["dep:encoding_rs"]
async-tokio = ["dep:tokio"]
async-async-std = ["dep:async-std"]
entity-decode = []
```

### 6.2 Competitive Benchmark Suite

```rust
// benches/comparison.rs — criterion

fn bench_tokenize(c: &mut Criterion) {
    let html = include_str!("../testdata/large_5mb.html");

    let mut group = c.benchmark_group("tokenize_5mb");
    group.throughput(Throughput::Bytes(html.len() as u64));

    // Bizim parser
    group.bench_function("hp (scalar)", |b| { /* ... */ });
    group.bench_function("hp (simd)",   |b| { /* ... */ });

    // Rakipler
    group.bench_function("tl",          |b| { /* ... */ });
    group.bench_function("html5ever",   |b| { /* ... */ });
    group.bench_function("lol_html",    |b| { /* ... */ });
}
```

### 6.3 Hedef Performans Metrikleri

| Metrik | Hedef (AVX2) | Hedef (Scalar) |
|---|---|---|
| Tokenizer throughput | 2-4 GB/s | 400-800 MB/s |
| Tree build (1MB HTML) | < 2 ms | < 8 ms |
| CSS selector match (1000 node doc) | < 50 µs | < 200 µs |
| End-to-end parse (100KB) | < 200 µs | < 600 µs |
| Peak memory (1MB HTML) | < 4 MB | < 4 MB |

---

## Tahmini Zaman Çizelgesi

| Faz | Süre | Kümülatif | Çıktı |
|---|---|---|---|
| Faz 0 — SIMD Abstraksiyon | 2 hafta | 2 hafta | Runtime dispatch altyapısı |
| Faz 1 — SIMD Tokenizer | 4 hafta | 6 hafta | 2-4 GB/s tokenizer |
| Faz 2 — Arena DOM Tree | 3 hafta | 9 hafta | Cache-optimized tree |
| Faz 3 — Selector Engine | 4 hafta | 13 hafta | CSS + XPath |
| Faz 4 — Encoding | 1-2 hafta | 15 hafta | Multi-encoding desteği |
| Faz 5 — Async/Streaming | 2-3 hafta | 18 hafta | Streaming parse |
| Faz 6 — API & Yayın | 2 hafta | 20 hafta | crates.io release |

**Toplam: ~5 ay** (full-time)

---

## Kritik Bağımlılıklar

| Crate | Neden |
|---|---|
| `encoding_rs` | SIMD-optimized encoding (Mozilla'dan) — tekrar yazmaya değmez |
| `phf` | Compile-time perfect hash (entity + tag lookup) |
| `smallvec` | Stack-allocated küçük vektörler |
| `thiserror` | Error tipleri |
| `criterion` | Benchmarking |
| `cargo-fuzz` | Fuzz testing |

**Kasıtlı olarak kullanılMAYACAK:**
- `memchr` → kendi SIMD multi-delimiter scanner'ımız daha iyi (memchr tek byte arar)
- `html5ever` → rekabet ediyoruz, bağımlılık olmaz

---

## Risk ve Azaltma

| Risk | Etki | Azaltma |
|---|---|---|
| SIMD portability karmaşıklığı | 3 backend maintain etmek zor | `hp-simd` crate'ini iyi soyutla; her backend aynı trait'i impl etsin |
| Bozuk HTML edge case'leri | Sonsuz çeşitlilik | Fuzzing + gerçek site HTML'leriyle sürekli test |
| Zero-copy lifetime karmaşıklığı | API ergonomisi düşer | `OwnedDocument` varyantı sun, `Cow` kullan |
| XPath scope creep | Süre uzar | Baştan alt küme tanımla, v2'ye bırak |
| Cache-line alignment waste | Küçük dokümanlar için bellek israfı | 1KB altı HTML'ler için compact mode |

---

## Referans Okumalar

- **simdjson paper:** https://arxiv.org/abs/1902.08318 (structural indexing yaklaşımı buradan)
- **Cloudflare lol_html:** Streaming parser mimarisi referansı
- **HTML5 Tokenizer Spec:** https://html.spec.whatwg.org/multipage/parsing.html
- **Servo/html5ever kaynak kodu:** Tree construction referansı
- **VPSHUFB classification:** https://branchfree.org/2019/02/25/paper-parsing-gigabytes-of-json-per-second/
