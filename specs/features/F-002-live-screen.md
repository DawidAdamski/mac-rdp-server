# F-002 — Żywy ekran: ScreenCaptureKit → klient RDP

- **Fala:** 1
- **Status:** Approved (2026-07-22)
- **Data:** 2026-07-22
- **ADR:** ADR-0002 (binding SCK), ADR-0005 (piksele/stride), ADR-0001 (silnik)

## 1. Cel

Zamiast wzorca testowego klient widzi żywy pulpit Maca — a brak uprawnienia
Screen Recording jest komunikowany zrozumiale, nie objawia się czarnym ekranem.

## 2. Wymagania

### Źródło klatek

- **F-002.1** (MUST) MUSI powstać `capture::ScreenCaptureKitSource`
  implementujący istniejący trait `FrameSource`. Podmiana źródła w `main`
  MUSI być jedyną zmianą poza modułem `capture`.
- **F-002.2** (MUST) Zależność `screencapturekit` MUSI być pinowana jako `=8.0.1`
  (ADR-0002 §Ryzyko).
- **F-002.3** (MUST) `displays()` MUSI zwracać rzeczywiste ekrany z
  `SCShareableContent::get()`, z wypełnionym `width_px`, `height_px`,
  `scale_factor` i nazwą. Wartości MUSZĄ być logowane na starcie.
- **F-002.4** (MUST) Konfiguracyjny `display` (indeks) MUSI wybierać ekran.
  Indeks spoza zakresu MUSI dać czytelny błąd z listą dostępnych ekranów,
  nie panikę ani cichy fallback na ekran 0.
- **F-002.5** (MUST) `SCStreamConfiguration` MUSI żądać formatu **BGRA**
  (`kCVPixelFormatType_32BGRA`) oraz rozmiaru równego backing store wybranego
  ekranu (piksele, nie punkty).
- **F-002.6** (MUST) Częstotliwość klatek MUSI być ograniczona wartością
  `max_fps` z konfiguracji (`minimumFrameInterval`).
- **F-002.7** (SHOULD) Kursor POWINIEN być na razie wrysowany w klatkę
  (`showsCursor`), żeby użytkownik go widział. Prawdziwy kursor sprzętowy
  to F-005 (Fala 3), która to ustawienie wyłączy.

### Konwersja klatki

- **F-002.8** (MUST) `CMSampleBuffer` → `Frame` MUSI używać
  `CVPixelBuffer::bytes_per_row()` jako `stride`, **bez przepakowywania
  wierszy** (ADR-0005 §1).
- **F-002.9** (MUST) Bufor planarny (`is_planar()`) lub o `pixel_format()`
  innym niż `32BGRA` MUSI być odrzucony z logiem `warn` i pominięty —
  nigdy przepuszczony jako śmieci na ekranie.
- **F-002.10** (MUST) Klatka MUSI być skopiowana spod `CVPixelBufferLockGuard`
  do `Bytes` **przed** zwolnieniem guarda. Czas trzymania locka MUSI być
  minimalny — poza kopiowaniem nie wolno pod nim nic robić.
- **F-002.11** (MUST) Klatki MUSZĄ być publikowane przez istniejący kanał
  `watch`, czyli koalescencja zgodnie z `01-architecture.md` §4b. Handler SCK
  jest wołany z wątku dispatch queue Apple i NIE MOŻE blokować.

### Uprawnienia (TCC)

- **F-002.12** (MUST) MUSI powstać moduł `perms` z API opisanym w
  `01-architecture.md`: `check`, `request`, `explain` dla
  `Permission::ScreenRecording`.
