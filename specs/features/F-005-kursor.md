# F-005 — Kursor jako wskaźnik RDP, nie piksele w klatce

- **Fala:** 3 (Jakość MVP)
- **Status:** Draft
- **Data:** 2026-08-24
- **ADR:** **ADR-0010 (do napisania — źródło obrazu kursora na macOS)**, ADR-0005 (stride), ADR-0009 (damage)
- **Zależności:** F-004 (Implemented) — F-005 domyka jego zysk; **blokuje** pomiar AC-6 z F-004 w wersji docelowej

## 1. Cel

Kursor przestaje być wypalany w klatkę ekranu, a zaczyna być osobnym wskaźnikiem
RDP rysowanym lokalnie przez klienta — dzięki czemu ruch myszy przestaje
generować damage, a wskaźnik przestaje być opóźniony o rundę sieciową.

## 2. Dlaczego teraz, a nie „kiedyś"

Dziś `build_config` ustawia `with_shows_cursor(true)` (`src/capture/screen.rs:455`),
czyli ScreenCaptureKit wkomponowuje kursor w każdą klatkę. Do Fali 2 to nie
kosztowało nic, bo i tak szła cała klatka. Po F-004 kosztuje **dwa razy**:

1. **Damage.** Kursor jest ruchomym obiektem na statycznym pulpicie. Każde
   drgnięcie myszy brudzi dwa kafelki (stara pozycja + nowa), więc sam ruch
   myszy po pustym pulpicie generuje ruch sieciowy. To jest dokładnie ten
   przypadek, dla którego F-004 powstał, i jedyny, którego F-004 nie umie
   rozwiązać po swojej stronie.
2. **Opóźnienie.** Kursor narysowany w klatce jedzie do klienta ścieżką
   capture → enkoder → sieć → dekoder. Użytkownik widzi wskaźnik spóźniony
   o pełną rundę. Kursor jako PDU jest rysowany przez klienta natychmiast,
   lokalnie, i opóźnienie znika **całkowicie**, niezależnie od łącza.

Punkt 2 jest odczuwalnie ważniejszy od punktu 1. Opóźniony wskaźnik to
pierwsza rzecz, którą człowiek zauważa jako „muli", i żadna optymalizacja
pasma tego nie naprawi.

## 3. Zbadany stan faktyczny (2026-08-24)

To jest sekcja, od której zależy kształt wymagań. Wszystko poniżej odczytane
ze źródeł pinowanych wersji, nie z pamięci.

### 3a. Co wystawia IronRDP

`DisplayUpdate` (`ironrdp-server-0.13.0/src/display.rs:20-29`) ma sześć
wariantów wskaźnika: `PointerPosition`, `ColorPointer`, `RGBAPointer`,
`HidePointer`, `DefaultPointer`, `CachedPointer(u16)`. Wysyłamy je tym samym
kanałem co bitmapy — `RdpServerDisplayUpdates::next_update`
(`src/session/display.rs:65`), bez żadnego nowego interfejsu.

`RGBAPointer` (`display.rs:32-39`) niesie `cache_index`, `width`, `height`,
`hot_x`, `hot_y`, `data: Vec<u8>`. Enkoder składa z tego `TS_POINTERATTRIBUTE`
z `xor_bpp: 32` i **pustą maską AND** (`encoder/mod.rs:185-205`), i wysyła jako
`UpdateCode::NewPointer`.

### 3b. Format bufora — trzy rzeczy, które trzeba trafić

Odczytane z referencyjnego dekodera `ironrdp-graphics-0.9.0/src/pointer.rs`,
czyli z tego, co klient robi z naszym buforem:

| Co | Fakt | Źródło |
|---|---|---|
| **Kolejność wierszy** | **Od dołu do góry.** `flip_vertical = xor_bpp != 1`, więc dla 32 bpp dekoder czyta wiersz `height - row - 1` | `pointer.rs:194`, `:220-226` |
| **Kolejność bajtów** | **BGRA** — dekoder czyta 4 bajty i zwraca `[b[2], b[1], b[0], b[3]]` jako RGBA | `pointer.rs:401-404` |
| **Długość maski** | Dokładnie `width * 4 * height`, bez wyrównania — dla 32 bpp `stride == width * 4` i tak jest parzysty | `pointer.rs:196-204` |

