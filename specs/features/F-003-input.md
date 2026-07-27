# F-003 — Input: sterowanie myszą i klawiaturą

- **Fala:** 2
- **Status:** Approved (2026-07-24)
- **Data:** 2026-07-24
- **ADR:** ADR-0003 (CGEvent), ADR-0001 (silnik), ADR-0007 (środkowy przycisk — ograniczenie IronRDP)
- **Powiązane:** F-003a (mapowanie klawiatury — osobny spec, bo to najtrudniejsza część)

## 1. Cel

Klient RDP steruje Makiem: mysz (ruch, przyciski, scroll) i klawiatura naprawdę
działają, a brak uprawnienia Accessibility jest komunikowany, nie milczy.

## 2. Zakres i granica

Ten spec obejmuje **całą mysz** oraz **szkielet klawiatury** (modyfikatory,
zdarzenia press/release, ścieżka wstrzykiwania). **Pełne mapowanie
scancode → CGKeyCode dla układów klawiatury to F-003a** — osobny spec, bo to
tabela na kilkaset wpisów zależna od układu i zasługuje na własną iterację
(ADR-0003 §2). F-003 może wejść w status Verified z podstawowym mapowaniem
(litery ASCII + modyfikatory), a pełne układy dokłada F-003a.

## 3. Wymagania

### Ujście inputu (CGEvent)

- **F-003.1** (MUST) MUSI powstać `input::CgEventSink` implementujący istniejący
  trait `InputSink`. Podmiana `LoggingSink` → `CgEventSink` w `main` MUSI być
  jedyną zmianą poza modułami `input` i `perms`.
- **F-003.2** (MUST) Zależność `core-graphics` MUSI zostać dodana z feature
  `highsierra` (wymagany przez `CGEvent::new_scroll_event`) i pinowana `=0.25.0`.
- **F-003.3** (MUST) Zdarzenia MUSZĄ być wstrzykiwane przez jedno źródło
  `CGEventSource::new(CGEventSourceStateID::HIDSystemState)`, trzymane przez
  cały czas życia sinka, nie tworzone per zdarzenie.
- **F-003.4** (MUST) Wstrzykiwanie MUSI dziać się w **dedykowanym zadaniu**
  czytającym z bounded kanału (`input::spawn`, już istnieje). Handler
  `RdpServerInputHandler` NIE MOŻE wołać `CGEventPost` — jest synchroniczny
  i w pętli serwera (`01-architecture.md` §4a, potwierdzone w Fali 0).

### Mysz

- **F-003.5** (MUST) `PointerEvent::MoveAbsolute { x, y }` (piksele pulpitu RDP)
  MUSI być przeliczony na globalne **punkty** macOS i wysłany jako
  `CGEventType::MouseMoved` (lub `LeftMouseDragged`/`RightMouseDragged`, gdy
  odpowiedni przycisk jest wciśnięty — inaczej przeciąganie nie działa).
- **F-003.6** (MUST) Konwersja piksele→punkty MUSI używać `scale_factor` oraz
  originu wybranego ekranu (`DisplayInfo`), tak by `(0,0)` pulpitu RDP trafiał
  w lewy górny róg tego ekranu, a nie ekranu głównego. Funkcja MUSI być czysta
  i pokryta testami jednostkowymi (Retina 2.0, ekran z niezerowym originem).
- **F-003.7** (MUST) Przyciski MUSZĄ mapować się:
  Left/Right/Middle → `CGMouseButton` + odpowiedni `CGEventType`
  (`{Left,Right,Other}MouseDown`/`Up`). Button4/Button5 → `OtherMouseDown/Up`
  z numerem przycisku w polu `MOUSE_EVENT_BUTTON_NUMBER`.
  **Znany brak (ADR-0007):** środkowy przycisk (`Middle`) jest zmapowany
  poprawnie w naszym kodzie, ale **nie działa end-to-end**, bo IronRDP 0.13.0
  gubi `PTRFLAGS_BUTTON3` w `From<MousePdu>` (dekoduje go jako `Move`) i nie ma
  publicznego haka na surowy PDU. Zdarzenie nigdy nie dociera do naszej
  konwersji. Odłożone do własnej implementacji pointer-PDU albo naprawy
  upstreamu; kod środkowego przycisku zostaje i zadziała bez zmian, gdy tylko
  zdarzenie zacznie docierać.
- **F-003.8** (MUST) Serwer MUSI utrzymywać **stan wciśniętych przycisków**,
  bo `MouseMoved` vs `MouseDragged` zależy od tego stanu, a klient wysyła
  ruch i przyciski osobnymi zdarzeniami.
- **F-003.9** (MUST) `PointerEvent::Scroll` MUSI być wysłany jako
  `CGEvent::new_scroll_event` w jednostkach linii (`CGScrollEventUnit`).
  Kierunek MUSI się zgadzać (scroll w dół pulpitu = scroll w dół na Macu),
  z jawną decyzją o „natural scrolling" opisaną w kryteriach akceptacji.
