# 🎯 Yöntem 1: İnteraktif Oturum Rehberi

## Başlamadan Önce (Tek Seferlik Kurulum)

Aşağıdaki komutları terminalde sırasıyla çalıştır:

```bash
# 1. Proje dizini
mkdir html-parser && cd html-parser
git init

# 2. Alias
alias clauded="claude --dangerously-skip-permissions"

# 3. docs/PLAN.md → daha önce indirdiğin SIMD planını buraya koy
mkdir -p docs
cp ~/Downloads/html-parser-plan-simd.md docs/PLAN.md   # kendi path'ine göre düzelt

# 4. CLAUDE.md oluştur (aşağıdaki içeriği yapıştır)
```

Aşağıdaki dosyayı proje kökünde `CLAUDE.md` olarak oluştur:

```markdown
# HTML Parser — SIMD-Optimized Rust Library

## Plan
Detaylı teknik plan: docs/PLAN.md — her fazda bu plana sadık kal.

## Kurallar
- Rust edition 2024
- Her public fonksiyona /// doc comment
- unsafe bloklarda SAFETY comment zorunlu
- Her modül için birim test
- cargo clippy -- -D warnings her zaman temiz
- cargo fmt her zaman uygulanmış

## Kod Stili
- thiserror ile error tipleri
- snake_case fonksiyonlar, PascalCase tipler
- SIMD intrinsic'lerin yanına ne yaptığını yazan yorum
- Hot path'lerde #[inline], çok kritik yerlerde #[inline(always)]
- Gereksiz allocation yapma, Cow<'a, str> tercih et

## Durum
- [ ] Faz 0: SIMD Abstraksiyon Katmanı
- [ ] Faz 1: SIMD Tokenizer
- [ ] Faz 2: Arena DOM Tree
- [ ] Faz 3: Selector Engine
- [ ] Faz 4: Encoding
- [ ] Faz 5: Async/Streaming
- [ ] Faz 6: API & Yayın

## Faz Tamamlama Kontrol Listesi
Her faz bittiğinde sırasıyla:
1. cargo fmt --all
2. cargo clippy --workspace -- -D warnings
3. cargo test --workspace
4. CLAUDE.md durum kutusunu [x] olarak güncelle
5. git add -A && git commit -m "feat(faz-N): açıklama"
```

```bash
# 5. İlk commit
git add -A && git commit -m "chore: proje iskeleti ve plan"
```

---

## Faz 0 — SIMD Abstraksiyon Katmanı

```bash
clauded
```

Claude Code açıldığında aşağıdaki prompt'u yapıştır:

---

### Prompt 0

```
docs/PLAN.md dosyasını oku, özellikle "Faz 0 — SIMD Abstraksiyon Katmanı" bölümünü.

Bu fazı eksiksiz implement et:

1. WORKSPACE KURULUMU
   - Kök Cargo.toml → workspace members: ["crates/*"]
   - crates/fhp-core/ → lib.rs, error.rs (thiserror), tag.rs (interned Tag enum + PHF), entity.rs (PHF entity tablosu)
   - crates/fhp-simd/ → lib.rs, dispatch.rs, scalar.rs, sse42.rs, avx2.rs, neon.rs

2. fhp-core CRATE
   - Tag enum: bilinen HTML tag'leri u8 olarak, from_bytes() ile PHF lookup
   - Tag::is_void() → branchless bit check
   - EntityTable: &amp; → &, &lt; → < vb. PHF ile compile-time hash
   - HtmlError enum: thiserror ile

3. fhp-simd CRATE
   - SimdLevel enum: Scalar, Sse42, Avx2, Neon
   - detect() fonksiyonu: runtime CPUID kontrolü
   - SimdOps struct: function pointer dispatch (OnceLock ile lazy init)
   - 4 temel operasyon her backend için:
     a) find_delimiters: <, >, &, " paralel arama
     b) classify_bytes: VPSHUFB/LUT tabanlı byte sınıflandırma
     c) skip_whitespace: art arda whitespace atlama
     d) find_byte_mask: tek byte için bitmask üretme
   - scalar.rs: portable fallback (SIMD yok, byte-by-byte)
   - sse42.rs: #[target_feature(enable = "sse4.2")] ile 128-bit ops
   - avx2.rs: #[target_feature(enable = "avx2")] ile 256-bit ops
   - neon.rs: #[target_feature(enable = "neon")] ile ARM ops
     (neon kodunu cfg(target_arch = "aarch64") ile gate'le, x86'da compile etmesin)

4. TESTLER
   - Her operasyon için birim test (scalar backend ile çalıştır)
   - Edge case: boş input, tek byte, 31 byte (SIMD sınırı altı), 64+ byte
   - Tag::from_bytes round-trip testleri
   - Entity lookup testleri

5. BENCHMARK (opsiyonel ama yapabilirsen yap)
   - benches/simd_bench.rs → criterion ile find_delimiters throughput

6. KONTROL
   - cargo fmt --all
   - cargo clippy --workspace -- -D warnings
   - cargo test --workspace
   - CLAUDE.md'de Faz 0 kutusunu [x] yap
   - git add -A && git commit -m "feat(faz-0): SIMD abstraksiyon katmanı"

Her adımı sırayla yap, test geçmeden sonraki adıma geçme.
```

