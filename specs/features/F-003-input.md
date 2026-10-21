# F-003 — Input: sterowanie myszą i klawiaturą

- **Fala:** 2
- **Status:** Approved (2026-07-24)
- **Data:** 2026-07-24
- **ADR:** ADR-0003 (CGEvent), ADR-0001 (silnik), ADR-0007 (środkowy przycisk — ograniczenie IronRDP)
- **Powiązane:** F-003a (mapowanie klawiatury — osobny spec, bo to najtrudniejsza część)
- **Uzupełnienie 2026-09-17 (Verified (2026-09-18, właściciel)):** F-003.22–F-003.26 — reset stanu wejścia
  (Synchronize, koniec połączenia). Źródło: przegląd `02-ms-rdpbcgr.md` §4 P-04.
  Wymagania F-003.1–F-003.21 bez zmian.
- **Uzupełnienie 2026-10-21 (Draft):** F-003.27–F-003.32 — licznik kliknięć
  (dwuklik). Źródło: zgłoszenie właściciela + pomiar `MOUSE_EVENT_CLICK_STATE`.
  Dotyczy niezaliczonej połowy AC-3. Pozostałe wymagania bez zmian.

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
  **Brak usunięty 2026-10-12 (ADR-0014).** Do tej daty środkowy przycisk był
  zmapowany poprawnie u nas, ale nie działał end-to-end: `ironrdp-server`
  0.13.0 z crates.io gubił `PTRFLAGS_BUTTON3` w `From<MousePdu>` (dekodował go
  jako `Move`), a publicznego haka na surowy PDU nie ma — zdarzenie nigdy nie
  docierało do naszej konwersji (ADR-0007, teraz Superseded). Silnik pochodzi
  dziś z naszego forka, w którym ta ścieżka jest naprawiona. Nasz kod nie
  zmienił się ani o linię; doszły dwa testy, oba padające, gdyby fork stracił
  łatkę: jednostkowy
  `session::convert::tests::f003_7_a_middle_button_pdu_arrives_as_the_middle_button`
  (start od `MousePdu`) i integracyjny `tests/wire_input.rs`, w którym
  prawdziwy klient IronRDP łączy się po TLS i wysyła Share Data Input PDU,
  a test czyta, co wyszło z kolejki wejścia serwera. Nic nie jest wstrzykiwane
  w macOS — odbiornik kolejki należy do testu, więc nie trzeba żadnego
  uprawnienia TCC i nic nie klika po pulpicie.
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

### Reset stanu wejścia — uzupełnienie 2026-09-17

**Status tej grupy: Verified (2026-09-18, właściciel)** (Approved 2026-09-17, Implemented 2026-09-18). Pozostałe wymagania F-003 zachowują
swój status.

Kontekst. MS-RDPBCGR 2.2.8.1.1.3.1.1.5 (str. 173) definiuje Synchronize Event
jako zdarzenie, które ustawia przełączniki (Caps/Num/Scroll Lock) **i „resetuje
stan klawiszy serwera do wszystko puszczone”**. Klient SHOULD po nim dosłać
wciśnięcia klawiszy i przycisków, które faktycznie trzyma. Fast-path ma
odpowiednik w 2.2.8.1.2.2.5 (str. 181).

Dziś `src/input/cgevent.rs:175-177` na `KeyEvent::SyncLocks` ustawia tylko
Caps Lock. Trzymane modyfikatory (`Modifiers`) i przyciski (`Buttons`) zostają.
Sink żyje przez cały proces (`src/input/mod.rs`), więc taki stan **przecieka
także między połączeniami**: klient rozłączony z wciśniętym Cmd albo w trakcie
przeciągania zostawia Maca z „wiszącym” klawiszem. Zdarzenie Synchronize
przychodzi typowo przy powrocie fokusu do okna klienta, np. po Cmd+Tab/Alt+Tab
po stronie klienta.

