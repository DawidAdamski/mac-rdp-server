# F-003a — Pełne mapowanie klawiatury: układy, znaki narodowe, dead keys

- **Fala:** 2 (domyka Falę 2 razem z F-003)
- **Status:** Draft
- **Data:** 2026-07-27
- **ADR:** ADR-0003 (CGEvent), ADR-0001 (silnik), **ADR-0008** (strategia
  scancode vs Unicode — do napisania razem z approve tego speca)
- **Poprzednik:** F-003 (szkielet klawiatury — Verified 2026-07-27)
- **Uzupełnienie 2026-09-17 (Implemented (2026-09-18); Approved 2026-09-17, właściciel):** F-003a.14–F-003a.18 — naprawa ścieżki
  Unicode (surogaty, flagi modyfikatorów). Źródło: `02-ms-rdpbcgr.md` §4 P-05.
  **Zatwierdzone niezależnie od ADR-0008** (reszta F-003a pozostaje Draft):
  to poprawki błędu, a nie wybór strategii scancode/Unicode.

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

### Naprawa ścieżki Unicode — uzupełnienie 2026-09-17

**Status tej grupy: Implemented (2026-09-18)** (Approved 2026-09-17, właściciel). Doprecyzowuje F-003a.4, F-003a.5
i F-003a.11. Ich treść zostaje bez zmian; wymagania poniżej opisują, jak
konkretnie mają być spełnione.

Kontekst.
- **Jednostka.** MS-RDPBCGR 1.1 (str. 19) definiuje „Unicode character” jako
  **16-bitową jednostkę kodową UTF-16**, o ile nie powiedziano inaczej.
  `unicodeCode` w Unicode Keyboard Event (2.2.8.1.1.3.1.1.2, str. 171;
  fast-path: 2.2.8.1.2.2.2, str. 179) jest takim polem, więc znak spoza BMP
  przychodzi jako dwa zdarzenia.
- **Błąd dziś.** `src/input/cgevent.rs:166-173` tworzy zdarzenie klawiatury
  z keycode `0` i ustawia mu tekst tylko wtedy, gdy `char::from_u32` się uda.
  Dla surogatu się nie udaje, więc **wysyłane jest zdarzenie z keycode 0, czyli
  `kVK_ANSI_A`**: emoji daje na Macu „aa”, a nie „nic”, jak zakładało
  F-003a.4.
- **Flagi.** To zdarzenie niesie też bieżące flagi modyfikatorów, bo `post`
  (`cgevent.rs:89-92`) stempluje je na każdym zdarzeniu. Czy przy wciśniętym
  Cmd macOS wyzwoli wtedy skrót — do potwierdzenia testem ręcznym AC-10.

- **F-003a.14** (MUST) Składanie par surogatów MUSI być czystą maszyną stanów
  w module `input` (bez wywołań macOS), z **osobnym buforem dla wciśnięć
  i dla zwolnień**.
  - Wysoki surogat (U+D800–U+DBFF) jest buforowany.
  - Niski surogat (U+DC00–U+DFFF), który przychodzi bezpośrednio po wysokim
    w tym samym kierunku, daje **jeden** znak skalarny i **jedno** zdarzenie
    w tym kierunku.
  - Kolejność zdarzeń klienta (`↓H ↓L ↑H ↑L` albo `↓H ↑H ↓L ↑L`) NIE MOŻE
    wpływać na wynik: w obu przypadkach powstaje jedno wciśnięcie i jedno
    zwolnienie złożonego znaku.
- **F-003a.15** (MUST) Samotny surogat MUSI zostać odrzucony z wpisem `debug`.
  Samotny to:
  - niski bez poprzedzającego wysokiego;
  - wysoki, po którym w tym samym kierunku przychodzi coś innego niż niski;
  - wysoki pozostawiony w buforze w chwili resetu (F-003a.17).

  Dla odrzuconego surogatu NIE WOLNO wstrzyknąć żadnego zdarzenia klawiatury.
  W szczególności żadne zdarzenie Unicode nie może trafić do systemu
  z keycode 0 bez ustawionego tekstu. Zdarzenie, które przerwało parę,
  przetwarzamy normalnie.
