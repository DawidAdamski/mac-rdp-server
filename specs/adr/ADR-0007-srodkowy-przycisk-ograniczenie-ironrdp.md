# ADR-0007: Środkowy przycisk myszy gubiony przez IronRDP 0.13.0 — odkładamy

- **Status:** Accepted
- **Data:** 2026-07-27
- **Dotyczy:** F-003 (Fala 2), F-003.7, AC-3
- **Powiązane:** ADR-0001 (silnik IronRDP), ADR-0003 (wstrzykiwanie inputu)

## Kontekst

Podczas weryfikacji F-003 właściciel zgłosił, że klik rolką (środkowy przycisk)
nie działa, mimo że lewy, prawy, scroll, przeciąganie i klawiatura działają.
Nasza konwersja przycisków (`src/input/cgevent.rs`) obsługuje środkowy przycisk
poprawnie: `MouseButton::Middle` → `OtherMouseDown/Up` + numer przycisku `2`
w polu `MOUSE_EVENT_BUTTON_NUMBER` (potwierdzone testem, który buduje prawdziwy
`CGEvent`). Mimo to klik rolką nic nie robi.

## Zbadany stan faktyczny (2026-07-27)

### IronRDP 0.13.0 gubi PTRFLAGS_BUTTON3 w standardowym pointer-PDU

Flaga środkowego przycisku **istnieje** w PDU:

```
// ironrdp-pdu-0.9.0/src/input/mouse.rs:113
const MIDDLE_BUTTON_OR_WHEEL = 0x4000;   // == MS-RDPBCGR PTRFLAGS_BUTTON3
```

ale konwersja `From<MousePdu> for MouseEvent` w `ironrdp-server` **jej nie
sprawdza** — obsługuje tylko `LEFT_BUTTON`, `RIGHT_BUTTON`, `VERTICAL_WHEEL`,
a wszystko inne (w tym `MIDDLE_BUTTON_OR_WHEEL`) wpada do gałęzi `else` jako
`MouseEvent::Move`:

```
// ironrdp-server-0.13.0/src/handler.rs:149-174  (From<MousePdu>)
if LEFT_BUTTON      { LeftPressed / LeftReleased }
else if RIGHT_BUTTON { RightPressed / RightReleased }
else if VERTICAL_WHEEL { VerticalScroll { .. } }
else                 { Move { x, y } }        // <- BUTTON3 ląduje tutaj
```

Środkowy przycisk jest dekodowany **tylko** w `From<MouseRelPdu>` (mysz
relatywna, `handler.rs:215`) oraz `From<ainput::MousePdu>` (`handler.rs:256`),
których zwykły klient RDP (mstsc / Windows App) **nie używa** dla klasycznego
sterowania. Ścieżka, którą faktycznie idzie klik rolką normalnego klienta —
`From<MousePdu>` — go traci.

### Nie ma publicznego haka na surowy PDU

`RdpServerInputHandler::mouse(&mut self, event: MouseEvent)` dostaje **już
skonwertowany** `MouseEvent`. Obie ścieżki serwera wołają tę samą stratną
konwersję zanim trafi do nas:

```
// ironrdp-server-0.13.0/src/server.rs
:331   handler.blocking_lock().mouse(pdu.into());   // slow path
:1548  handler.mouse(mouse.into());                 // fast path
```

Nie istnieje żaden inny publiczny punkt wejścia, przez który dostalibyśmy
`PointerFlags` w surowej postaci. **Środkowy przycisk jest nie do odzyskania
przez publiczne API IronRDP 0.13.0.**

### Wersja

`0.13.0` jest najnowszą opublikowaną wersją `ironrdp-server` na crates.io
(sprawdzone 2026-07-27). Nie ma dokąd bumpnąć — poprawka nie istnieje w żadnym
wydaniu.

## Decyzja

**Odkładamy środkowy przycisk.** Nie łatamy teraz pinowanej zależności ani nie
vendorujemy IronRDP dla jednego przycisku. Powody:

1. To jest dokładnie przypadek z „zasady zero": wymaganie okazało się
   niewykonalne w obecnej zależności → zatrzymujemy się i dokumentujemy zamiast
   obchodzić po cichu.
2. Długoterminowym celem projektu jest **własna implementacja MS-RDPBCGR**
   (ADR-0001). Nasz parser pointer-PDU obsłuży `PTRFLAGS_BUTTON3` poprawnie
   z definicji — wtedy ten dług znika sam, bez utrzymywania forka.
3. Koszt obejścia (vendorowany crate / fork + patch + utrzymanie) przewyższa
   wartość jednego przycisku w MVP.

Kod środkowego przycisku w `src/input/cgevent.rs` **zostaje** — jest poprawny
i zadziała bez zmian, gdy tylko zdarzenie zacznie do niego docierać (po naszej
implementacji IronRDP albo po naprawie upstreamu). Oznaczony komentarzem
wskazującym na ten ADR, żeby nie wyglądał na martwy kod.

## Konsekwencje

- **F-003.7** skorygowane: mapowanie Left/Right/Middle/Button4/Button5 pozostaje
  wymagane i zaimplementowane, ale **środkowy przycisk jest znanym, jawnym
  brakiem** ograniczonym przez IronRDP 0.13.0, nie przez nasz kod.
- **AC-3** skorygowane: obejmuje lewy i prawy przycisk (klik, podwójny klik,
  menu kontekstowe). Środkowy przycisk wykreślony z kryterium do czasu własnego
  parsera PDU lub naprawy upstreamu.
- **F-003 może przejść w Verified** — wszystkie pozostałe kryteria (AC-2, AC-4…
  AC-10) potwierdzone ręcznie przez właściciela.
- Powstaje pozycja w backlogu: „środkowy przycisk myszy" — do zamknięcia wraz
  z własną implementacją pointer-PDU (kandydat: przyszła fala protokołu) albo
  PR-em do IronRDP.

## Odrzucone alternatywy

- **`[patch.crates-io]` → vendorowana kopia `ironrdp-server` z ~6-liniową
  poprawką** (`else if MIDDLE_BUTTON_OR_WHEEL { MiddlePressed/Released }`).
  Działałoby od razu, ale wprowadza vendorowany, pinowany crate do utrzymania
  dla jednego przycisku. Odrzucone na korzyść odłożenia.
- **Fork repo IronRDP + patch przez git+rev.** Czyściej niż vendor w repo, ale
  wymaga hostowania i utrzymania forka. Odrzucone teraz; rozważymy razem z PR-em
  do upstreamu.
- **Wymuszenie ścieżki `ainput`/relatywnej na kliencie.** Nie kontrolujemy tego
  — to klient decyduje, którym PDU wysyła zdarzenia; klasyczne sterowanie idzie
  przez `MousePdu`.

## Do zrobienia (poza tym ADR)

- Zgłosić brak obsługi `MIDDLE_BUTTON_OR_WHEEL` w `From<MousePdu>` do upstreamu
  IronRDP (issue lub PR) — poprawka jest trywialna i przyda się wszystkim.