BGRA to **ten sam układ, co nasze klatki** (`PixelFormat::Bgra8`, ADR-0005),
więc konwersji kanałów nie ma. Odwrócenie pionowe jest, i jest jedyną
nieoczywistą operacją w całym enkoderze.

Pusta maska AND jest bezpieczna: dekoder podstawia za nią same zera
(`pointer.rs:207-210`), a `and_bit == 0` znaczy „skopiuj piksel z maski XOR
razem z jego alfą". Czyli **przezroczystość jedzie kanałem alfa** i nic więcej
nie trzeba wysyłać.

### 3c. Twardy limit rozmiaru — główne ryzyko tego speca

Dwa niezależne ograniczenia:

1. **`ironrdp-server` 0.13.0 nie ogłasza `CapabilitySet::LargePointer`.**
   Lista zdolności jest zamknięta (`src/capabilities.rs:6-16`) i pozycji
   `LargePointer` w niej nie ma; serwer nie czyta też tej zdolności od klienta.
   Per MS-RDPBCGR 2.2.9.1.1.4.4 bez `LARGE_POINTER_FLAG_96x96` formalny limit
   wskaźnika to **32×32**, a nie 96×96.
2. **Pole długości maski jest `u16`** (`ironrdp-pdu-0.9.0/src/basic_output/pointer/mod.rs:70-102`,
   `check_masks_alignment!` z `large_ptr = false`). Nawet gdyby klient był
   pobłażliwy, `128 × 128 × 4 = 65536` przekracza `u16::MAX` i **enkodowanie
   się wywali**, zanim cokolwiek pójdzie w sieć.

Kursor macOS na Retinie ma zwykle 2× wymiar w punktach, więc typowa strzałka
(~24 pt) to ~48 px — **powyżej limitu 32×32**. To nie jest szczegół do
przemilczenia: albo skalujemy w dół, albo rysujemy z reprezentacji 1×, albo
sprawdzamy empirycznie, ile realny klient przyjmie.

**Hipoteza do zweryfikowania w AC-6:** `mstsc` i FreeRDP w praktyce przyjmują
wskaźniki większe niż 32×32 mimo braku ogłoszonej zdolności. Jeśli tak —
dopuszczamy 48×48 lub 64×64 przełącznikiem i zapisujemy wynik. Jeśli nie —
zostajemy na 32×32 i kursor na Retinie jest lekko miękki. **Nie zgadujemy;
domyślna wartość to 32, a pomiar decyduje o zmianie.**

### 3d. Czego macOS nam **nie** daje

- **`screencapturekit` 8.0.1 nie wystawia obrazu kursora.** Jedyne, co ma, to
  przełącznik `with_shows_cursor` (`src/stream/configuration/captured_elements.rs:9-18`).
- **`core-graphics` 0.25.0 nie ma API obrazu kursora.** Ma `hide_cursor`,
  `show_cursor`, `warp_mouse_cursor_position` (`src/display.rs:593-643`) —
  i nic, co zwraca bitmapę. Nie ma też `CGCursorIsVisible`.
- **W drzewie zależności nie ma `objc2` ani żadnego runtime'u ObjC.**
  `screencapturekit` idzie przez `apple-cf` + `apple-metal` (`Cargo.lock:2827-2830`).
- **Crate ma `unsafe_code = "deny"`** (`Cargo.toml`, sekcja `[lints.rust]`).

Publiczne API, które daje obraz kursora, to `NSCursor.currentSystemCursor`
z AppKit. Sięgnięcie po nie znaczy **nową zależność ObjC i rozstrzygnięcie, co
z `unsafe_code = "deny"`**. To jest decyzja architektoniczna, nie detal
implementacji — stąd wymóg ADR-0010 **przed** kodem (§9).

Znane pułapki `currentSystemCursor`, które ADR-0010 musi rozstrzygnąć, a nie
odkryć w trakcie: zwraca `nil` dla kursorów spoza zestawu systemowego (kursory
rysowane przez aplikację), wymaga wątku głównego, i nie ma publicznej
notyfikacji „kursor się zmienił" — trzeba odpytywać.

## 4. Wymagania

### Rdzeń: źródło kursora (strona macOS)

- **F-005.1** (MUST) `build_config` ustawia `with_shows_cursor(false)`
  (`src/capture/screen.rs:455`). Kursor znika z klatek — to jest cała zmiana
  po stronie capture'u i musi obowiązywać także przy każdej przebudowie
  strumienia przez watchdog (F-007.14).