- **F-003.22** (MUST) Po `KeyEvent::SyncLocks` serwer MUSI zwolnić **każdy**
  modyfikator i **każdy** przycisk myszy, który według jego stanu jest
  wciśnięty. Zwolnienie MUSI zostać **wstrzyknięte do systemu** prawdziwymi
  zdarzeniami (`FlagsChanged`/key-up dla modyfikatorów,
  `{Left,Right,Other}MouseUp` dla przycisków). Samo wyzerowanie stanu
  w pamięci nie wystarcza, bo macOS nadal uważałby klawisz za trzymany.
  Dopiero **po** zwolnieniach serwer ustawia przełączniki zgodnie z F-003.13.
- **F-003.23** (MUST) Wciśnięcia, które klient dosyła po Synchronize, MUSZĄ być
  przetwarzane normalnie, w kolejności odbioru. Reset NIE MOŻE ich zgubić ani
  wyprzedzić. Warunek spełnia to, że reset jedzie tym samym uporządkowanym
  kanałem co zdarzenia wejścia (`01-architecture.md` §4a).
- **F-003.24** (MUST) Koniec połączenia — czysty, z błędem albo zerwany —
  MUSI wywołać to samo zwolnienie stanu co F-003.22, **bez** zmiany
  przełączników. Nowe połączenie MUSI zaczynać ze stanem „wszystko puszczone”.
  - Sygnał końca połączenia MUSI dotrzeć do `input` jako **nasz** typ zdarzenia,
    tym samym kanałem co wejście — analogicznie do `InputEvent::Geometry`
    (F-007.17). Typy IronRDP nie wychodzą poza `session` (granica nr 1
    z `CLAUDE.md`).
  - Źródło sygnału w `session` wybiera implementacja. Musi pokrywać zerwanie
    połączenia. Kandydaci w API 0.13.0: `ConnectionHandler::on_disconnected`
    i `on_accept` (`ironrdp-server-0.13.0/src/server.rs:70-91`) albo początek
    `RdpServerDisplay::updates()`.
  - Wysłanie sygnału NIE MOŻE blokować: obowiązuje `try_send`, tak samo jak
    w F-003.4.
- **F-003.25** (MUST) Decyzja „co zwolnić” MUSI być czystą funkcją modułu
  `input::state`, testowalną bez uprawnień macOS. Na podstawie stanu
  modyfikatorów i przycisków zwraca listę zwolnień i stan po resecie.
  `cgevent.rs` tylko wstrzykuje wynik.
- **F-003.26** (SHOULD) Każdy reset POWINIEN dać jeden wpis `debug`
  z przyczyną (`sync` albo `disconnect`) i liczbą zwolnionych
  modyfikatorów i przycisków. Wpisy nie mogą zalewać logu przy pustym stanie
  — wtedy wystarczy `trace`.

### Licznik kliknięć (dwuklik) — uzupełnienie 2026-10-21

**Status tej grupy: Draft (2026-10-21)** — czeka na approve właściciela.
Pozostałe wymagania F-003 zachowują swój status.

Kontekst. AC-3 wymaga dwukliku od 2026-07-24 i został zaliczony 2026-07-27
razem z pojedynczym klikiem — ale dwuklik **nigdy nie działał**. Zgłoszenie
właściciela 2026-10-21: „szybki 2 klik nie działa", przy wchodzeniu do folderu
w Finderze i w folder na stronie Dysku Google. Ta połowa AC-3 jest więc
nieprawdziwa od początku, a nie zregresowana.

**Pomiar 2026-10-21** (sonda budująca `CGEvent` bez wysyłania go, więc nic nie
kliknęła po pulpicie):

| zdarzenie | `MOUSE_EVENT_CLICK_STATE` |
|---|---|
| świeżo zbudowany `LeftMouseDown` | **1** |
| świeżo zbudowany `LeftMouseUp` | **0** |
| po `set_integer_value_field(…, 2)` | 2 |

