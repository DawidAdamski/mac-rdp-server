# 01 — Architektura

**Status:** Approved (2026-07-22)
**Data:** 2026-07-22
**Powiązane ADR:** ADR-0001, ADR-0002, ADR-0003, ADR-0004, ADR-0005

## 1. Zasada organizująca całość

> **Kod macOS-owy nie zna RDP. Kod RDP-owy nie zna macOS.
> Spotykają się wyłącznie w module `session`.**

To nie jest estetyka. To jest warunek celu długoterminowego z `00-vision.md`:
podmiany IronRDP na własną implementację MS-RDPBCGR bez przepisywania capture
i inputu. Jedyny moduł, który `use`-uje `ironrdp_server::*`, to `session`.
Egzekwowane testem w CI (F-001.12).

## 2. Moduły

```
src/
  main.rs        — bootstrap: config → perms → tls → capture → session → run
  config.rs      — flagi CLI + plik TOML, walidacja
  perms.rs       — TCC: Screen Recording, Accessibility (preflight + komunikaty)
  capture/       — trait FrameSource + implementacje (test pattern, ScreenCaptureKit)
  input/         — trait InputSink + implementacje (logging, CGEvent)
  encoder/       — Frame → strumień aktualizacji obrazu (full-frame → damage)
  session/       — JEDYNE miejsce, które zna IronRDP: adaptery na jego traity
  tls.rs         — generowanie/wczytywanie self-signed certu (rcgen + TlsIdentityCtx)
```

### `capture` — źródło klatek

```rust
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub stride: usize,          // bytes_per_row; MOŻE być > width*4 (ADR-0005)
    pub format: PixelFormat,    // nasz enum, na start: Bgra8
    pub data: bytes::Bytes,
    pub captured_at: std::time::Instant,
}

pub struct DisplayInfo {
    pub id: DisplayId,
    pub width_px: u32,          // piksele backing store
    pub height_px: u32,
    pub scale_factor: f64,      // 2.0 na Retinie — potrzebne inputowi (ADR-0003)
    pub name: String,
}

#[async_trait]
pub trait FrameSource: Send {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    fn desktop_size(&self) -> (u32, u32);
    async fn start(&mut self) -> Result<FrameStream>;
    async fn stop(&mut self) -> Result<()>;
}
```

Implementacje:
- `TestPatternSource` — Fala 0, deterministyczna, testowalna jednostkowo.
- `ScreenCaptureKitSource` — Fala 1 (ADR-0002).

### `input` — ujście zdarzeń

Nasze typy, celowo **nie** typy IronRDP:

```rust
pub enum PointerEvent {
    MoveAbsolute { x: u32, y: u32 },   // w pikselach pulpitu RDP
    MoveRelative { dx: i32, dy: i32 },
    Button { button: MouseButton, pressed: bool },
    Scroll { dx: i32, dy: i32 },
}

pub enum KeyEvent {
    Scancode { code: u8, extended: bool, pressed: bool },  // PC set 1
    Unicode  { ch: u16, pressed: bool },
    SyncLocks { caps: bool, num: bool, scroll: bool },
}

pub trait InputSink: Send {
    fn pointer(&mut self, ev: PointerEvent) -> Result<()>;
    fn key(&mut self, ev: KeyEvent) -> Result<()>;
}
```

Implementacje: `LoggingSink` (Fala 0), `CgEventSink` (Fala 2, ADR-0003).

### `encoder` — Frame → aktualizacje obrazu

```rust
pub enum FrameUpdate {
    Full  { frame: Frame },
    Region { x: u32, y: u32, frame: Frame },   // Fala 3
    Resize { width: u32, height: u32 },
}

pub trait Encoder: Send {
    fn encode(&mut self, frame: Frame) -> Result<Vec<FrameUpdate>>;
}
```

- Fala 0/1: `FullFrameEncoder` — jedna klatka = jeden `FrameUpdate::Full`.
- Fala 3: `DamageEncoder` — porównanie z poprzednią klatką, cięcie na regiony.
  **Cała logika cięcia jest czysta i testowalna jednostkowo bez GUI** — to główny
  powód, dla którego `encoder` jest osobnym modułem, a nie częścią `session`.

### `session` — warstwa antykorupcyjna wokół IronRDP

Jedyny moduł z `use ironrdp_server::*`. Zawiera:
- `MacRdpDisplay: RdpServerDisplay` — `size()`, `updates()`, `request_layout()`,
- `MacRdpDisplayUpdates: RdpServerDisplayUpdates` — `next_update()`,
- `MacRdpInput: RdpServerInputHandler` — `keyboard()`, `mouse()`,
- konwersje: `FrameUpdate` → `DisplayUpdate`, `MouseEvent`/`KeyboardEvent` →
  `PointerEvent`/`KeyEvent`, `capture::PixelFormat` → `ironrdp_server::PixelFormat`,
- budowę `RdpServer` przez builder.

### `perms` — TCC

```rust
pub enum Permission { ScreenRecording, Accessibility }
pub enum PermissionState { Granted, Denied, Unknown }

pub fn check(p: Permission) -> PermissionState;
pub fn request(p: Permission);            // otwiera systemowy dialog
pub fn explain(p: Permission) -> &'static str;  // instrukcja dla użytkownika
```

Kontrakt: brak uprawnienia **nigdy** nie panikuje. Zawsze: czytelny log
z instrukcją „System Settings → Privacy & Security → …" i degradacja
(brak capture → wzorzec zastępczy; brak Accessibility → input logowany,
nie wstrzykiwany). Uzasadnienie: `CGEventPost` bez Accessibility **milczy** —
nie zwraca błędu — więc bez tego sprawdzenia użytkownik zgaduje (ADR-0003 §5).

### `config`