- **F-002.13** (MUST) Brak uprawnienia MUSI dać: log `error` z instrukcją
  („System Settings → Privacy & Security → Screen Recording → dodaj <aplikacja>,
  potem zrestartuj serwer"), próbę wywołania systemowego dialogu, i **degradację
  do wzorca testowego** zamiast czarnego ekranu lub paniki.
- **F-002.14** (MUST) Wzorzec zastępczy z F-002.13 MUSI być wizualnie odróżnialny
  od zwykłego wzorca Fali 0 — użytkownik ma od razu wiedzieć, że widzi zastępnik
  z powodu uprawnień, a nie awarii capture.
- **F-002.15** (SHOULD) Uprawnienie POWINNO być sprawdzane przed startem
  strumienia, żeby komunikat pojawił się natychmiast, a nie po pierwszej
  nieudanej klatce.

### Cykl życia i diagnostyka

- **F-002.16** (MUST) `stop()` MUSI zatrzymać strumień SCK i zwolnić zasoby.
  Zamknięcie serwera (Ctrl-C) NIE MOŻE zostawić działającego strumienia.
- **F-002.17** (SHOULD) Błąd strumienia zgłoszony przez `SCStreamDelegate`
  POWINIEN być logowany. Automatyczny restart strumienia to F-007 (Fala 3).
- **F-002.18** (SHOULD) Co N sekund POWINIEN być logowany na poziomie `debug`
  licznik: klatki odebrane, klatki pominięte przez koalescencję, średni rozmiar
  klatki. To jest materiał wejściowy do decyzji w Fali 3.

### Testy

- **F-002.19** (MUST) Testy jednostkowe MUSZĄ pokrywać części czyste:
  walidację FourCC formatu piksela, walidację geometrii
  (`bytes_per_row` ≥ `width * 4`, długość bufora ≥ `height * stride`),
  wybór ekranu po indeksie wraz z przypadkiem indeksu spoza zakresu.
- **F-002.20** (MUST) Testy istniejące MUSZĄ nadal przechodzić bez zmian —
  w szczególności `tests/architecture.rs`: `capture` nie może zacząć
  importować `ironrdp`.
- **F-002.21** (SHOULD) `examples/dump_pattern.rs` POWINIEN zostać rozszerzony
  albo uzupełniony bliźniaczym narzędziem zrzucającym **prawdziwą klatkę**
  do PPM — to najtańsze narzędzie diagnostyczne dla „obraz wygląda dziwnie".

## 3. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-002.1, F-002.3, F-002.5 | Apple, ScreenCaptureKit: `SCShareableContent`, `SCContentFilter`, `SCStreamConfiguration` |
| F-002.6 | Apple: `SCStreamConfiguration.minimumFrameInterval` |
| F-002.8, F-002.9, F-002.10 | Apple, Core Video: `CVPixelBufferGetBytesPerRow`, `CVPixelBufferLockBaseAddress`, `kCVPixelFormatType_32BGRA`; ADR-0005 |
| F-002.11 | docs.rs `screencapturekit` 8.0.1: handlery muszą być `Send + Sync`, wołane z dowolnych wątków dispatch queue |
| F-002.12, F-002.13 | Apple: `CGPreflightScreenCaptureAccess`, `CGRequestScreenCaptureAccess`; ADR-0003 |
| F-002.5 (BGRA→RDP) | MS-RDPBCGR §2.2.9.1.1.3.1.2 (Bitmap Update); `PixelFormat::BgrA32` — potwierdzone w Fali 0 |

## 4. Kryteria akceptacji

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, wszystkie testy.
2. **AC-2** Po połączeniu klientem widzę **żywy pulpit Maca**: ruch okien,
   pisanie w edytorze i odtwarzane wideo są widoczne po stronie klienta.
3. **AC-3** **Kolory się zgadzają** — biały jest biały, a nie żółty/niebieski.
   (Kanały potwierdzone w Fali 0, ale SCK może oddać inny wariant, niż prosimy.)
4. **AC-4** **Obraz nie jest skośny ani przesunięty** na ekranie Retina.
   To jest test na `bytes_per_row`; sprawdzić na ekranie zewnętrznym o innej
   rozdzielczości, jeśli jest dostępny.
5. **AC-5** **Geometria pulpitu zgadza się z Retiną:** rozmiar sesji RDP
   odpowiada pikselom backing store, a `scale_factor` w logu jest poprawny
   (2.0 na wbudowanym ekranie MacBooka).
6. **AC-6** `--display 1` na Macu z jednym ekranem daje czytelny błąd z listą
   dostępnych ekranów. Z dwoma ekranami — wybiera drugi.
7. **AC-7** **Odebranie uprawnienia Screen Recording** (System Settings →
   usunięcie Terminala z listy) i restart serwera daje: log `error` z instrukcją,
   wzorzec zastępczy na kliencie, brak paniki i brak czarnego ekranu.
8. **AC-8** Po przywróceniu uprawnienia i restarcie serwera obraz wraca.
9. **AC-9** Ctrl-C zamyka serwer, a wskaźnik nagrywania ekranu w pasku menu
   macOS gaśnie (dowód, że strumień naprawdę został zatrzymany).
10. **AC-10** Sesja przeżywa kilka minut bez narastania zużycia pamięci procesu
    (dowód, że koalescencja działa i klatki nie kolejkują się w pamięci).

## 5. Poza zakresem

- **Damage tracking** — cała klatka leci w całości; optymalizacja to F-004.
- **Kursor sprzętowy** — F-005; tu kursor jest wrysowany w klatkę.
- **Resize sesji i reakcja na zmianę rozdzielczości ekranu** — F-006.
- **Reconnect, sleep/wake, restart strumienia po błędzie** — F-007.
- **Wiele monitorów** — F-105; tu jeden wybrany ekran.
- **Capture pojedynczego okna lub aplikacji** — poza roadmapą MVP.
- **Audio** (SCK to potrafi od macOS 13) — F-103.
- **Zero-copy / IOSurface / Metal** — F-107. Tu kopiujemy klatkę; przy 4K@30
  to ok. 1 GB/s memcpy i to jest **znany, świadomy dług**, materiał dla Fali 3.
- **Wstrzykiwanie inputu** — nadal `LoggingSink`; input to Fala 2.
- **Bundle `.app` z `NSScreenCaptureUsageDescription`** — F-106. Binarka
  odpalana z terminala dziedziczy uprawnienia terminala i tak zostaje.

## 6. Ryzyka i otwarte kwestie

1. **Retina.** Pulpit RDP musi mieć wymiary w pikselach backing store.
   `scale_factor` jest tu tylko zapisywany — użyje go dopiero Fala 2, do
   konwersji współrzędnych myszy (ADR-0003 §1). Jeśli w Fali 1 zapiszemy go
   źle, objawi się to dopiero w Fali 2 jako „mysz klika 2× za daleko",
   dlatego AC-5 sprawdza go jawnie **teraz**.
2. **Rozmiar pulpitu > 65535.** `DesktopSize` używa `u16`; przy realnych
   ekranach to nieosiągalne, ale konwersja MUSI być jawna, nie przez `as`.
3. **Churn `screencapturekit`.** Cztery majory w dwa miesiące (ADR-0002).
   Pin `=8.0.1` plus `Cargo.lock`; API bierzemy z docs.rs dokładnie tej wersji,
   nie z README na GitHubie, które może wyprzedzać wydanie.
4. **Nieznane:** czy crate wystawia `SCStreamFrameInfo` / dirty rects.
   Fala 1 tego nie potrzebuje, ale **przy okazji implementacji należy to
   sprawdzić i zapisać w ADR-0002** — od tego zależy kształt F-004.
5. **macOS 26.** Środowisko docelowe to macOS 26.5. Wsparcie SCK dla nowych
   wersji jest deklarowane feature'ami do `macos_15_0`; zachowanie na 26.x
   trzeba potwierdzić empirycznie przy pierwszym uruchomieniu.

## 7. Status

**Approved** (2026-07-22) — implementacja w toku.