`build_button_event` (`src/input/cgevent.rs:261`) nigdy tego pola nie ustawia,
więc **każde** wciśnięcie, jakie wstrzykujemy, niesie `clickState = 1`. AppKit
podaje tę wartość aplikacji jako `NSEvent.clickCount`, a Finder i przeglądarki
otwierają element przy `clickCount == 2`. Dwa szybkie kliknięcia są dla
aplikacji dwoma niezależnymi pojedynczymi klikami i żadne przyspieszenie po
stronie klienta tego nie zmieni. To ta sama klasa błędu co środkowy przycisk
w F-003.7: zdarzenie wygląda poprawnie, dopóki nie sprawdzi się pola, od
którego macOS faktycznie zależy.

Liczby kliknięć **nie ma na łączu**: MS-RDPBCGR 2.2.8.1.1.3.1.1.3 (Mouse Event)
niesie tylko `pointerFlags`, `xPos` i `yPos`, a Extended Mouse Event
(2.2.8.1.1.3.1.1.4) dokłada wyłącznie przyciski 4/5. Serwer MUSI ją wyliczyć
sam — nie ma czego przepisać z PDU.

- **F-003.27** (MUST) Sink MUSI utrzymywać licznik kliknięć i ustawiać go
  w polu `MOUSE_EVENT_CLICK_STATE` **zarówno** dla `…MouseDown`, jak i dla
  `…MouseUp` — prawdziwy sprzęt niesie w obu tę samą wartość, a nasze `Up`
  niesie dziś 0 (pomiar wyżej).
- **F-003.28** (MUST) Licznik MUSI rosnąć o 1, gdy wciśnięcie spełnia
  **wszystkie** warunki: ten sam przycisk co poprzednie, odstęp od poprzedniego
  wciśnięcia nie większy niż interwał z F-003.29, a kursor nie dalej niż
  `CLICK_SLOP` punktów od pozycji poprzedniego wciśnięcia. W każdym innym
  przypadku MUSI wrócić do 1. Warunek odległości nie jest ozdobą: klient
  wysyła ruch i przyciski osobno (F-003.8), więc bez niego dwa szybkie
  kliknięcia w **różne** foldery otworzyłyby ten drugi.
- **F-003.29** (MUST) Interwał dwukliku MUSI pochodzić z ustawień systemu
  (`NSEvent.doubleClickInterval`), nie być stałą w kodzie. Czytany raz przy
  tworzeniu sinka. **Pomiar 2026-10-21:** metoda działa w procesie **bez**
  `NSApplication` (czyli w trybie CLI), jest w `objc2-app-kit` 0.3.2 **bezpieczna**
  (nie wymaga `unsafe`, więc nie rusza `unsafe_code = "deny"`), i u właściciela
  zwraca `0.5 s`; klucze `defaults` (`com.apple.mouse.doubleClickThreshold`)
  u niego nie istnieją, więc czytanie preferencji ręcznie dałoby nic.
- **F-003.30** (MUST) Licznik NIE MOŻE być obcięty do 2 — potrójne kliknięcie
  (zaznaczenie akapitu) MUSI dawać 3, tak jak prawdziwa mysz.
- **F-003.31** (MUST) Reset stanu wejścia (F-003.22 … F-003.26: Synchronize
  i koniec połączenia) MUSI zerować licznik, żeby pierwszy klik nowej sesji
  nie dokleił się jako dwuklik do poprzedniej.