Precedencja: **flagi CLI > plik TOML > wartości domyślne.**
Klucze: `bind_addr`, `port`, `cert_path`, `key_path`, `display` (indeks/ID),
`max_fps`, `log_level`.

## 3. Przepływ danych

```
   ┌──────────────── OBRAZ (macOS → klient) ────────────────┐

   ScreenCaptureKit
   (wątek dispatch queue Apple, Send+Sync handler)
        │  CMSampleBuffer → CVPixelBuffer → lock_read_only() → &[u8]
        ▼
   capture::ScreenCaptureKitSource
        │  Frame { data, w, h, stride, format }
        ▼
   [ tokio::sync::watch<Frame> ]   ← KOALESCENCJA: trzyma tylko najnowszą klatkę
        │                             wolny klient ⇒ gubimy klatki, nie rośnie kolejka
        ▼
   encoder::Encoder                 → Vec<FrameUpdate>
        ▼
   session::MacRdpDisplayUpdates::next_update()   ← async, ciągnięte przez IronRDP
        │  FrameUpdate → DisplayUpdate::Bitmap(BitmapUpdate)
        ▼
   ironrdp-server  →  RDP 6.0 bitmap compression  →  FastPath  →  TLS  →  klient


   ┌──────────────── INPUT (klient → macOS) ────────────────┐

   klient → TLS → ironrdp-server
        │  KeyboardEvent / MouseEvent
        ▼
   session::MacRdpInput::{keyboard,mouse}()   ← SYNCHRONICZNE, w pętli serwera!
        │  konwersja na PointerEvent / KeyEvent
        │  try_send do kanału (NIGDY nie blokuje, przepełnienie = drop + warn)
        ▼
   [ tokio::sync::mpsc, bounded ]
        ▼
   dedykowane zadanie inputu
        ▼
   input::CgEventSink  →  CGEvent  →  CGEventPost  →  system
```

## 4. Dwie decyzje wątkowe, które wynikają wprost ze zbadanego API

**(a) Input nie może blokować.** `RdpServerInputHandler::keyboard/mouse` są
**synchroniczne** (potwierdzone w docs.rs 0.13.0, ADR-0001). Wywoływane są
w pętli serwera, więc `CGEventPost` w nich = zacinanie się całej sesji.
Dlatego: handler robi wyłącznie konwersję + `try_send` na **bounded** kanał,
a wstrzykiwanie dzieje się w osobnym zadaniu. Bounded, nie unbounded —
przy zalewie zdarzeń chcemy je gubić z ostrzeżeniem, a nie zjeść pamięć.

**(b) Klatki się koalescują.** SCK woła handler z własnego dispatch queue,
niezależnie od tego, jak szybko klient konsumuje. `tokio::sync::watch`
z definicji trzyma tylko najnowszą wartość — wolny klient dostaje mniej klatek,
ale zawsze **aktualną**, i nic nie rośnie w pamięci. Kolejka FIFO dałaby tu
narastające opóźnienie (obraz „z przeszłości") — dokładnie to, czego nie chcemy.

## 5. Granice, które trzymamy świadomie

| Granica | Co za nią jest | Kiedy się przyda |
|---|---|---|
| `FrameSource` | ScreenCaptureKit | zamiana na `objc2-screen-capture-kit` (ADR-0002), capture pojedynczego okna, multi-monitor |
| `InputSink` | CGEvent | zmiana backendu, testy bez uprawnień, nagrywanie/odtwarzanie sesji |
| `Encoder` | pełna klatka | damage tracking (Fala 3), RemoteFX/EGFX, inne kodeki |
| `session` | **cały IronRDP** | **własna implementacja MS-RDPBCGR — główny cel długoterminowy** |

## 6. Obsługa błędów i logowanie

- `anyhow` na granicach (main, session), własne typy błędów wewnątrz modułów.
- `tracing` + `tracing-subscriber`; poziom z `MAC_RDP_LOG` (analogicznie do
  `IRONRDP_LOG`). Do debugowania protokołu: `IRONRDP_LOG=trace`.
- **Serwer nie umiera z powodu jednego klienta.** Błąd połączenia = log + czekamy
  na następne. Panic w zadaniu klienta nie może ubić procesu.
- Debugowanie TLS: `TlsIdentityCtx::make_acceptor()` włącza `rustls::KeyLogFile`,
  więc `SSLKEYLOGFILE=/tmp/keys.log` + Wireshark = czytelny ruch RDP (ADR-0004).

## 7. Zależności (pinowane dokładnie — ADR-0001 §Ryzyko, ADR-0002 §Ryzyko)

| Crate | Wersja | Po co | Od fali |
|---|---|---|---|
| `ironrdp-server` | `=0.13.0`, features `["helper"]` | silnik protokołu + TlsIdentityCtx | 0 |
| `tokio` | `1`, features `["full"]` | runtime | 0 |
| `async-trait` | `0.1` | traity IronRDP go wymagają | 0 |
| `anyhow` | `1` | błędy na granicach | 0 |
| `bytes` | `1` | `BitmapUpdate.data` | 0 |
| `tracing`, `tracing-subscriber` | `0.1`, `0.3` | logi | 0 |
| `clap` | `4`, feature `derive` | CLI | 0 |
| `serde`, `toml` | `1`, `0.9` | plik konfiguracyjny | 0 |
| `rcgen` | `0.14` | self-signed cert (ADR-0004) | 0 |
| `screencapturekit` | `=8.0.1` | capture (ADR-0002) | 1 |
| `core-graphics` | `0.25` | CGEvent (ADR-0003) | 2 |

Rust: stable, **edition 2024**, MSRV ≥ 1.89 (wymóg `ironrdp-server`).
`Cargo.lock` jest w repo.