- **F-003a.16** (MUST) Zdarzenie Unicode MUSI być wstrzyknięte z **wyzerowanymi
  flagami modyfikatorów**. Stan modyfikatorów serwera (F-003.12) się nie
  zmienia, więc następne zdarzenie scancode dostaje flagi jak dotąd.
  - To rozstrzyga pytanie z F-003a.5 („czyścić flagi czy budować je
    świadomie”) na korzyść czyszczenia. ADR-0008 wybiera tylko między
    ścieżkami scancode i Unicode, a skróty i tak idą ścieżką pozycyjną
    (F-003a.2).
  - Jeśli ADR-0008 uzna to za błąd, zmiana idzie przez aktualizację tego
    wymagania, a nie po cichu w kodzie.
- **F-003a.17** (MUST) Bufor surogatów MUSI być czyszczony przy każdym resecie
  stanu wejścia: po Synchronize (F-003.22) i na końcu połączenia (F-003.24).
- **F-003a.18** (MUST) Testy jednostkowe MUSZĄ pokrywać przypadki wymienione
  w AC-9, a każdy test MUSI mieć w nazwie albo komentarzu ID wymagania.
  Istniejące testy przechodzą bez zmian (F-003a.12).
- **F-003a.19** (SHOULD) **Draft (2026-09-18).** Diagnostyka odbioru
  klawiatury. `session` POWINIEN logować na poziomie `trace` każde zdarzenie
  klawiatury odebrane od IronRDP, **przed** konwersją na nasz `KeyEvent`.
  - Wpis zawiera rodzaj zdarzenia (scancode, Unicode, Synchronize), kod
    (scancode z flagą `extended` albo jednostkę UTF-16 szesnastkowo) i kierunek.
  - Target logu: `mac_rdp_server::session::input`. Tylko `trace`, nigdy
    `debug` ani wyżej: taki log zawiera wszystko, co użytkownik pisze, także
    hasła.
  - Uzasadnienie: AC-10 nie przeszło 2026-09-18. Strona Maca jest sprawdzona
    (zob. §8), więc trzeba ustalić, czy mstsc w ogóle wysyła zdarzenia Unicode
    dla emoji z panelu Win+.

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-003a.1 | MS-RDPBCGR §2.2.8.1.1.3.1.1.1 (Keyboard Event, scancode set 1, flagi `KBDFLAGS_EXTENDED`) |
| F-003a.4 | RDP Unicode Keyboard Event (jednostka UTF-16); Unicode: pary surogatów |
| F-003a.5, F-003a.3 | Apple: `CGEventKeyboardSetUnicodeString`, `CGEventCreateKeyboardEvent`, `CGEventSetFlags` |
| F-003a.8 | MS-RDPBCGR Client Core Data `keyboardLayout` (KLID); do zweryfikowania w API IronRDP |
| F-003a.7 | Apple `UCKeyTranslate` / TSM (jeśli ścieżka scancode ma obsłużyć dead keys po naszej stronie) |
| F-003a.14, F-003a.15 | MS-RDPBCGR 1.1 Glossary „Unicode character” (str. 19); 2.2.8.1.1.3.1.1.2 (str. 171); 2.2.8.1.2.2.2 (str. 179); Unicode Standard, UTF-16 (zakresy surogatów D800–DBFF, DC00–DFFF) |
| F-003a.16 | Apple: `CGEventSetFlags`, `CGEventKeyboardSetUnicodeString`; `specs/02-ms-rdpbcgr.md` §4 P-05 |
| F-003a.17 | F-003.22, F-003.24 |

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

**Uzupełnienie 2026-09-17 — naprawa ścieżki Unicode (F-003a.14–F-003a.18).**