- **F-005.2** (MUST) Powstaje `src/capture/cursor.rs` z naszym typem
  `CursorState`. Moduł **nie zna RDP** (`01-architecture.md` §1): nie zna
  `cache_index`, nie zna kolejności wierszy, nie zna limitów rozmiaru.
  Wystawia bitmapę BGRA, jej wymiary, hotspot i identyfikator kształtu.
- **F-005.3** (MUST) Stan kursora jedzie do sesji przez `tokio::sync::watch`,
  czyli **koalescuje się, nie kolejkuje** (CLAUDE.md §Granice 4). Wolny klient
  ma dostać ostatni kształt, nie wszystkie pośrednie.
- **F-005.4** (MUST) Odpytywanie kursora dzieje się we **własnym zadaniu**,
  z własną częstotliwością, i **nie leży na ścieżce klatek**. Zablokowanie się
  źródła kursora nie może zatrzymać obrazu.
- **F-005.5** (MUST) Bitmapa idzie na drut z alfą **prostą**
  (non-premultiplied) — tak MS-RDPBCGR opisuje 32-bitową maskę XOR, i tak
  traktuje ją dekoder, który sam decyduje o premnożeniu przy rysowaniu
  (`ironrdp-graphics-0.9.0/src/pointer.rs:260-269`). AppKit oddaje bitmapy
  **premultiplied**, więc źródło MUSI je od-premnożyć; wysłanie premnożonych
  da ciemną obwódkę wokół cienia kursora. Test jednostkowy MUSI pokryć piksel
  półprzezroczysty. **Do potwierdzenia w AC-2** — patrz §9 pkt 6.
- **F-005.6** (MUST) Hotspot jest wyrażony w **pikselach wysyłanej bitmapy**,
  nie w punktach macOS. Skalowanie bitmapy (F-005.12) skaluje hotspot razem
  z nią.
- **F-005.7** (MUST) Brak dostępnego kursora (`nil`, błąd, kursor rysowany
  przez aplikację) NIE MOŻE panikować ani zatrzymywać obrazu. Reakcja:
  `DisplayUpdate::DefaultPointer` i **jeden** wpis w logu na przejście stanu,
  nie na próbkę.

### Rdzeń: enkoder wskaźnika (logika czysta)

- **F-005.8** (MUST) Powstaje `CursorEncoder` w `src/encoder/cursor.rs`.
  Jest **czysty i testowalny bez GUI**: dostaje `CursorState`, oddaje
  `Vec<PointerUpdate>` — nasz typ, nie typ IronRDP.
- **F-005.9** (MUST) Wiersze bufora XOR są emitowane **od dołu do góry**
  (§3b). Test jednostkowy MUSI wykryć regresję na kolejności naturalnej —
  bitmapa asymetryczna w pionie, sprawdzana bajt po bajcie.
- **F-005.10** (MUST) Długość bufora XOR to dokładnie `width * 4 * height`.
  Test jednostkowy MUSI to asertować, bo rozjazd tutaj to błąd dekodowania
  po stronie klienta, a nie nasz panic.
- **F-005.11** (MUST) Maska AND jest **pusta**; przezroczystość idzie wyłącznie
  kanałem alfa (§3b).
- **F-005.12** (MUST) Bitmapa większa niż limit MUSI zostać przeskalowana
  w dół, nigdy przycięta. Limit jest konfigurowalny (`--pointer-max-size`),
  **domyślnie 32** (§3c). Wartość powyżej 127 MUSI zostać odrzucona przy
  starcie z czytelnym komunikatem, bo `128 × 128 × 4` przepełnia pole `u16`
  i wywala enkoder IronRDP.
- **F-005.13** (MUST) Ten sam kształt kursora wysłany drugi raz idzie jako
  `CachedPointer(index)`, nie jako pełna bitmapa. Identyczność liczona
  z bitmapy **i** hotspotu — dwa kursory o tych samych pikselach i różnym
  hotspocie to różne kursory.
- **F-005.14** (SHOULD) Cache ma stały rozmiar (start: 32 pozycje) i eksmisję
  LRU. Serwer ogłasza `pointer_cache_size: 2048`
  (`ironrdp-server-0.13.0/src/capabilities.rs:61-65`), więc 32 jest bezpieczne
  z ogromnym zapasem.