- **F-003.10** (SHOULD) `PointerEvent::MoveRelative` POWINIEN być obsłużony,
  jeśli klient go używa; jeśli nie, może być na razie zignorowany z logiem.

### Klawiatura (szkielet; pełne układy → F-003a)

- **F-003.11** (MUST) `KeyEvent::Scancode { code, extended, pressed }` MUSI być
  mapowany na `CGKeyCode` i wysłany jako `CGEvent::new_keyboard_event`.
  W F-003 mapowanie MUSI pokrywać **co najmniej** litery A–Z, cyfry 0–9,
  Enter, Space, Tab, Backspace, Esc oraz modyfikatory. Reszta → log `trace`
  i pominięcie (pełne pokrycie to F-003a).
- **F-003.12** (MUST) Serwer MUSI utrzymywać **stan modyfikatorów**
  (Shift, Control, Option/Alt, Command) i ustawiać `CGEventFlags` na **każdym**
  zdarzeniu klawiatury i myszy — macOS nie utrzymuje tego stanu za nas dla
  zdarzeń syntetycznych (ADR-0003 §3).
- **F-003.13** (MUST) `KeyEvent::SyncLocks { caps, num, scroll }` MUSI
  synchronizować stan Caps Lock z flagą `AlphaShift` przy zmianie fokusu.
- **F-003.14** (SHOULD) `KeyEvent::Unicode` POWINIEN być obsłużony przez
  `CGEventKeyboardSetUnicodeString`, jako droga awaryjna dla znaków, których
  nie ma w mapowaniu scancode. Pełne rozstrzygnięcie scancode vs Unicode → F-003a.
- **F-003.15** (MUST) Mapowanie klawisza rozszerzonego (`extended == true`,
  prefiks `E0`) MUSI być rozróżnialne od wersji bez prefiksu — np. prawy Alt
  (`E0 38`) to na macOS inny klawisz niż lewy Alt (`38`).

### Uprawnienia (Accessibility)

- **F-003.16** (MUST) `perms` MUSI obsłużyć `Permission::Accessibility`:
  sprawdzenie na starcie i **czytelny `error` z instrukcją**, jeśli brak.
  Uzasadnienie: `CGEventPost` bez Accessibility **po cichu nic nie robi** —
  bez tego sprawdzenia użytkownik zgaduje (ADR-0003 §5).
- **F-003.17** (MUST) Brak Accessibility NIE MOŻE blokować obrazu ani panikować.
  Serwer MUSI dalej serwować pulpit, a input logować zamiast wstrzykiwać
  (degradacja do `LoggingSink` albo `CgEventSink`, który wykrył brak uprawnienia
  i tylko loguje z ostrzeżeniem raz na jakiś czas).
- **F-003.18** (SHOULD) Jeśli uprawnienie pojawi się w trakcie działania,
  sink MOŻE zacząć wstrzykiwać bez restartu; jeśli to trudne, restart jest
  akceptowalny i MUSI być opisany w logu.

### Testy

- **F-003.19** (MUST) Testy jednostkowe MUSZĄ pokrywać części czyste:
  konwersję piksele→punkty (Retina, niezerowy origin), mapowanie przycisków,
  logikę MouseMoved vs Dragged wg stanu przycisków, składanie `CGEventFlags`
  ze stanu modyfikatorów, mapowanie podstawowych scancode’ów wraz z `extended`.
- **F-003.20** (MUST) Testy istniejące MUSZĄ przechodzić bez zmian, w tym
  `tests/architecture.rs` (moduł `input` nie importuje `ironrdp`) oraz
  `tests/rdp_pixels.rs` (obraz nadal działa).
- **F-003.21** (SHOULD) `examples/probe_client.rs` MOŻE zostać rozszerzony
  o wysyłanie zdarzeń wejściowych, żeby dało się bez GUI sprawdzić, że serwer
  je przyjmuje i konwertuje (wstrzyknięcia do systemu i tak nie da się
  zweryfikować bez człowieka).

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-003.1, F-003.4 | `ironrdp_server::RdpServerInputHandler` (synchroniczny); `01-architecture.md` §4a |
| F-003.5–F-003.9 | Apple, Quartz Event Services: `CGEventCreateMouseEvent`, `CGEventCreateScrollWheelEvent`, `CGEventPost` |
| F-003.11–F-003.15 | MS-RDPBCGR §2.2.8.1.1.3 (Keyboard Event, scancode set 1); Apple: `CGEventCreateKeyboardEvent`, `CGEventFlags` |
| F-003.16 | Apple: `AXIsProcessTrusted`; ADR-0003 §5 |
| F-003.6 | `DisplayInfo.scale_factor` z Fali 1 (F-002.5) |