---

### Faz 0 Bittikten Sonra Kontrol

Claude Code'dan çıkmadan önce veya çıktıktan sonra şunları kontrol et:

```bash
# Dosya yapısı doğru mu?
tree crates/ -L 3

# Testler geçiyor mu?
cargo test --workspace

# Clippy temiz mi?
cargo clippy --workspace -- -D warnings

# Commit atılmış mı?
git log --oneline -3

# Tag at (geri dönüş noktası)
git tag faz-0-complete
```

✅ Her şey tamamsa Faz 1'e geç.
❌ Sorun varsa aynı oturumda Claude'a hatayı söyle, düzeltsin.

---

## Faz 1 — SIMD Tokenizer

```bash
clauded
```

### Prompt 1a — Structural Indexer (Aşama 1 + 1.5)

Context window dolmasını önlemek için Faz 1'i ikiye böl:

```
docs/PLAN.md dosyasını oku, "Faz 1 — SIMD-Accelerated Tokenizer" bölümünü.

Faz 1'in ilk yarısını implement et — StructuralIndexer:

1. crates/fhp-tokenizer/ crate'ini oluştur, Cargo.toml'da fhp-core ve fhp-simd bağımlılığı ekle

2. STRUCTURAL INDEXER (Aşama 1)
   - StructuralIndexer struct: fhp-simd dispatch kullanarak
   - index(&self, input: &[u8]) -> StructuralIndex
   - 64-byte bloklar halinde input'u tara
   - Her blok için bitmask üret: lt_mask, gt_mask, amp_mask, quot_mask
   - BlockBitmaps struct: her delimiter için u64 bitmask

3. QUOTE-AWARE MASKING (Aşama 1.5)
   - compute_string_masks(): tırnak içindeki delimiter'ları iptal et
   - prefix XOR sum ile string state tracking
   - Önceki bloktan kalan in_string state'i carry et
   - Sonuç: structural olmayan pozisyonlar maskelenir

4. StructuralIndex API
   - iter_delimiters() → pozisyon + byte türü iterator'ı
   - estimated_token_count() → Vec pre-allocation için hint

5. TESTLER
   - Basit HTML: <div class="foo">bar</div>
   - Attribute içinde < ve >: <a title="x > y">
   - Boş input, sadece text, sadece tag
   - Uzun input (1000+ byte)

6. cargo fmt && clippy && test && git commit -m "feat(faz-1a): structural indexer"
```

### Faz 1a Kontrol

```bash
cargo test -p fhp-tokenizer
git log --oneline -1
```

✅ Tamamsa Prompt 1b'ye geç.

---

### Prompt 1b — Token Extraction + State Machine (Aşama 2)

Aynı oturumda devam edebilirsin (`clauded --continue`) veya yeni oturum aç:

```
docs/PLAN.md dosyasını oku, Faz 1 Aşama 2 bölümünü.

Mevcut fhp-tokenizer'daki StructuralIndexer üzerine token extraction'ı ekle:

1. TOKEN TİPLERİ
   - Token<'a> enum: OpenTag, CloseTag, Attribute, Text, Comment, Doctype, CData
   - Her varyant &'a str referansları taşısın (zero-copy)

2. BRANCHLESS STATE MACHINE
   - State enum: Data, TagOpen, TagName, EndTagOpen, AttributeName, BeforeAttrValue, AttributeValue, SelfClosing, Comment, Doctype, RawText
   - ByteClass enum: Lt, Gt, Slash, Eq, Quot, Amp, Bang, Dash, Alpha, Whitespace, Other
   - STATE_TABLE: [[Transition; ByteClass sayısı]; State sayısı] const array
   - Transition struct: { state: State, action: Action }
   - Action enum: None, FlushText, StartTag, EndTag, EmitAttr, StartComment, EndComment, ...

3. TOKEN EXTRACTION
   - extract_tokens<'a>(input: &'a [u8], index: &StructuralIndex) -> Vec<Token<'a>>
   - StructuralIndex üzerinden iterate et
   - State machine ile her delimiter'da state geçişi yap
   - Action'a göre token emit et

4. ENTITY DECODING
   - decode_entities<'a>(input: &'a str) -> Cow<'a, str>
   - SIMD ile '&' varlık kontrolü — yoksa Cow::Borrowed döndür
   - Varsa fhp-core entity tablosundan decode → Cow::Owned

5. STREAMING API
   - StreamTokenizer struct: state + residual buffer (ArrayVec<u8, 64>)
   - feed<'a, F>(&mut self, chunk: &'a [u8], emit: F) where F: FnMut(Token<'a>)
   - Chunk sınırında kalan partial token'lar için residual handling

6. ANA PARSE FONKSİYONU
   - pub fn tokenize<'a>(input: &'a str) -> Vec<Token<'a>> (convenience wrapper)

7. TESTLER — kapsamlı:
   - Well-formed HTML: her token türü için
   - Bozuk HTML: kapatılmamış tag, eksik tırnak, <script> içinde < karakteri
   - Entity decoding: &amp; &lt; &#60; &#x3C; bilinmeyen entity
   - Streaming: aynı HTML'i 1-byte, 7-byte, 64-byte chunk'larla besle, sonuç aynı olmalı
   - Boş input, sadece whitespace, sadece entity

8. BENCHMARK
   - benches/tokenizer_bench.rs → criterion ile GB/s throughput ölçümü
   - 1KB, 100KB, 5MB HTML dosyaları ile (testdata/ dizinine basit HTML koy)

9. cargo fmt && clippy && test
   - CLAUDE.md'de Faz 1 kutusunu [x] yap
   - git add -A && git commit -m "feat(faz-1b): token extraction, state machine, streaming"
```

### Faz 1 Tamamlanma Kontrolü

```bash
cargo test --workspace
cargo bench -- tokenize  # throughput'u gör

git tag faz-1-complete
git log --oneline -5
```

---

## Faz 2 — Arena DOM Tree

```bash
clauded
```

### Prompt 2