- **F-005.15** (MUST) Cache jest **per połączenie**, budowany razem z enkoderem
  klatek (`src/session/mod.rs:66-72`), i pierwsza aktualizacja po połączeniu
  zawsze wysyła pełny kształt. Nie zakładamy niczego o tym, co klient pamięta
  z poprzedniej sesji — to ten sam błąd, który ADR-0009 §1 opisuje dla klatek.

### Sesja i granica architektoniczna

- **F-005.16** (MUST) Zamiana `PointerUpdate` → `DisplayUpdate` żyje wyłącznie
  w `src/session/convert.rs`. `tests/architecture.rs` (F-001.12) musi nadal
  przechodzić bez zmian w regule.
- **F-005.17** (MUST) `next_update` obsługuje dwa źródła — klatki i kursor —
  i **pozostaje cancel-safe**, bo jest awaitowane w `tokio::select!`
  po stronie IronRDP (`src/session/display.rs:60-65`). Aktualizacje już
  wyprodukowane siedzą w `pending`, nie w zmiennej lokalnej.
- **F-005.18** (MUST) Strumień klatek nie może **zagłodzić** wskaźnika:
  przy 30 fps i ciągłym ruchu obrazu kursor musi się nadal aktualizować.
  Sprawdzalne testem integracyjnym na sztucznym źródle, bez GUI.

### Konfiguracja i diagnostyka

- **F-005.19** (MUST) Przełącznik `--cursor pointer|burned`, domyślnie
  `pointer`. `burned` przywraca `with_shows_cursor(true)` i nie wysyła żadnych
  PDU wskaźnika — czyli zachowanie Fali 1. To jest **ścieżka diagnostyczna**,
  ta sama rola co `--encoder full` (F-004.16): oddziela „kursor jest zły" od
  „obraz jest zły" bez przebudowy.
- **F-005.20** (SHOULD) Log co 10 s, w rytmie statystyk damage'u z F-004:
  liczba wysłanych kształtów, trafień w cache, bajtów wskaźnika. Bez tego
  AC-4 jest opinią, nie pomiarem.

## 5. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-005.9, F-005.10, F-005.11 | MS-RDPBCGR 2.2.9.1.1.4.4 (`TS_COLORPOINTERATTRIBUTE`), 2.2.9.1.1.4.5 (`TS_POINTERATTRIBUTE`); `ironrdp-graphics-0.9.0/src/pointer.rs:194,220-226,401-404` |
| F-005.12 | MS-RDPBCGR 2.2.7.2.7 (`TS_LARGE_POINTER_CAPABILITYSET`); `ironrdp-server-0.13.0/src/capabilities.rs:6-16`; `ironrdp-pdu-0.9.0/src/basic_output/pointer/mod.rs:70-102` |
| F-005.13, F-005.14 | MS-RDPBCGR 2.2.9.1.1.4.6 (`TS_CACHEDPOINTERATTRIBUTE`), 2.2.7.1.5 (Pointer Capability Set); `ironrdp-server-0.13.0/src/capabilities.rs:61-65` |
| F-005.1 | `SCStreamConfiguration.showsCursor` (docs Apple, ScreenCaptureKit); `screencapturekit-8.0.1/src/stream/configuration/captured_elements.rs:9-18` |
| F-005.5 | MS-RDPBCGR 2.2.9.1.1.4.4 (`xorMaskData`, 32 bpp); `NSBitmapImageRep` / alfa premultiplied (docs Apple); `ironrdp-graphics-0.9.0/src/pointer.rs:260-269` |
| F-005.3, F-005.17 | `01-architecture.md` §4b (koalescencja przez `watch`), CLAUDE.md §Granice 4 |
| F-005.2, F-005.16 | `01-architecture.md` §1; `tests/architecture.rs` |
| — | ADR-0010 (do napisania), ADR-0005 (stride i układ bajtów), ADR-0009 (damage) |

## 6. Kryteria akceptacji

CI (AC-1) jest automatyczne. AC-2…AC-7 to **checklista ręczna**, wymagana przez
CLAUDE.md §Testy dla każdego speca dotykającego macOS. Wykonywana na prawdziwym
kliencie, wynik wpisywany do §8.

1. **AC-1** CI zielone: `fmt`, `clippy -D warnings`, testy — w tym nowe testy
   jednostkowe: odwrócenie pionowe (F-005.9), długość bufora (F-005.10),
   od-premnożenie alfy (F-005.5), skalowanie z hotspotem (F-005.12),
   trafienie i eksmisja w cache (F-005.13/.14).