## 5. Kryteria akceptacji

Sprawdzane ręcznie prawdziwym klientem.

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, wszystkie testy.
2. **AC-2** Ruch myszą w oknie klienta rusza kursorem Maca; pozycja się zgadza
   (kliknięcie w róg trafia w róg, nie w środek — dowód poprawnej konwersji
   piksele→punkty).
3. **AC-3** Lewy i prawy przycisk działają: klik, podwójny klik, menu
   kontekstowe prawym. **Środkowy przycisk wykreślony z kryterium** — znany
   brak ograniczony przez IronRDP 0.13.0 (ADR-0007), nie przez nasz kod.
4. **AC-4** **Przeciąganie działa** — złap okno za pasek i przesuń
   (dowód, że MouseMoved przechodzi w MouseDragged wg stanu przycisku).
5. **AC-5** Scroll działa w obu osiach, w oczekiwanym kierunku.
6. **AC-6** Pisanie działa: litery, cyfry, spacja, Enter, Backspace.
7. **AC-7** Modyfikatory działają: Shift daje wielkie litery, Cmd+C / Cmd+V
   działa, Cmd+Tab przełącza aplikacje.
8. **AC-8** Prawy Alt jest rozróżniany od lewego (F-003.15) — sprawdzić na
   skrócie, który ich używa różnie, albo w logu mapowania.
9. **AC-9** **Brak uprawnienia Accessibility**: obraz nadal działa, w logu jest
   `error` z instrukcją, żadnej paniki. Po nadaniu uprawnienia (i ewentualnym
   restarcie) input zaczyna działać.
10. **AC-10** Sesja pozostaje sterowalna przez kilka minut; brak „zawieszonych"
    modyfikatorów (nie zostaje wciśnięty Shift po zwolnieniu).

## 6. Poza zakresem

- **Pełne mapowanie układów klawiatury** (PL, DE, dead keys, IME) — F-003a.
- **Clipboard** (Cmd+C między maszynami) — F-102; tu Cmd+C działa lokalnie na Macu.
- **Touch, gesty, force touch, tablet** — poza MVP.
- **Mapowanie klawiszy multimedialnych / Fn** — poza F-003; ewentualnie F-003a.
- **Sterowanie, gdy zaznaczono nie ten ekran co capture** — spójne z tym, że
  Fala 2 obsługuje jeden wybrany ekran (jak Fala 1).

## 7. Ryzyka i otwarte kwestie

1. **Klawiatura to najtrudniejsza część MVP** (ADR-0003 §2). Dlatego świadomie
   rozbita: szkielet w F-003, pełne układy w F-003a. Nie próbujemy zrobić
   wszystkich układów naraz.
2. **`scale_factor` niesprawdzony na Retinie** (dług z Fali 1, clamshell).
   F-003 jest **pierwszym miejscem, gdzie zła wartość boli** — objawi się jako
   „mysz klika 2× za daleko". AC-2 to złapie. Jeśli wyjdzie błąd, poprawka może
   dotyczyć Fali 1 (odczyt `scale_factor`), nie tylko F-003.
3. **Origin ekranu.** Przy jednym ekranie origin to `(0,0)`; przy wielu
   trzeba użyć `frame()` z `SCDisplay`. F-003 obsługuje wybrany ekran — origin
   MUSI być brany z niego, nie zakładany jako zero, żeby F-105 (multi-monitor)
   nie wymagało przepisywania konwersji.
4. **`CGEventPost` a `HIDSystemState` vs `CombinedSessionState`.** Do ustalenia
   empirycznie, które źródło daje poprawne zachowanie modyfikatorów; decyzja
   do zapisania w ADR-0003, jeśli okaże się nieoczywista.

## 8. Status

**Verified** (2026-07-27).

Zweryfikowane ręcznie prawdziwym klientem (Windows App) przez właściciela:
- **AC-2** ruch myszy i pozycja — działa (klik trafia we właściwe miejsce;
  widoczne opóźnienie to normalna latencja rundy RDP, nie błąd konwersji).
- **AC-3** lewy i prawy przycisk — działa. Środkowy przycisk: znany brak
  ograniczony przez IronRDP 0.13.0 → **ADR-0007** (odłożone, nie regresja).
- **AC-4** przeciąganie — działa.
- **AC-5** scroll — działa.
- **AC-6** pisanie — działa.
- **AC-7** modyfikatory / skróty (Cmd+C/V, Cmd+Tab) — działa.
- **AC-1** CI zielone (fmt, clippy `-D warnings`, testy).

Pozostałe kryteria (AC-8…AC-10) bez zastrzeżeń w trakcie sesji.

**Otwarte:** środkowy przycisk myszy (ADR-0007) — do zamknięcia z własnym
parserem pointer-PDU albo PR-em do IronRDP.
