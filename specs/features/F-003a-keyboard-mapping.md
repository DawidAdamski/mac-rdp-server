# F-003a — Pełne mapowanie klawiatury: układy, znaki narodowe, dead keys

- **Fala:** 2 (domyka Falę 2 razem z F-003)
- **Status:** Draft
- **Data:** 2026-07-27
- **ADR:** ADR-0003 (CGEvent), ADR-0001 (silnik), **ADR-0008** (strategia
  scancode vs Unicode — do napisania razem z approve tego speca)
- **Poprzednik:** F-003 (szkielet klawiatury — Verified 2026-07-27)

## 1. Cel

Pisanie na kliencie RDP daje na Macu **te same znaki**, które użytkownik
zamierzył — w tym polskie znaki diakrytyczne (ą, ć, ę, ł, ń, ó, ś, ź, ż),
dead keys i znaki spoza podstawowego ASCII — niezależnie od tego, jaki układ
klawiatury jest aktywny po stronie macOS, albo z jawnie udokumentowanym,
znanym ograniczeniem.

## 2. Kontekst i sedno problemu

F-003 zbudował **pozycyjną** tablicę scancode set 1 → `CGKeyCode`. To jest
poprawne dla klawiszy sterujących i skrótów (strzałki, Enter, F-klawisze,
Cmd+C), bo one są pozycyjne z natury. Ale dla **znaków drukowalnych** rodzi
fundamentalne ograniczenie:

- RDP `Scancode` niesie **pozycję** klawisza na fizycznej klawiaturze klienta,
  nie znak. Klient naciska klawisz, który *u niego* (pod *jego* układem) daje
  „ą", i wysyła scancode tej pozycji.
- My mapujemy ten scancode pozycyjnie na tę samą pozycję klawiatury Maca, a
  macOS produkuje znak wg **swojego aktualnego układu**. Jeśli układ macOS ≠
  układ klienta, wychodzi zły znak.

Czyli: **wierność znaków przy ścieżce scancode wymaga zgodności układu macOS
z układem klienta.** To jest znane, opisane ograniczenie F-003, które F-003a
ma zdjąć albo świadomie ograniczyć.

Layout-independence wymaga poznania **zamierzonego znaku**. Są dwie drogi:
1. **Ścieżka Unicode klienta** (`KeyboardEvent::UnicodePressed(u16)`), którą
   część klientów wysyła dla znaków narodowych — wtedy wstrzykujemy dokładnie
   ten znak przez `CGEventKeyboardSetUnicodeString` (`ev.set_string`),
   **z pominięciem układu macOS**. Ścieżka Unicode już istnieje w kodzie
   (F-003.14), ale jest surowa (patrz §5 Ryzyka).
2. **Odczyt negocjowanego układu klienta** (Client Core Data `keyboardLayout`,
   KLID Windows) i własne tłumaczenie scancode→znak. Cięższe, zależne od tego,
   czy IronRDP w ogóle wystawia ten układ. **Otwarta kwestia → ADR-0008.**

## 3. Wymagania

### Kompletność tablicy pozycyjnej (klawisze niedrukowalne i sterujące)

- **F-003a.1** (MUST) Tablica scancode set 1 → `CGKeyCode` MUSI pokrywać
  **komplet** klawiszy niedrukowalnych i sterujących: F1–F12, strzałki,
  Home/End/PageUp/PageDown, Insert/Delete, PrintScreen/ScrollLock/Pause
  (w zakresie, w jakim mają sens na macOS), pełny blok numeryczny z Num Lock,
  oraz wszystkie warianty rozszerzone (`E0`), z zachowaniem rozróżnienia
  lewy/prawy dla modyfikatorów (rozszerza F-003.11, F-003.15).
- **F-003a.2** (MUST) Skróty klawiszowe (Cmd+litera, Cmd+Shift+…, Ctrl+…,
  Option+…) MUSZĄ iść **ścieżką pozycyjną** (keycode + `CGEventFlags`), nie
  ścieżką Unicode — `set_string` nie wyzwala skrótów w aplikacjach. To działa
  już w F-003 (AC-7) i F-003a nie może tego zepsuć.

### Znaki drukowalne i układy (sedno)