- **F-003.32** (MUST) Decyzja o wartości licznika MUSI być **czystą funkcją**
  w `src/input/state.rs`, z czasem podanym jako argument i pokrytą testami
  jednostkowymi (dwuklik w interwale, za późno, za daleko, inny przycisk,
  potrójny, po resecie). `cgevent.rs` tylko ustawia pole — tak jak mówi
  nagłówek tego modułu o podziale „co wstrzyknąć" / „jak wstrzyknąć".

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-003.1, F-003.4 | `ironrdp_server::RdpServerInputHandler` (synchroniczny); `01-architecture.md` §4a |
| F-003.5–F-003.9 | Apple, Quartz Event Services: `CGEventCreateMouseEvent`, `CGEventCreateScrollWheelEvent`, `CGEventPost` |
| F-003.11–F-003.15 | MS-RDPBCGR §2.2.8.1.1.3 (Keyboard Event, scancode set 1); Apple: `CGEventCreateKeyboardEvent`, `CGEventFlags` |
| F-003.16 | Apple: `AXIsProcessTrusted`; ADR-0003 §5 |
| F-003.6 | `DisplayInfo.scale_factor` z Fali 1 (F-002.5) |
| F-003.22, F-003.23 | MS-RDPBCGR 2.2.8.1.1.3.1.1.5 Synchronize Event (str. 173–174); 2.2.8.1.2.2.5 Fast-Path Synchronize Event (str. 181); `specs/ref/MS-RDPBCGR-v20260309.txt` |
| F-003.24 | `ironrdp-server-0.13.0/src/server.rs:70-91` (`ConnectionHandler`); `specs/02-ms-rdpbcgr.md` §4 P-04 |
| F-003.25 | `src/input/state.rs` (`Modifiers`, stan przycisków); ADR-0003 §3 |
| F-003.27–F-003.32 | MS-RDPBCGR 2.2.8.1.1.3.1.1.3 Mouse Event (brak liczby kliknięć na łączu) i 2.2.8.1.1.3.1.1.4 Extended Mouse Event; Apple: `kCGMouseEventClickState`, `NSEvent.clickCount`, `NSEvent.doubleClickInterval` |

## 5. Kryteria akceptacji

Sprawdzane ręcznie prawdziwym klientem.

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, wszystkie testy.
2. **AC-2** Ruch myszą w oknie klienta rusza kursorem Maca; pozycja się zgadza
   (kliknięcie w róg trafia w róg, nie w środek — dowód poprawnej konwersji
   piksele→punkty).
3. **AC-3** Lewy i prawy przycisk działają: klik, podwójny klik, menu
   kontekstowe prawym. **Środkowy przycisk wrócił do kryterium 2026-10-12**
   (ADR-0014 zdjął ograniczenie zapisane w ADR-0007) — patrz AC-16.
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

**Uzupełnienie 2026-09-17 — reset stanu wejścia (F-003.22–F-003.26).**
Testy jednostkowe (część CI, AC-11) i checklista ręczna (AC-12…AC-15):

11. **AC-11** Testy jednostkowe dla F-003.25. Każdy ma w nazwie albo komentarzu
    ID wymagania.
    - Trzymane lewy Shift, prawy Cmd i lewy przycisk; reset ⇒ lista zwolnień
      zawiera dokładnie te trzy pozycje, `cg_flags()` bez modyfikatorów,
      rodzaj ruchu `MouseMoved`, a nie `Dragged`.
    - Pusty stan; reset ⇒ pusta lista.
    - Reset z `SyncLocks { caps: true, .. }` ⇒ po resecie Caps Lock włączony,
      a zwolnienia wypadają **przed** ustawieniem przełącznika (F-003.22).
    - Reset z przyczyny `disconnect` nie zmienia Caps Lock (F-003.24).
    - Wciśnięcie Shift po resecie ⇒ Shift znów trzymany (F-003.23).
12. **AC-12** **Przełączenie okien po stronie klienta.** Na kliencie wciśnij
    Alt+Tab (Windows) albo Cmd+Tab (klient na Macu). Przełącz się na inną
    aplikację, puść klawisze, wróć do okna RDP. Na Macu pisanie daje małe
    litery i nie wyzwala skrótów — żaden modyfikator nie „wisi”. Powtórz
    10 razy.
13. **AC-13** **Rozłączenie z wciśniętym Cmd.** Trzymając Cmd w oknie klienta,
    zamknij okno klienta (albo odetnij sieć). Na **lokalnej** klawiaturze Maca
    pisanie działa normalnie. Po ponownym połączeniu z klienta również.
