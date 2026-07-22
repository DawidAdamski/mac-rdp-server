# ADR-0002: Binding do ScreenCaptureKit — `screencapturekit` vs `objc2-screen-capture-kit`

- **Status:** Accepted
- **Data:** 2026-07-22
- **Dotyczy:** F-002 (Fala 1), F-004 (Fala 3)

## Kontekst

Przechwytywanie ekranu na macOS 12.3+ robi się przez ScreenCaptureKit.
Do wyboru dwa craty o zupełnie różnej filozofii.

## Zbadany stan faktyczny (2026-07-22)

### `screencapturekit` 8.0.1

- Wydany 2026-07-18 (4 dni temu), ~903k pobrań łącznie.
- Repo: <https://github.com/doom-fish/screencapturekit-rs>, opis:
  „Safe Rust bindings for Apple's ScreenCaptureKit framework".
- **Nie jest oparty na objc2** — używa własnych cienkich bindingów (`apple-cf`,
  `apple-metal`) i surowego Objective-C FFI.
- Wymaga macOS 12.3+; feature'y kumulatywne: `macos_13_0` (audio),
  `macos_14_0` (screenshot/picker), `macos_15_0` (recording/HDR), `async`.
- Moduły: `stream`, `shareable_content`, `cm` (CMSampleBuffer, CMTime, IOSurface),
  `cv` (CVPixelBuffer + lock guardy), `cg`, `metal`, `dispatch_queue`,
  `async_api`, `screenshot_manager`, `recording_output`.
- Wątkowanie: handlery muszą być `Send + Sync`, bo Apple woła je z dowolnych
  wątków dispatch queue. **Nie wymaga run loopa na main thread** — to kluczowe
  dla binarki CLI pod Tokio.
- Uprawnienia: czysto TCC, bez entitlementu; `NSScreenCaptureUsageDescription`
  w Info.plist dla zbundlowanej aplikacji.

Ścieżka do pikseli (potwierdzona w docs.rs 8.0.1):
```rust
impl SCStreamOutputTrait for Handler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, _: SCStreamOutputType) {
        let pb  = sample.image_buffer()?;            // CVPixelBuffer
        let w   = pb.width();                        // usize
        let h   = pb.height();                       // usize
        let bpr = pb.bytes_per_row();                // usize  <-- stride, patrz ADR-0005
        let fmt = pb.pixel_format();                 // u32 (FourCC)
        let g   = pb.lock_read_only()?;              // CVPixelBufferLockGuard (RAII)
        let px: &[u8] = g.as_slice();
    }
}
```
Setup strumienia:
```rust
let content = SCShareableContent::get()?;
let display = &content.displays()[0];
let filter  = SCContentFilter::create().with_display(display)
                                       .with_excluding_windows(&[]).build();
let config  = SCStreamConfiguration::new()
                  .with_width(w).with_height(h)
                  .with_pixel_format(PixelFormat::BGRA);
let mut stream = SCStream::new(&filter, &config);
stream.add_output_handler(Handler, SCStreamOutputType::Screen);
stream.start_capture()?;  // ... stream.stop_capture()?
```

### `objc2-screen-capture-kit` 0.3.2

- Wydany 2025-10-04, ~140k pobrań. Repo: <https://github.com/madsmtm/objc2>.
- Bindingi generowane automatycznie z nagłówków — **pełne pokrycie API**,
  ale surowe: `unsafe`, ręczne `define_class!` na delegata, ręczne feature'y per klasa.
- Spójny z resztą ekosystemu objc2 (`objc2-core-media`, `objc2-core-video`,
  `objc2-app-kit`) — jeżeli i tak będziemy potrzebować AppKit, unikamy dublowania
  cienkich bindingów.
- Stabilne, wolne tempo wydań (0.2.2 → 0.3.0 → 0.3.2 przez ~1,5 roku).

## Decyzja

**Fala 1 używa `screencapturekit` 8.0.1, pinowanego dokładnie (`=8.0.1`).**

Powody:
1. Bezpieczne API bez ręcznego `define_class!` i zarządzania retain/release —
   Fala 1 to ma być parę dni, nie dwa tygodnie walki z Objective-C runtime.
2. Gotowy `CVPixelBufferLockGuard` (RAII) i `bytes_per_row()` — dokładnie to,
   czego potrzebuje nasz `BitmapUpdate` (ADR-0005).
3. Brak wymogu run loopa na main thread — pasuje do binarki CLI na Tokio.
4. Jeden porządny przykład end-to-end w README, zgodny z naszym przypadkiem użycia.

Implementacja siedzi wyłącznie w `capture::screencapturekit_source` za naszym
traitem `FrameSource` (patrz `01-architecture.md`). Zamiana backendu = jeden plik.

## Ryzyko i warunki rewizji

**Churn jest ostry:** 5.0.1 (2026-05-18) → 6.0.0 (05-18) → 6.0.1 (05-20) →
6.1.0 (05-29) → 7.0.0 (06-02) → 7.0.1 (06-06) → 8.0.0 (06-19) → 8.0.1 (07-18).
Cztery majory w dwa miesiące. Mitygacja: pin `=8.0.1` + `Cargo.lock` w repo;
bump wersji tylko świadomie, z przejrzeniem CHANGELOG-a.

**Niepotwierdzone:** dokumentacja modułu `stream` (docs.rs 8.0.1) **nie wymienia**
`SCStreamFrameInfo`, statusu klatki ani dirty rects / content rect / attachments.
`objc2-screen-capture-kit` ma je na pewno, bo generuje całe API.

**Warunek rewizji tej decyzji:** jeżeli w Fali 3 (damage tracking) okaże się,
że `screencapturekit` nie wystawia `SCStreamFrameInfoDirtyRects`, mamy dwie drogi:
(a) własny diff klatek po stronie `encoder` (i tak potrzebny jako fallback),
(b) przejście na `objc2-screen-capture-kit` w module `capture`.
Decyzja wtedy → nowy ADR, bez zmian poza modułem `capture`.

## Odrzucone alternatywy

- **`objc2-screen-capture-kit` od razu** — odrzucone na Falę 1: więcej `unsafe`
  i ręcznej roboty z delegatem, spowalnia dojście do „widzę pulpit".
  Pozostaje pierwszym kandydatem przy rewizji.
- **CGDisplayStream / CGWindowListCreateImage** — odrzucone: deprecated
  na rzecz ScreenCaptureKit, gorsza wydajność, brak wsparcia dla nowych funkcji.
