# ADR-0001: Silnik protokołu RDP — `ironrdp-server`

- **Status:** Accepted
- **Data:** 2026-07-22
- **Dotyczy:** F-001, wszystkie fale

## Kontekst

Serwer RDP musi obsłużyć pełen handshake (X.224 → MCS/GCC → licensing → capability
exchange → finalization) zanim wyśle pierwszy piksel. To kilkanaście typów PDU
i kilka tygodni pracy przed jakimkolwiek widocznym efektem.

## Decyzja

Używamy crate'a `ironrdp-server` (Devolutions/IronRDP, MIT/Apache-2.0) jako silnika
protokołu w MVP. **Pinujemy dokładną wersję** (`=0.13.0`) — patrz „Ryzyko" niżej.

Cała powierzchnia IronRDP jest izolowana w module `session`. Reszta kodu
(`capture`, `input`, `encoder`) nie zna typów IronRDP i operuje na naszych typach.

## Zbadany stan faktyczny (2026-07-22)

**Wersja:** `ironrdp-server` 0.13.0, wydana 2026-07-10, ~16.8k pobrań,
repo <https://github.com/Devolutions/IronRDP>, `rust-version = "1.89"`.

**Deklarowane wsparcie (README):**
- Security: Enhanced RDP Security z TLS 1.2 / 1.3 (external security protocol).
- Input: FastPath input events, X.224 input events i disconnect.
- Codecs: bitmap display updates z kompresją RDP 6.0.

**Feature flags:**
```toml
default = ["rayon", "qoi", "qoiz"]
helper  = ["dep:x509-cert", "dep:rustls-pemfile"]   # <-- TlsIdentityCtx jest TU
rayon   = ["dep:rayon"]
qoi     = ["dep:qoicoubeh", "ironrdp-pdu/qoi"]
qoiz    = ["dep:zstd-safe", "qoi", "ironrdp-pdu/qoiz"]
egfx    = ["dep:ironrdp-egfx"]
nscodec = ["dep:ironrdp-nscodec"]
```
> **Ważne:** `TlsIdentityCtx` nie jest widoczny w domyślnej dokumentacji docs.rs,
> bo siedzi za feature `helper`. Bez niego trzeba samemu składać `rustls::ServerConfig`.

**Traity do zaimplementowania (dokładne sygnatury z docs.rs 0.13.0):**

```rust
pub trait RdpServerDisplay: Send {
    async fn size(&mut self) -> DesktopSize;                                  // required
    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>>;  // required
    async fn request_initial_size(&mut self, client_size: DesktopSize) -> DesktopSize { .. }
    fn request_layout(&mut self, layout: DisplayControlMonitorLayout) { .. }
}

pub trait RdpServerDisplayUpdates {   // #[async_trait::async_trait]
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>>;
}

pub trait RdpServerInputHandler: Send {
    fn keyboard(&mut self, event: KeyboardEvent);   // SYNCHRONICZNE
    fn mouse(&mut self, event: MouseEvent);         // SYNCHRONICZNE
}
```
Traity async używają `#[async_trait::async_trait]` (widać w przykładzie w repo),
więc `async-trait` musi być naszą bezpośrednią zależnością.

**Builder (stan-maszyna, z `crates/ironrdp-server/src/builder.rs`):**

`WantsAddr` → `WantsSecurity` → `WantsHandler` → `WantsDisplay` → `BuilderDone`

```rust
RdpServerBuilder::<WantsAddr>::new()
    .with_addr(impl Into<SocketAddr>)                       -> WantsSecurity
    // jedno z:
    .with_no_security() | .with_tls(acceptor) | .with_hybrid(acceptor, pub_key: Vec<u8>)
                                                            -> WantsHandler
    .with_input_handler(h) | .with_no_input()               -> WantsDisplay
    .with_display_handler(d) | .with_no_display()           -> BuilderDone
    // opcjonalne na BuilderDone:
    .with_cliprdr_factory(..) .with_sound_factory(..) .with_gfx_factory(..)
    .with_bitmap_codecs(..)   .with_max_request_size(..)    .with_connection_handler(..)
    .with_display_suppressed_handle(..)  .with_honor_client_desktop_size(bool)
    .with_credential_validator(..)       .with_autodetect_rtt_handle(..)
    .build() -> RdpServer
```
`RdpServer` ma dalej: `run()`, `run_connection(stream)`, `run_connection_with(stream, tls)`,
`event_sender()`, `set_credentials(Option<Credentials>)`, `set_credential_validator(..)`,
`enable_autodetect()`, `display_suppressed_handle()`, `rtt_snapshot()`.