14. **AC-14** **Rozłączenie w trakcie przeciągania.** Złap okno na Macu za pasek
    tytułu i w trakcie przeciągania rozłącz klienta. Okno zostaje puszczone
    i nie „przykleja się” do lokalnego kursora.
15. **AC-15** Skróty z AC-7 (Cmd+C/V, Cmd+Tab na Macu przez klienta) działają
    bez zmian — reset ich nie zepsuł.

**Uzupełnienie 2026-10-12 — środkowy przycisk (F-003.7, ADR-0014).**

16. **AC-16** **Klik rolką działa end-to-end.** Z prawdziwego klienta kliknij
    rolką:
    - w Safari/Chrome w link ⇒ otwiera się w nowej karcie;
    - w karcie przeglądarki ⇒ karta się zamyka;
    - w Terminalu/iTermie po zaznaczeniu tekstu ⇒ wkleja zaznaczenie
      (X11-owe „primary selection” działa w iTermie, jeśli włączone; jeśli nie,
      wystarczą dwa pierwsze).
    Przewijanie rolką działa dalej i **nie** jest brane za klik (AC-5).
    Sprawdzić przynajmniej jednym klientem z rodziny mstsc i jednym z rodziny
    FreeRDP/Windows App — oba idą ścieżką `MousePdu`, ale wysyłają różne
    kombinacje flag.

**Uzupełnienie 2026-10-21 — licznik kliknięć (F-003.27 … F-003.32).**

17. **AC-17** **Dwuklik działa.** W Finderze dwuklik na folderze wchodzi do
    niego; na stronie Dysku Google dwuklik na folderze otwiera go. Trzy razy
    pod rząd, bo chodzi o powtarzalność, nie o jedno trafienie.
18. **AC-18** **Dwuklik nie powstaje przypadkiem.** Dwa szybkie kliknięcia
    w **różne** foldery (odstęp mniejszy niż interwał systemowy, kursor
    przeniesiony między nimi) nie otwierają żadnego z nich — dowód na warunek
    odległości z F-003.28.
19. **AC-19** **Potrójny klik** w polu tekstowym zaznacza akapit (F-003.30).
20. **AC-20** **Klik pojedynczy się nie zepsuł.** Zaznaczanie ikon jednym
    klikiem, menu kontekstowe prawym i przeciąganie (AC-4) działają jak
    dotąd — licznik nie ma prawa zmienić zachowania pojedynczego kliknięcia.

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

5. **`CLICK_SLOP` — wartość do ustalenia.** macOS ma własny próg odległości
   dla dwukliku i nie jest on publicznie udokumentowany. Propozycja: **4 punkty**
   (ciasno, bo klient RDP wysyła pozycję bezwzględną, a nie drżącą rękę na
   myszy), jako stała nazwana w `state.rs`, do skorygowania po AC-17/AC-18,
   jeśli dwuklik będzie „gubiony" przy minimalnym ruchu kursora.

## 8. Status

**Verified** (2026-07-27).

**Sprostowanie 2026-10-21 do AC-3.** „Lewy i prawy przycisk — działa"
zaliczone 2026-07-27 obejmowało pojedynczy klik i menu kontekstowe; **dwuklik,
wymieniony w AC-3 od początku, nie działał i nie działa** (zgłoszenie
właściciela 2026-10-21, przyczyna zmierzona — patrz §3, uzupełnienie
2026-10-21). AC-3 pozostaje w tej połowie niezaliczone do czasu AC-17.

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

**Uzupełnienie 2026-09-17:** F-003.22–F-003.26 (reset stanu wejścia) —
**Approved** (2026-09-17, właściciel). Status Verified dotyczy wyłącznie
F-003.1–F-003.21. Grupa jest częścią Fali 3 (`waves.md`, poprawki zgodności
z MS-RDPBCGR).

