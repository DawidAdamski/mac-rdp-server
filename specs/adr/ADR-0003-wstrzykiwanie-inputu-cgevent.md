# ADR-0003: Wstrzykiwanie inputu — `core-graphics` (CGEvent / Quartz Event Services)

- **Status:** Accepted
- **Data:** 2026-07-22
- **Dotyczy:** F-003 (Fala 2)

## Kontekst

RDP dostarcza zdarzenia myszy i klawiatury; musimy je wstrzyknąć do systemu tak,
żeby dowolna aplikacja na Macu je zobaczyła. Na macOS jedyna sensowna droga to
Quartz Event Services (`CGEventCreateKeyboardEvent` / `CGEventCreateMouseEvent`
+ `CGEventPost`).

## Zbadany stan faktyczny (2026-07-22)

### `core-graphics` 0.25.0
- Wydany 2025-05-27, ~57,7 mln pobrań, repo `servo/core-foundation-rs`.
- Moduł `core_graphics::event` wystawia: `CGEvent`, `CGEventRef`, `CGEventTap`,
  `CGEventFlags`, `CGEventType`, `CGEventTapLocation`, `CGMouseButton`,
  `EventField`, `ScrollEventUnit`, `KeyCode` (stałe virtual key codes),
  aliasy `CGKeyCode`, `CGEventField`, `CGEventMask`.
- Dojrzały, stabilny, ale repo jest w trybie utrzymaniowym (wolne wydania).

### `objc2-core-graphics` 0.3.2
- Wydany 2025-10-04, ~28,8 mln pobrań, repo `madsmtm/objc2`.
- Bindingi generowane, pełne pokrycie CoreGraphics — w tym funkcje TCC
  (`CGPreflightScreenCaptureAccess`, `CGRequestScreenCaptureAccess`).

## Decyzja

**Fala 2 używa `core-graphics` 0.25.0** dla `CGEvent` — najkrótsza droga do
działającego inputu, API bezpieczne i przetestowane przez pół ekosystemu Rusta.

Implementacja siedzi w `input::cgevent_sink` za naszym traitem `InputSink`.

**Funkcje TCC deklarujemy sami** przez `extern "C"` w module `perms`
(4 symbole: `CGPreflightScreenCaptureAccess`, `CGRequestScreenCaptureAccess`,
`AXIsProcessTrusted`, `AXIsProcessTrustedWithOptions`) — to kilkanaście linii
i oszczędza wciągnięcie całego drugiego drzewa bindingów tylko po dwa boole.
Jeśli `perms` urośnie, przechodzimy na `objc2-core-graphics` +
`objc2-application-services` i piszemy nowy ADR.

## Konsekwencje i znane pułapki (do rozpisania w specu F-003)

1. **Współrzędne.** `MouseEvent::Move { x: u16, y: u16 }` jest w przestrzeni
   pulpitu RDP (piksele obrazu, który wysyłamy). CGEvent chce współrzędnych
   w *punktach* globalnej przestrzeni ekranów, z początkiem w lewym górnym rogu
   ekranu głównego. Na ekranie Retina 1 punkt = 2 piksele → potrzebna jawna
   konwersja przez `scale_factor` z modułu `capture`. To jest testowalna
   jednostkowo funkcja czysta i tak ma być napisana.
2. **Klawiatura to osobna, duża robota.** RDP daje scancode PC set 1
   (`code: u8` + `extended: bool`), macOS chce `CGKeyCode` (virtual key code,
   ANSI\_\* z `KeyCode`), zależnego od układu klawiatury. Mapowanie musi być
   tablicą scancode→CGKeyCode, a nie „przez znak". Osobny spec F-003a.
3. **Modyfikatory.** `CGEventFlags` trzeba ustawiać jawnie na *każdym* zdarzeniu
   (macOS nie utrzymuje stanu za nas przy syntetycznych zdarzeniach) — serwer
   musi trzymać własny stan modyfikatorów i `KeyboardEvent::Synchronize`
   (Caps/Num/Scroll lock) go resetuje.
4. **Wątek.** `RdpServerInputHandler::keyboard/mouse` są **synchroniczne**
   i wołane z pętli serwera. Nie wolno w nich blokować — patrz
   `01-architecture.md`, sekcja o kanale wejściowym.
5. **Uprawnienie Accessibility** jest twardym warunkiem: bez niego `CGEventPost`
   po cichu nic nie robi (żadnego błędu!). Dlatego `perms` MUSI sprawdzić
   `AXIsProcessTrusted()` na starcie i przy pierwszym zdarzeniu wejściowym
   wypisać czytelne ostrzeżenie, zamiast pozwolić użytkownikowi zgadywać.
   Binarka odpalona z Terminala dziedziczy uprawnienia Terminala.

## Odrzucone alternatywy

- **`enigo` / `rdev`** — odrzucone: warstwa cross-platform, która ukrywa dokładnie
  te szczegóły (flagi modyfikatorów, scancode vs virtual key, skalowanie), które
  musimy kontrolować co do bitu.
- **`CGPostKeyboardEvent` / `CGPostMouseEvent`** — odrzucone: API deprecated.
