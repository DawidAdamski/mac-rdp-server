# ADR-0005: Kontrakt formatu pikseli i stride między `capture` a `encoder`

- **Status:** Accepted
- **Data:** 2026-07-22
- **Dotyczy:** F-001, F-002

## Kontekst

Dwa końce łańcucha mają jawny stride, ale różne konwencje nazw formatu:

- **ScreenCaptureKit** → `CVPixelBuffer` z `pixel_format(): u32` (FourCC),
  `bytes_per_row(): usize`. Dla `kCVPixelFormatType_32BGRA` bajty w pamięci to
  `B, G, R, A`. **`bytes_per_row` prawie nigdy nie równa się `width * 4`** —
  CoreVideo wyrównuje wiersze (typowo do 64 B).
- **IronRDP** → `BitmapUpdate { format: PixelFormat, data: Bytes, stride: NonZeroUsize }`.
  `PixelFormat` ma 8 wariantów 32-bitowych: `ARgb32, XRgb32, ABgr32, XBgr32,
  BgrA32, BgrX32, RgbA32, RgbX32`, plus metody `bytes_per_pixel()`,
  `channel_offsets()`, `has_alpha()`, `eq_no_alpha()`, `read_color()`, `write_color()`.

## Decyzja

1. **Nie przepakowujemy wierszy.** `BitmapUpdate.stride` przyjmuje wprost
   `CVPixelBuffer::bytes_per_row()`. Kopiujemy surowy bufor 1:1 do `Bytes`.
   Zero repackingu = zero kosztu CPU na każdej klatce.
2. **Format wewnętrzny:** nasz własny enum `capture::PixelFormat` (na start jeden
   wariant: `Bgra8`), żeby `capture`/`encoder` nie zależały od typów IronRDP.
   Konwersja na `ironrdp_server::PixelFormat` żyje wyłącznie w module `session`.
3. **SCK konfigurujemy na BGRA** (`SCStreamConfiguration::with_pixel_format(PixelFormat::BGRA)`),
   żeby nie mieć ścieżki YUV/planar w MVP. `CVPixelBuffer::is_planar()` jest
   sprawdzane i planar = błąd (`unsupported`), nie ciche śmieci na ekranie.
4. **Mapowanie na wariant IronRDP: `Bgra8` → `PixelFormat::BgrA32`.**
   **Potwierdzone ze źródła** (`ironrdp-graphics-0.9.0/src/image_processing.rs`),
   nie tylko z przykładu:
   ```rust
   pub const fn channel_offsets(self) -> [usize; 4] {   // [r, g, b, a]
       Self::BgrA32 | Self::BgrX32 => [2, 1, 0, 3],
       ...
   }
   ```
   Czyli bajt 0 = B, 1 = G, 2 = R, 3 = A — dokładnie
   `kCVPixelFormatType_32BGRA`. Test `session::convert::tests::
   bgra_maps_to_the_format_whose_bytes_are_bgra` asercjuje te offsety, więc
   gdyby IronRDP zmienił znaczenie nazwy przy bumpie wersji, CI to złapie.

   **Weryfikacja wizualna** i tak zostaje: wzorzec testowy z Fali 0 ma trzy
   jednoznaczne pola (czerwone, zielone, niebieskie) — kryterium akceptacji AC-5.
5. **Alfa:** SCK oddaje `A = 255` dla nieprzezroczystego ekranu. Jeżeli klient
   zachowa się dziwnie, przełączamy na wariant `BgrX32` (bez alfy) — jednolinijkowa
   zmiana w tym samym miejscu.

## Konsekwencje

- Struktura klatki w `capture`:
  ```rust
  pub struct Frame {
      pub width: u32,
      pub height: u32,
      pub stride: usize,          // bytes_per_row, MOŻE być > width*4
      pub format: PixelFormat,    // nasz enum
      pub data: bytes::Bytes,     // wiersze wyrównane wg stride
      pub captured_at: Instant,
  }
  ```
- Każdy kod tnący klatkę na regiony (Fala 3, damage tracking) **musi** liczyć
  offset jako `y * stride + x * 4`, nigdy `y * width * 4`. To jest klasyczne
  źródło „skośnego obrazu" — dlatego dostaje test jednostkowy już w Fali 0
  (`encoder` tnie wzorzec testowy przy stride celowo różnym od `width * 4`).
- `BitmapUpdate::sub(x, y, w, h) -> Option<Self>` z IronRDP potrafi wyciąć
  podregion z zachowaniem stride — do rozważenia w Fali 3 zamiast własnego cięcia.