**Implemented (2026-09-18):** F-003.22–F-003.26 (commity `5e57e3e`, `d0169ed`,
`f4978d8`). Testy jednostkowe z ID w nazwie: `input::state::tests::f003_2*`,
`input::cgevent::tests::f003_2*`, `input::tests::f003_24_*`,
`session::input::tests::f003_24_*`. Czeka na test ręczny właściciela
(AC-11…AC-15, BRAMKA B z `goals.md`) → Verified.

**Test ręczny 2026-09-18** (właściciel; mstsc, Windows 11 build 26100; logi
`target/measure-mstsc-3.log`, `target/measure-mstsc-4.log`, lokalne):
- **AC-11** — CI zielone (fmt, clippy `-D warnings`, 151 testów).
- **AC-12** — działa w trybie okienkowym: po powrocie Alt+Tab pisanie bez
  wiszących modyfikatorów. Każdy powrót daje w logu
  `input state reset, nothing held cause="sync"` (trace) — mstsc sam wysyła
  zwolnienie Alt przed oddaniem fokusu, więc serwer nie ma czego zwalniać.
  W trybie pełnoekranowym Alt+Tab trafia do Maca jako Option+Tab i nie
  przełącza okien Windows — to ustawienie mstsc („Apply Windows key
  combinations”), nie błąd serwera.
- **AC-13** — działa i jest potwierdzone logiem (08:50:46Z):
  `input state reset cause="disconnect" modifiers=1 buttons=0` po zamknięciu
  mstsc z wciśniętym Win (Cmd). Lokalna klawiatura i ponowne połączenie bez
  skrótów.
- **AC-14** — okno puszczone; w logu brak `buttons=1`, bo w trakcie
  przeciągania nie da się kliknąć zamknięcia mstsc (przycisk puszczony przed
  rozłączeniem). Ścieżkę zwalniania przycisku pokrywają tylko testy jednostkowe.
- **AC-15** — Win+Tab (= Cmd+Tab) przełącza aplikacje na Macu; Caps Lock
  zgodny z klientem po zmianie fokusu.
- Poza zakresem, zaobserwowane: schowek między maszynami nie działa — F-102.

**Verified (2026-09-18, właściciel):** F-003.22–F-003.26 — AC-11…AC-15 spełnione
(wyniki wyżej).

**Uzupełnienie 2026-10-12 — środkowy przycisk (F-003.7).** Silnik pochodzi od
tej daty z naszego forka IronRDP (ADR-0014, rewizja
`ac4cd7db5e09a3293ffe2cede01b41e17a78bf2e`), w którym `From<MousePdu>`
rozpoznaje `PTRFLAGS_BUTTON3`. Nasz kod wstrzykiwania i konwersji nie zmienił
się; doszedł jeden test pokrywający ścieżkę od PDU z drutu
(`session::convert::tests::f003_7_a_middle_button_pdu_arrives_as_the_middle_button`),
jeden integracyjny przez całą drogę po drucie (`tests/wire_input.rs`) oraz
cztery testy w samym forku (`server::mouse::*` w `ironrdp-testsuite-core`).
ADR-0007 → Superseded.

**Dowód, że test łapie dokładnie tę regresję.** Ten sam `tests/wire_input.rs`
uruchomiony na drzewie z zależnościami z crates.io **pada**, i pada w sposób,
który jeden do jednego potwierdza analizę z ADR-0007: oba kliknięcia środkowym
przychodzą jako `MoveAbsolute { x: 32, y: 64 }`, czyli pozycja z PDU, zamiast
`Button { Middle }`. Scroll w tej samej paczce zdarzeń jest poprawny w obu
wersjach — czyli łatka nie zamieniła obrotu rolki w klik, co przy fladze
nazwanej „middle button **or wheel**" było realnym ryzykiem.
**AC-3 i AC-16 czekają na test ręczny właściciela** — dopóki go nie ma,
środkowy przycisk jest „Implemented”, nie „Verified”.