2. **AC-2** **Kursor w ogóle się pojawia i jest właściwy.** Na kliencie widać
   wskaźnik; nad tekstem zmienia się w belkę, nad krawędzią okna w strzałkę
   zmiany rozmiaru, nad linkiem w łapkę. Kształty **nie są** przekłamane:
   nic nie jest do góry nogami (F-005.9), kolory się zgadzają (§3b),
   półprzezroczysty cień nie ma czarnej obwódki (F-005.5).
3. **AC-3** **Hotspot trafia.** Klik w mały cel — pole wyboru, przycisk
   zamknięcia okna — trafia tam, gdzie wskazuje ostrze kursora, a nie obok.
   Sprawdzone dla strzałki **i** dla belki tekstowej (jej hotspot jest
   pośrodku, nie w rogu — to najlepszy test tego, że hotspot naprawdę jedzie
   ze wskaźnikiem, a nie jest zgadywany).
4. **AC-4** **Zysk zmierzony liczbą.** Przy nieruchomym pulpicie i **ciągle
   poruszanej myszy** statystyki damage'u z F-004 §7 pokazują `unchanged`
   bliskie `frames_in`. Ta sama sesja z `--cursor burned` pokazuje przeciwnie.
   Obie liczby wpisane do §8. To jest właściwy pomiar AC-6 z F-004.
5. **AC-5** **Płynność wskaźnika.** Kursor nadąża za ręką bez odczuwalnego
   opóźnienia, także wtedy, gdy obraz jest zajęty (przewijanie długiej strony).
   To jest test F-005.18: obraz może zwalniać, wskaźnik nie.
6. **AC-6** **Rozmiar na Retinie.** Zapisane, przy jakim `--pointer-max-size`
   klient jeszcze rysuje kursor: 32, 48, 64. Osobno dla `mstsc` i dla
   `xfreerdp`. To rozstrzyga hipotezę z §3c i decyduje o wartości domyślnej.
7. **AC-7** **Ścieżka diagnostyczna działa.** `--cursor burned` daje obraz
   identyczny z Falą 1 — kursor wypalony w klatkę, zero PDU wskaźnika w logu.
8. **AC-8** **Brak duchów przy rozłączeniu.** Rozłączenie i ponowne
   połączenie daje poprawny kursor od pierwszej klatki (F-005.15), bez
   „pustego" wskaźnika czekającego na pierwszą zmianę kształtu.

## 7. Poza zakresem

- **`PointerPosition` — synchronizacja pozycji.** W iteracji 1 **nie
  wysyłamy** pozycji wskaźnika. Klient rysuje kursor tam, gdzie sam ma mysz,
  co jest poprawne dla zdalnego użytkownika; niewidoczne zostaje tylko to, że
  ktoś fizycznie przy Macu rusza myszą albo że aplikacja przestawia kursor
  programowo. Iteracja 2 (**F-005.21**, SHOULD): wysyłać `PointerPosition`
  **tylko** wtedy, gdy pozycja zmieniła się z przyczyny innej niż wejście
  tego klienta — inaczej wskaźnik będzie się szarpał, walcząc z ruchem
  lokalnym klienta. Pozycję da się czytać bez nowych zależności:
  `CGEvent::new(source)?.location()` (`core-graphics-0.25.0/src/event.rs:767`).
- **`HidePointer` — ukrywanie kursora.** macOS chowa wskaźnik przy pisaniu
  i w wideo pełnoekranowym, a `core-graphics` 0.25.0 **nie ma
  `CGCursorIsVisible`** (§3d), więc nie mamy publicznego sposobu, żeby to
  wykryć. Iteracja 2 (**F-005.22**, MAY), po rozpoznaniu w ADR-0010.
- **Kursory rysowane przez aplikację** (nie z zestawu systemowego) —
  `currentSystemCursor` zwraca dla nich `nil`. Reakcją jest `DefaultPointer`
  (F-005.7), nie obsługa.
- **`ColorPointer` (24 bpp z maską AND)** — starsze klienty. Wysyłamy wyłącznie
  `RGBAPointer`. Gdyby okazało się to potrzebne, jest to osobna iteracja.
- **Ogłoszenie `LargePointer` / wskaźniki 96×96 i 384×384** — wymaga zmiany
  w `ironrdp-server` (§3c), czyli PR-a upstream albo F-200. Poza tą falą.
- **Kursor przy wielu ekranach** — F-105.