```
docs/PLAN.md dosyasını oku, "Faz 2 — Cache-Optimized Arena DOM Tree" bölümünü.

Bu fazı eksiksiz implement et:

1. crates/fhp-tree/ crate'ini oluştur, fhp-core ve fhp-tokenizer bağımlılığı ekle

2. NODE LAYOUT (64-byte, cache-line aligned)
   - #[repr(C, align(64))] pub struct Node
   - İlk 32 byte (hot): tag(Tag), flags(NodeFlags), depth(u16), parent/first_child/next_sibling/last_child/prev_sibling (NodeId=u32), text_offset(u32), text_len(u32)
   - İkinci 32 byte (cold): attr_offset(u32), attr_count(u8), padding
   - NodeId: u32 wrapper, NULL = u32::MAX
   - NodeFlags: bitflags — is_void, has_children, has_attrs, is_self_closing

3. ARENA
   - Arena struct: nodes(Vec<Node>), text_slab(Vec<u8>), attr_slab(Vec<Attribute>)
   - Attribute<'a>: name(&'a str), value(Cow<'a, str>)
   - arena.new_node(data) -> NodeId
   - arena.append_child(parent, child)
   - arena.attrs(node) -> &[Attribute]
   - arena.text(node) -> &str

4. TREE BUILDER
   - TreeBuilder struct: arena + open_elements stack (Vec<NodeId>)
   - process(token: Token) -> Option<NodeId> — token'ı tree'ye ekle
   - Implicit close kuralları: IMPLICIT_CLOSE LUT (Tag×Tag -> bool)
   - Void element handling: Tag::is_void() kontrolü
   - Bozuk HTML stratejileri:
     a) Kapatılmamış tag → dosya sonunda otomatik kapat
     b) Yanlış sırada kapanan tag → stack'te en yakın eşleşeni bul
     c) Bilinmeyen tag → Unknown olarak kabul et
   - finish() -> Document

5. DOCUMENT
   - Document<'a> struct: arena + root NodeId
   - document.root() -> NodeRef
   - document.get(NodeId) -> NodeRef

6. TRAVERSAL ITERATOR'LARI (allocation-free)
   - DepthFirst<'a>: pre-order traversal, stack kullanmadan (tree link'leri ile)
   - BreadthFirst<'a>: VecDeque ile (burada allocation kaçınılmaz)
   - Children<'a>: bir node'un çocuklarını iterate et
   - Ancestors<'a>: parent zinciri
   - Siblings<'a>: next_sibling zinciri

7. NodeRef CONVENIENCE API
   - node.tag() -> Tag
   - node.text_content() -> String (recursive text toplama)
   - node.inner_html() -> String
   - node.outer_html() -> String
   - node.attr(name) -> Option<&str>
   - node.has_class(name) -> bool
   - node.children() -> Children
   - node.parent() -> Option<NodeRef>

8. ANA PARSE FONKSİYONU
   - pub fn parse<'a>(input: &'a str) -> Result<Document<'a>, HtmlError>
   - tokenize → tree build → Document

9. TESTLER
   - Basit HTML → doğru tree yapısı kontrolü (parent-child ilişkileri)
   - Bozuk HTML: <p><p> implicit close, kapatılmamış <div>, yanlış nesting
   - Void elementler: <br>, <img src="x">
   - text_content(): nested text toplama
   - inner_html() / outer_html() round-trip
   - Traversal: depth-first sırası, children sayısı, ancestor zinciri
   - Arena memory: 64-byte alignment kontrolü (std::mem::size_of::<Node>() == 64)

10. BENCHMARK
    - benches/tree_bench.rs: parse throughput (ms), node count, memory kullanımı

11. cargo fmt && clippy && test
    - CLAUDE.md'de Faz 2 kutusunu [x] yap
    - git add -A && git commit -m "feat(faz-2): arena-based DOM tree"
```

### Faz 2 Kontrol

```bash
cargo test --workspace
cargo bench -- tree

# Node boyutu 64 byte mi?
# (testlerde kontrol edilmiş olmalı ama elle de bakabilirsin)

git tag faz-2-complete
```

---

## Faz 3 — Selector Engine

```bash
clauded
```

### Prompt 3a — CSS Selector