**Typy danych obrazu:**
```rust
pub struct BitmapUpdate {
    pub x: u16, pub y: u16,
    pub width: NonZeroU16, pub height: NonZeroU16,
    pub format: PixelFormat,
    pub data: bytes::Bytes,
    pub stride: NonZeroUsize,     // <-- stride jest jawny, patrz ADR-0005
}
pub enum DisplayUpdate {
    Resize(DesktopSize), Bitmap(BitmapUpdate),
    PointerPosition(..), ColorPointer(..), RGBAPointer(..),
    HidePointer, DefaultPointer, CachedPointer(u16),
}
```

**Typy zdarzeń wejściowych:**
```rust
pub enum KeyboardEvent {
    Pressed  { code: u8, extended: bool },
    Released { code: u8, extended: bool },
    UnicodePressed(u16), UnicodeReleased(u16),
    Synchronize(SynchronizeFlags),
}
pub enum MouseEvent {   // Copy
    Move { x: u16, y: u16 },
    LeftPressed, LeftReleased, RightPressed, RightReleased,
    MiddlePressed, MiddleReleased,
    Button4Pressed, Button4Released, Button5Pressed, Button5Released,
    VerticalScroll { value: i16 }, Scroll { x: i32, y: i32 },
    RelMove { x: i32, y: i32 },
}
```
> `KeyboardEvent::Pressed.code` to `u8` + flaga `extended` — czyli scancode set 1
> (make code) rozbity na bajt i prefiks `E0`. To jest wejście dla mapowania z Fali 2.

**`RdpServer::run()` zwraca future, który NIE jest `Send`** (odkryte przy
implementacji F-001, potwierdzone kompilatorem). Wewnątrz trzyma `Rc<Mutex<..>>`
przez punkty await. Konsekwencje:
- w `main` jest OK, bo future jest `await`-owany bezpośrednio (`block_on`),
- **nie da się go przekazać do `tokio::spawn`** — wymaga
  `tokio::task::LocalSet` + `spawn_local` (tak robi to nasz test integracyjny
  `tests/tls_handshake.rs`, tak robi też przykład Devolutions),
- gdyby kiedyś serwer miał działać obok innych zadań w tle, trzeba to zaplanować
  wokół LocalSetu albo osobnego wątku z własnym runtime'em.

**Pętla `run()` obsługuje jedno połączenie naraz**, sekwencyjnie, i **loguje**
błędy połączenia zamiast je propagować (`server.rs:960-990`). To spełnia
F-001.16 / AC-9 bez żadnego kodu po naszej stronie — potwierdzone praktycznie:
`nc -z` na porcie generuje `Connection error: not enough bytes`, po czym serwer
dalej przyjmuje połączenia.

**Referencyjny przykład:** `crates/ironrdp/examples/server.rs` w repo — kompletny
serwer z `TlsIdentityCtx::init_from_paths` → `make_acceptor()` → `with_tls(..)`,
losowe bitmapy jako `DisplayUpdate::Bitmap` z `PixelFormat::BgrA32`.
To jest nasz wzorzec dla Fali 0.

## Konsekwencje

**Plus:**
- Fala 0 jest realna w jeden podejście: implementujemy 2 traity i mamy obraz w mstsc.
- Dostajemy za darmo: TLS przez rustls, FastPath input, kompresję bitmap RDP 6.0,
  DisplayControl (resize), a od 0.11/0.12 także EGFX server, ECHO, autodetect RTT,
  `CredentialValidator` — to skraca późniejsze fale.

**Minus / dług:**
- Nie kontrolujemy warstwy protokołu; błąd w IronRDP = nasz błąd u klienta.
- API bardzo szybko się zmienia (patrz Ryzyko).

## Ryzyko: churn API

Historia wydań: 0.5.0 (2025-03) → 0.10.0 (2026-01) → 0.11.0 (2026-06-01) →
0.12.0 (2026-06-17) → 0.13.0 (2026-07-10). Cztery majory w pół roku, w tym
breaking change w 0.13.0 („Send NetworkAutoDetect over the MCS message channel").

**Mitygacja:**
1. Pin dokładnej wersji `ironrdp-server = { version = "=0.13.0", features = ["helper"] }`
   + zacommitowany `Cargo.lock`.
2. Cały kontakt z IronRDP wyłącznie w module `session` (ADR w `01-architecture.md`).
   Bump wersji = zmiana w jednym module + ewentualny nowy ADR.

## Odrzucone alternatywy

- **Własny stos MS-RDPBCGR od zera w MVP** — odrzucone: kilkanaście PDU przed
  pierwszym pikselem, zero wartości dla części macOS-owej. Wraca jako cel
  długoterminowy (backlog fal), stąd wymóg izolacji za `session`.
- **`ironrdp` (umbrella crate) z feature `server`** — odrzucone na rzecz
  bezpośredniej zależności od `ironrdp-server`: mniejsze drzewo zależności,
  `DesktopSize` i `PixelFormat` są re-eksportowane z `ironrdp-server`, więc
  umbrella nie jest potrzebna.