9. **AC-9** Testy jednostkowe maszyny stanów (część CI):
   - `↓D83D ↓DE00` ⇒ jedno wciśnięcie znaku 😀 (U+1F600);
     `↑D83D ↑DE00` ⇒ jedno zwolnienie (F-003a.14);
   - kolejność przeplatana `↓D83D ↑D83D ↓DE00 ↑DE00` ⇒ ten sam wynik
     (F-003a.14);
   - samotne `↓DE00` ⇒ brak zdarzeń (F-003a.15);
   - `↓D83D ↓0061` ⇒ wyłącznie wciśnięcie `a`; wysoki surogat odrzucony
     (F-003a.15);
   - `↓D83D`, potem reset ⇒ brak zdarzeń; kolejne `↓DE00` też nic nie daje
     (F-003a.15, F-003a.17);
   - znak z BMP `↓0105` ⇒ jedno wciśnięcie `ą`, bez buforowania (F-003a.14);
   - decyzja o flagach: zdarzenie Unicode przy trzymanym Cmd ma flagi = 0,
     a następne zdarzenie scancode ma Cmd (F-003a.16).
10. **AC-10** Checklista ręczna:
    - emoji wpisane z klienta (np. z panelu emoji Windows, Win+.) pojawia się
      w edytorze na Macu jako **jeden** znak, bez dodatkowych liter „a”;
    - to samo z wciśniętym i puszczonym wcześniej Cmd: żaden skrót się nie
      wyzwala;
    - po Alt+Tab po stronie klienta w trakcie wpisywania emoji nic nie „wisi”
      (F-003a.17).
11. **AC-11** (F-003a.19) Z `MAC_RDP_LOG=debug,mac_rdp_server::session::input=trace`
    każde naciśnięcie klawisza w kliencie daje w logu wpis z kodem i kierunkiem.
    Przy `MAC_RDP_LOG=debug` takich wpisów nie ma.

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

**Uzupełnienie 2026-09-17:** F-003a.14–F-003a.18 — **Approved** (2026-09-17,
właściciel), osobno i przed ADR-0008, bo naprawiają błąd widoczny dziś
(`02-ms-rdpbcgr.md` §4 P-05) i nie przesądzają strategii. Grupa jest częścią
Fali 3 (`waves.md`).

**Implemented (2026-09-18):** F-003a.14–F-003a.18 (commity `fe3e66d`,
`8828b38`, `f4978d8`). Testy jednostkowe z ID w nazwie:
`input::unicode::tests::f003a_*`, `input::cgevent::tests::f003a_*`. Czeka na
test ręczny właściciela (AC-10, BRAMKA B z `goals.md`) → Verified.

**Test ręczny 2026-09-18** (właściciel; mstsc, Windows 11 build 26100):
- **AC-10 — NIE PRZESZŁO.** Emoji wybrane z panelu Win+. (tryb okienkowy,
  panel otwiera się lokalnie na Windows) nie pojawia się na Macu. W logu brak
  wpisów o odrzuconych surogatach, ale ścieżka Unicode nie loguje odebranych
  zdarzeń, więc nie wiadomo, czy mstsc cokolwiek wysłał. Obie strony
  deklarują `InputFlags::UNICODE` (Confirm Active). Przyczyna do ustalenia
  przed Verified.

**Diagnoza AC-10, 2026-09-18.** Program testowy spoza repo (scratchpad) wstrzyknął
do TextEdit 😀 i `ą` dokładnie tak jak `CgEventSink`, czyli:
- keycode 0, łańcuch UTF-16, flagi 0, tap HID, źródło `HIDSystemState`.

Wszystkie sześć wariantów dotarło:
- HID i session tap;
- źródła HID i combined;
- `postToPid`;
- łańcuch tylko na key-down.

Strona Maca działa. Podejrzenie: mstsc nie wysyła emoji z panelu Win+.
jako zdarzeń Unicode, co jest do potwierdzenia przez F-003a.19 (Draft).