```
docs/PLAN.md dosyasını oku, "Faz 3 — Selector Engine" bölümünü.

CSS Selector kısmını implement et:

1. crates/fhp-selector/ crate'ini oluştur, fhp-core ve fhp-tree bağımlılığı ekle

2. CSS SELECTOR PARSER
   - Selector string → SelectorAST parse
   - Desteklenen selector'lar:
     Basit: div, .class, #id, *, [attr], [attr=val], [attr~=val], [attr^=val], [attr$=val], [attr*=val]
     Combinator: A B (descendant), A > B (child), A + B (adjacent), A ~ B (general sibling)
     Pseudo: :first-child, :last-child, :nth-child(an+b), :not(sel)
     Compound: div.class#id[attr]

3. COMPILED SELECTOR
   - CompiledSelector struct: instructions(Vec<SelectorOp>) + bloom_hashes
   - SelectorOp enum: MatchTag, MatchClass, MatchId, MatchAttr, MatchAttrValue, Combinator, PseudoNthChild, PseudoNot, Accept, Reject
   - compile(ast: &SelectorAST) -> CompiledSelector
   - SelectorCache: HashMap<String, Arc<CompiledSelector>> ile aynı selector'ı tekrar compile etme

4. BLOOM FILTER
   - AncestorBloom struct: [u64; 4] (256-bit, stack-allocated)
   - Tree build sırasında her node'un ancestor bloom'unu hesapla
   - may_contain(hash) → descendant selector için hızlı false elimination

5. MATCHING ENGINE
   - Right-to-left matching: selector'ın sağ ucundan başla
   - match_node(selector, node, arena) -> bool
   - select_all(selector, document) -> Vec<NodeId>
   - select_first(selector, document) -> Option<NodeId>

6. DOCUMENT ENTEGRASYONU
   - Document'a select(&self, css: &str) -> Result<Selection, SelectorError> ekle
   - Selection struct: Vec<NodeId> + arena referansı
   - Selection API: first(), iter(), text(), attr(), inner_html(), len()
   - Chaining: selection.select("a") → alt-seçim

7. CONVENIENCE API
   - doc.find_by_tag(Tag) -> Selection
   - doc.find_by_id(&str) -> Option<NodeRef>  (id → NodeId HashMap ile O(1))
   - doc.find_by_class(&str) -> Selection
   - doc.find_by_attr(name, value) -> Selection

8. TESTLER
   - Her selector türü için: div, .class, #id, [attr], [attr=val]
   - Combinator'lar: "div > p", "div p", "h1 + p", "h1 ~ p"
   - Pseudo: ":first-child", ":nth-child(2n+1)", ":not(.hidden)"
   - Compound: "div.active#main[data-x]"
   - Chaining: doc.select("ul")?.select("li > a")?
   - Bloom filter: ancestor kontrolünde false positive oranı düşük olmalı
   - Büyük HTML (1000+ node) ile selector performansı

9. cargo fmt && clippy && test
   - git add -A && git commit -m "feat(faz-3a): CSS selector engine"
```

### Prompt 3b — XPath

Aynı oturumda veya yeni oturumda:

```
docs/PLAN.md dosyasını oku, Faz 3 XPath kısmını.

Mevcut fhp-selector crate'ine XPath desteği ekle:

1. XPATH PARSER
   - XPath string → XPathExpr AST
   - Desteklenen:
     //tag — descendant arama
     //tag[@attr='value'] — attribute predicate
     /path/to/tag — absolute path
     //tag[contains(@attr, 'substr')] — contains predicate
     //tag[position()=N] — position predicate
     //tag/text() — text extraction
     ../ — parent axis

2. XPATH EVALUATOR
   - evaluate(expr, document) -> XPathResult
   - XPathResult enum: Nodes(Vec<NodeId>), Strings(Vec<String>), Boolean(bool)

3. DOCUMENT ENTEGRASYONU
   - doc.xpath(&str) -> Result<XPathResult, XPathError>

4. TESTLER
   - Her XPath ifadesi için birim test
   - CSS selector ile aynı sonucu veren karşılaştırma testleri
   - Edge case: root'tan arama, boş sonuç, birden fazla eşleşme

5. cargo fmt && clippy && test
   - CLAUDE.md'de Faz 3 kutusunu [x] yap
   - git add -A && git commit -m "feat(faz-3b): XPath desteği"
```

### Faz 3 Kontrol

```bash
cargo test -p fhp-selector
git tag faz-3-complete
```

---

## Faz 4 — Encoding

```bash
clauded
```

### Prompt 4