- **F-003a.3** (MUST) Dla **znaków drukowalnych** serwer MUSI dawać wierny znak
  niezależnie od aktywnego układu macOS, albo mieć jawnie udokumentowane
  ograniczenie. Wybór strategii (Unicode-first vs odczyt układu klienta vs
  hybryda) MUSI być rozstrzygnięty w **ADR-0008** przed implementacją.
- **F-003a.4** (MUST) Ścieżka Unicode (`KeyEvent::Unicode`) MUSI poprawnie
  składać **pary surogatów UTF-16**: `ch` jest jednostką kodową `u16`, więc
  znaki spoza BMP (np. emoji) przychodzą jako dwa zdarzenia (high + low
  surrogate). Obecny kod robi `char::from_u32(u16)` i **gubi** takie znaki —
  MUSI buforować high surrogate i łączyć z low surrogate.
- **F-003a.5** (MUST) Wstrzyknięcie znaku przez `set_string` MUSI być odporne
  na aktualny stan modyfikatorów: znak z układu (np. „ą") NIE MOŻE być zniekształcony
  przez utrzymywaną flagę Option/Ctrl. (Do rozstrzygnięcia w ADR-0008:
  czyścić flagi na czas zdarzenia Unicode, czy budować je świadomie.)
- **F-003a.6** (SHOULD) Polskie znaki („programmer's" / „Polish Pro":
  ą ć ę ł ń ó ś ź ż + wielkie) POWINNY działać end-to-end z typowego klienta
  (Windows App / mstsc) na typowym układzie klienta, bez wymogu ręcznego
  zestrajania układu macOS z klientem.
- **F-003a.7** (SHOULD) **Dead keys** (np. `´` + `e` → `é`, `~` + `n` → `ñ`)
  POWINNY działać albo mieć udokumentowany znany brak. Jeśli ścieżką jest
  Unicode klienta, klient zwykle wysyła już złożony znak — wtedy „działa" jest
  tanie; jeśli ścieżką jest scancode, dead key zależy od układu macOS.

### Układy klawiatury klienta

- **F-003a.8** (MAY) Serwer MOŻE odczytać negocjowany układ klienta
  (Client Core Data `keyboardLayout`), **jeśli** IronRDP go wystawia — do
  zbadania i zapisania w ADR-0008. Jeśli nie wystawia, ta droga odpada bez
  własnego parsera MCS/GCC (poza zakresem Fali 2).
- **F-003a.9** (MAY) Serwer MOŻE logować wykryty układ klienta na starcie sesji,
  żeby diagnoza „złe znaki" była szybka.

### ISO / dodatkowe klawisze

- **F-003a.10** (SHOULD) Klawisze specyficzne dla klawiatur ISO (np. klawisz
  między lewym Shiftem a Z; `§`/`±`) POWINNY być zmapowane albo mieć znany brak.

### Testy

- **F-003a.11** (MUST) Testy jednostkowe MUSZĄ pokrywać: pełną tablicę
  pozycyjną (reprezentatywne wpisy z każdej grupy: F-klawisze, strzałki,
  numpad z/bez Num Lock, ISO), składanie par surogatów UTF-16 (high+low →
  jeden `char`; osierocony surogat → brak paniki), oraz logikę wyboru ścieżki
  scancode vs Unicode wg decyzji z ADR-0008.
- **F-003a.12** (MUST) Testy istniejące MUSZĄ przechodzić bez zmian, w tym
  `tests/architecture.rs` (moduł `input` nie importuje `ironrdp`) i cały
  dorobek F-003 (skróty, modyfikatory).
- **F-003a.13** (SHOULD) `examples/probe_client.rs` MOŻE wysłać sekwencję
  Unicode (w tym parę surogatów), żeby ścieżkę składania dało się sprawdzić
  bez GUI (samego wstrzyknięcia i tak nie zweryfikuje bez człowieka).

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-003a.1 | MS-RDPBCGR §2.2.8.1.1.3.1.1.1 (Keyboard Event, scancode set 1, flagi `KBDFLAGS_EXTENDED`) |
| F-003a.4 | RDP Unicode Keyboard Event (jednostka UTF-16); Unicode: pary surogatów |
| F-003a.5, F-003a.3 | Apple: `CGEventKeyboardSetUnicodeString`, `CGEventCreateKeyboardEvent`, `CGEventSetFlags` |
| F-003a.8 | MS-RDPBCGR Client Core Data `keyboardLayout` (KLID); do zweryfikowania w API IronRDP |
| F-003a.7 | Apple `UCKeyTranslate` / TSM (jeśli ścieżka scancode ma obsłużyć dead keys po naszej stronie) |

## 5. Kryteria akceptacji

Część czysta — testy jednostkowe. Część macOS — **checklista ręczna**
(prawdziwy klient RDP, ADR-0008 zdecyduje o ścieżce):

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, wszystkie testy (w tym nowe
   dla par surogatów i pełnej tablicy).
2. **AC-2** Polskie znaki (ą ć ę ł ń ó ś ź ż + wielkie) wpisane z klienta
   pojawiają się poprawnie na Macu — albo w polu tekstowym, albo w logu ścieżki.
3. **AC-3** Skróty z F-003 (Cmd+C/V, Cmd+Tab, Shift+litera) **nadal działają**
   — ścieżka Unicode ich nie przejęła i nie zepsuła.
4. **AC-4** F-klawisze, strzałki, Home/End/PageUp/Down, numpad (z Num Lock i bez)
   działają zgodnie z oczekiwaniem.
5. **AC-5** Znak spoza BMP (np. emoji 😀) wysłany jako para surogatów pojawia
   się jako jeden znak, nie dwa krzaki i bez paniki.
6. **AC-6** Dead key (jeśli objęty) daje złożony znak, albo brak jest
   udokumentowany jako znany.
7. **AC-7** Prawy Alt / AltGr zachowuje się zgodnie z decyzją ADR-0008 (na
   układzie, gdzie AltGr coś zmienia) — bez „zawieszonego" modyfikatora.
8. **AC-8** Kilkuminutowa sesja pisania: brak zawieszonych modyfikatorów,
   brak zgubionych znaków przy szybkim pisaniu.

## 6. Poza zakresem

- **Własny parser MCS/GCC / Client Core Data**, jeśli IronRDP nie wystawia
  układu (F-003a.8 wtedy odpada) — to zadanie warstwy własnego protokołu,
  nie Fali 2.
- **IME / języki CJK** (chiński, japoński, koreański) — osobny spec w backlogu.
- **Klawisze multimedialne / Fn / jasność / głośność** — backlog (były już poza
  F-003).
- **Remapowanie / makra / warstwy** — poza MVP.
- **Środkowy przycisk myszy** — to F-003/ADR-0007, nie klawiatura.

## 7. Ryzyka i otwarte kwestie

1. **Ścieżka scancode vs Unicode to decyzja architektoniczna — ADR-0008.**
   Ten spec celowo jej nie przesądza; podaje wymagania i ograniczenia. Approve
   F-003a idzie w parze z napisaniem ADR-0008 (zbadany stan: czy IronRDP
   wystawia `keyboardLayout`, jak `set_string` współgra z flagami modyfikatorów,
   czy klient wysyła Unicode dla znaków narodowych).
2. **Obecna ścieżka Unicode jest surowa.** `char::from_u32(u16)` gubi surogaty
   (F-003a.4) i nie zeruje modyfikatorów (F-003a.5). To pierwsze rzeczy do
   naprawy, niezależnie od wybranej strategii.
3. **Czy klient w ogóle wysyła Unicode?** mstsc/Windows App zwykle wysyłają
   scancode; Unicode bywa opcjonalne. Jeśli klient nie wysyła Unicode dla
   znaków narodowych, layout-independence wymusza drogę z odczytem układu
   (F-003a.8) — a ta zależy od API IronRDP. Do rozstrzygnięcia empirycznie
   (log surowych zdarzeń klawiatury z prawdziwego klienta) w ADR-0008.
4. **Zgodność z celem długoterminowym.** Pełna, layout-niezależna klawiatura
   bez pomocy IronRDP i tak wpadnie w warstwę własnego protokołu — F-003a robi
   tyle, ile da się zrobić przez publiczne API IronRDP, resztę jawnie odkłada.

## 8. Status

**Draft** — czeka na approve właściciela. Approve F-003a = zgoda na napisanie
ADR-0008 (strategia scancode/Unicode) przed kodem.