## 8. Wyniki pomiaru

*(do wypełnienia — AC-4, AC-6)*

| Scenariusz | `unchanged` / `frames_in` | bajty wskaźnika | uwagi |
|---|---|---|---|
| Nieruchomy pulpit, mysz w ruchu, `--cursor pointer` | | | |
| Nieruchomy pulpit, mysz w ruchu, `--cursor burned` | | | |

| Klient | 32×32 | 48×48 | 64×64 |
|---|---|---|---|
| `mstsc` | | | |
| `xfreerdp` | | | |

## 9. Ryzyka i otwarte kwestie

1. **`unsafe_code = "deny"` kontra AppKit.** To jest największe ryzyko tego
   speca i **jedyny powód, dla którego ADR-0010 musi powstać przed kodem**.
   Do rozstrzygnięcia: czy sięgamy po `objc2` + `objc2-app-kit`, czy po
   cieńszą zależność, i czy `deny` zostaje z wyjątkiem ograniczonym do jednego
   modułu, czy schodzi na `warn`. Zdejmowanie tego lintu globalnie „bo tak
   wyszło" byłoby cichą zmianą decyzji architektonicznej — czyli dokładnie
   tym, czego CLAUDE.md zakazuje.
2. **Limit 32×32 może zepsuć wygląd kursora na Retinie.** Wynik AC-6 zdecyduje.
   Gorszy scenariusz: 32×32 wygląda miękko, a nic większego nie przechodzi —
   wtedy to jest **znany, udokumentowany brak**, nie porażka fali.
3. **Odpytywanie zamiast notyfikacji.** Nie ma publicznego zdarzenia „kursor
   się zmienił", więc częstotliwość jest kompromisem: za rzadko — kształt
   zmienia się z opóźnieniem, za często — marnujemy CPU. Start: 30 Hz,
   do skorygowania po pomiarze. Wątek główny (§3d) może to dodatkowo
   ograniczać i to też jest kwestia dla ADR-0010.
4. **`currentSystemCursor` a wątek główny.** Jeżeli okaże się, że wywołanie
   wymaga pętli zdarzeń AppKit, a nie tylko wątku głównego, to zmienia
   strukturę procesu (`main` musiałby oddać wątek główny AppKitowi).
   **To trzeba sprawdzić w ADR-0010 najmniejszym możliwym programem, zanim
   powstanie reszta F-005** — odkrycie tego w połowie implementacji byłoby
   przepisaniem `main`, a nie poprawką.
5. **Regresja przy przebudowie strumienia.** `with_shows_cursor(false)` żyje
   w `build_config`, wspólnym dla startu i dla odbudowy przez watchdog
   (F-007.14). Jeśli kiedyś się rozjadą, kursor wróci do klatek po pierwszym
   zaniku strumienia — dlatego F-005.1 mówi o `build_config`, a nie
   o „konfiguracji przy starcie".
6. **Alfa: prosta czy premnożona — klienty mogą się różnić.** MS-RDPBCGR
   opisuje maskę XOR jako zwykłe piksele 32-bitowe, ale traktowanie kanału
   alfa jest historycznie miejscem, w którym implementacje się rozjeżdżają:
   referencyjny dekoder IronRDP w trybie programowym stosuje własne
   przekształcenie kanałów koloru (`pointer.rs:260-269`), a `mstsc` i FreeRDP
   nie muszą robić tego samego. Objaw rozjazdu jest charakterystyczny i łatwy
   do rozpoznania w AC-2: **ciemna obwódka wokół półprzezroczystego cienia
   kursora**. Jeśli wystąpi, poprawką jest przełącznik wyboru wariantu alfy,
   a nie zgadywanie — i zapis wyniku per klient w §8.

## 10. Status

**Draft** — czeka na approve właściciela.

Approve F-005 oznacza zgodę na:

- **(a)** napisanie **ADR-0010** (źródło obrazu kursora na macOS + rozstrzygnięcie
  `unsafe_code`) **przed** jakimkolwiek kodem — na wzór F-003a → ADR-0008;
- **(b)** nową zależność ObjC w `Cargo.toml`, o ile ADR-0010 tak rozstrzygnie;
- **(c)** zakres iteracji 1 = kształt kursora, bez `PointerPosition`
  i bez `HidePointer` (§7);
- **(d)** domyślny limit wskaźnika 32×32, do zmiany dopiero po wyniku AC-6.