```
docs/PLAN.md dosyasını oku, "Faz 4 — Encoding Katmanı" bölümünü.

Bu fazı implement et:

1. crates/fhp-encoding/ crate'ini oluştur, encoding_rs bağımlılığı ekle

2. ENCODING DETECTION
   - detect(input: &[u8]) -> &'static Encoding
   - Sırasıyla kontrol:
     a) BOM detection (UTF-8 BOM, UTF-16 LE/BE BOM) — ilk 3 byte
     b) <meta charset="..."> pre-scan — SIMD ile '<' ve 'meta' ara, ilk 1KB'da
     c) <meta http-equiv="Content-Type" content="...charset=..."> 
     d) Fallback: UTF-8

3. ENCODING DÖNÜŞÜMÜ
   - decode(input: &[u8], encoding: &Encoding) -> Result<String, EncodingError>
   - decode_or_detect(input: &[u8]) -> Result<String, EncodingError> (auto-detect + decode)
   - Streaming variant: DecodingReader wrap (chunk-based decode)

4. PARSER ENTEGRASYONU
   - fast-html-parser (henüz yok ama fhp-tree'deki parse fonksiyonunu güncelle):
     raw bytes → encoding detect → UTF-8'e çevir → tokenize → tree build
   - from_bytes(input: &[u8]) -> Result<Document, HtmlError>

5. TESTLER
   - UTF-8 (BOM'lu ve BOM'suz)
   - ISO-8859-1 (Latin-1)
   - Windows-1252 (Türkçe karakterler: ş, ğ, ı, ö, ü, ç)
   - UTF-16 LE ve BE
   - Meta charset detection: <meta charset="windows-1252">
   - Fallback: encoding bilgisi olmayan input → UTF-8

6. cargo fmt && clippy && test
   - CLAUDE.md'de Faz 4 kutusunu [x] yap
   - git add -A && git commit -m "feat(faz-4): encoding detection ve dönüşüm"
```

### Faz 4 Kontrol

```bash
cargo test -p fhp-encoding
git tag faz-4-complete
```

---

## Faz 5 — Async / Streaming

```bash
clauded
```

### Prompt 5

```
docs/PLAN.md dosyasını oku, "Faz 5 — Async + Streaming Entegrasyonu" bölümünü.

Bu fazı implement et:

1. FEATURE FLAGS (fhp-tree ve fhp-tokenizer Cargo.toml'larına ekle)
   - async-tokio = ["dep:tokio"]
   - async-async-std = ["dep:async-std"]

2. STREAMING PARSER
   - StreamParser struct: StreamTokenizer + TreeBuilder + config
   - feed(&mut self, chunk: &[u8]) → partial tree build
   - finish(self) -> Result<Document, HtmlError>

3. ASYNC API (tokio feature flag altında)
   - AsyncParser<R: tokio::io::AsyncRead>
   - async fn parse(self) -> Result<Document, HtmlError>
   - 64KB aligned buffer ile chunk okuma
   - Her chunk: encoding decode → tokenize → tree build

4. EARLY TERMINATION
   - EarlyStopParser: bir predicate ile parse'ı erken durdur
   - stop_when(predicate: impl Fn(&Node) -> bool)
   - ParseStatus enum: NeedMore, Found(NodeId), Done(Document)
   - Kullanım: aranan element bulunduğunda kalan HTML'i parse etme

5. CONVENIENCE
   - parse_stream(chunks: impl Iterator<Item = &[u8]>) -> Result<Document>
   - async fn parse_async<R: AsyncRead>(reader: R) -> Result<Document> (tokio)

6. TESTLER
   - Aynı HTML'i tek seferde ve streaming ile parse et → aynı tree
   - Chunk boyutları: 1, 7, 64, 1024, 65536 byte
   - Early termination: ilk <a> tag'ını bul ve dur
   - Async test: tokio::test ile
   - Encoding + streaming: UTF-16 HTML'i chunk'larla besle

7. cargo fmt && clippy && test
   - cargo test --workspace --features async-tokio
   - CLAUDE.md'de Faz 5 kutusunu [x] yap
   - git add -A && git commit -m "feat(faz-5): async ve streaming parse"
```

### Faz 5 Kontrol

```bash
cargo test --workspace --all-features
git tag faz-5-complete
```

---

## Faz 6 — Facade API & Yayın

```bash
clauded
```

### Prompt 6

```
docs/PLAN.md dosyasını oku, "Faz 6 — Public API, Benchmark ve Yayın" bölümünü.

Son fazı implement et:

1. crates/fast-html-parser/ FACADE CRATE
   - Tüm alt crate'leri re-export et
   - Builder pattern:
     HtmlParser::builder()
       .encoding(Encoding::Auto)
       .max_errors(100)
       .fragment_mode(true)
       .build()
       .parse(html)?
   - Convenience: HtmlParser::parse(html), HtmlParser::parse_bytes(bytes)
   - Async: HtmlParser::parse_async(reader).await

2. FEATURE FLAGS (fast-html-parser/Cargo.toml)
   default = ["css-selector", "entity-decode"]
   simd = []
   css-selector = []
   xpath = []
   encoding = ["dep:fhp-encoding"]
   async-tokio = ["fhp-tree/async-tokio"]
   entity-decode = []

3. EXAMPLES
   - examples/basic_parse.rs: basit HTML parse + select
   - examples/web_scraping.rs: CSS selector ile link çıkarma
   - examples/streaming.rs: chunk-based parse
   - examples/xpath_query.rs: XPath kullanımı
   - examples/encoding.rs: farklı encoding'lerle çalışma

4. DOKÜMANTASYON
   - Her public API'ye /// doc comment (örnekli)
   - lib.rs'e crate-level dokümantasyon (//! ile)
   - README.md: proje açıklaması, kurulum, hızlı başlangıç, benchmark sonuçları, feature flags
   - CHANGELOG.md: v0.1.0 girdisi

5. SON KONTROLLER
   - cargo fmt --all
   - cargo clippy --workspace --all-features -- -D warnings
   - cargo test --workspace --all-features
   - cargo doc --workspace --no-deps (doc build başarılı olmalı)
   - Tüm example'lar çalışmalı: cargo run --example basic_parse

6. BENCHMARK
   - benches/e2e_bench.rs: end-to-end parse + select throughput
   - Küçük (1KB), orta (100KB), büyük (5MB) HTML ile ölçüm
   - testdata/ dizinine örnek HTML dosyaları koy

7. YAYINA HAZIRLIK
   - Her crate Cargo.toml'unda: name, version="0.1.0", edition="2024", license="MIT OR Apache-2.0", description, repository, keywords, categories
   - CLAUDE.md'de tüm fazları [x] yap
   - git add -A && git commit -m "feat(faz-6): public API, docs, örnekler"
   - git tag v0.1.0
```

### Faz 6 Kontrol

```bash
cargo test --workspace --all-features
cargo doc --workspace --no-deps --open  # dökümantasyonu incele
cargo run --example basic_parse

git tag faz-6-complete
git log --oneline
```

---

## Son Doğrulama

Tüm fazlar bittikten sonra tam bir doğrulama yap:

```bash
# Tüm testler
cargo test --workspace --all-features

# Clippy
cargo clippy --workspace --all-features -- -D warnings

# Doc build
cargo doc --workspace --all-features --no-deps

# Benchmark
cargo bench

# Örnek çalıştırma
cargo run --example basic_parse
cargo run --example web_scraping
cargo run --example streaming

# Git durumu
git log --oneline --graph
git tag -l

echo "🎉 Proje tamamlandı!"
```

---

## Sorun Giderme

### Claude hata yapıp düzeltemezse:
```bash
git stash                  # mevcut değişiklikleri sakla
git log --oneline -10      # son commit'leri gör
git reset --hard faz-X-complete  # son başarılı faz'a dön
clauded                    # yeni oturum, tekrar dene
```

### Context window dolarsa:
```bash
# Yeni oturum aç, kaldığı yerden devam et
clauded
# > "Projeyi incele. Faz X yarım kalmış, kaldığı yerden devam et."
```

### Compile hatası çözemezse:
```bash
clauded
# > "cargo test --workspace çalıştır, çıkan hataları tek tek düzelt.
#    Her düzeltmeden sonra tekrar test et."
```
