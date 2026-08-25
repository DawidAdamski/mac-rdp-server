# F-004 — Damage tracking: wysyłamy tylko to, co się zmieniło

- **Fala:** 3 (Jakość MVP)
- **Status:** Implemented (2026-08-07); weryfikacja w toku (2026-08-25) — AC-1/2/6/7 zaliczone, AC-3/AC-4 do zrobienia
- **Data:** 2026-08-07
- **ADR:** ADR-0009 (źródło damage'u), ADR-0005 (stride), ADR-0006 (kodeki)
- **Zależności:** brak wejściowych; **blokuje** sensowność F-005 (kursor)

## 1. Cel

Serwer wysyła klientowi wyłącznie zmienione fragmenty ekranu zamiast pełnej
klatki, tak żeby praca na statycznym lub prawie statycznym pulpicie przestała
generować stały, pełnoklatkowy ruch sieciowy.

## 2. Kontekst liczbowy

Dzisiaj `FullFrameEncoder` produkuje jedną aktualizację pełnoekranową na klatkę
(`src/encoder/full_frame.rs:22`). Przy Retinie 3024×1964 BGRA to **23,8 MB
surowo na klatkę**, do 30 razy na sekundę. Kompresja RDP 6.0 po stronie IronRDP
to zbija, ale kompresuje **całą klatkę, także niezmienioną** — koszt CPU i pasma
jest ponoszony niezależnie od tego, czy cokolwiek się ruszyło.

Przy pisaniu w edytorze zmienia się kilka tysięcy pikseli. Wysyłamy siedem
milionów.

## 3. Wymagania

### Rdzeń: wykrywanie zmian

- **F-004.1** (MUST) Powstaje `DamageEncoder` implementujący `Encoder`
  (`src/encoder/mod.rs:23`), w nowym module `src/encoder/damage.rs`.
- **F-004.2** (MUST) `DamageEncoder` MUSI trzymać kopię **ostatniej klatki
  wysłanej** przez ten enkoder i porównywać z nią klatkę wejściową
  (ADR-0009 §1). NIE WOLNO opierać poprawności na `dirty_rects()` z SCK.
- **F-004.3** (MUST) Porównanie MUSI być kafelkowe: siatka o stałym rozmiarze
  kafelka (domyślnie 64×64 px), kafelek brudny, gdy różni się choćby jednym
  bajtem w obszarze **widocznym** (pierwsze `width*bpp` bajtów wiersza —
  bajty wypełnienia stride'u są ignorowane).
- **F-004.4** (MUST) Cała arytmetyka na buforze MUSI używać
  `y * stride + x * bpp` (ADR-0005). Test jednostkowy MUSI pokryć przypadek
  `stride > width * 4` i wykryć regresję na `y * width * bpp`.
- **F-004.5** (MUST) Brudne kafelki MUSZĄ być scalane w prostokąty przed
  wysłaniem — nie wysyłamy każdego kafelka osobno.

### Kontrakt z enkoderem IronRDP

Ograniczenia zbadane w `ironrdp-server-0.13.0/src/encoder/bitmap.rs`,
uzasadnienie w ADR-0009 §4.

- **F-004.6** (MUST) Szerokość każdego wysyłanego regionu MUSI być
  wielokrotnością 4. Prostokąty MUSZĄ być w tym celu poszerzane (`x` w dół,
  `width` w górę), nigdy zwężane — zwężenie gubi piksele.
- **F-004.7** (MUST) Poszerzony prostokąt MUSI być przycięty do granic klatki.
- **F-004.8** (MUST) `FrameUpdate::Region` MUSI nieść bufor **zwarty**
  (`stride == width * bpp`), skopiowany z klatki źródłowej (ADR-0009 §4).
  Wycinek zero-copy jest jawnie poza zakresem tej iteracji.
- **F-004.9** (MUST) Serwer MUSI walidować przy starcie, że szerokość pulpitu
  jest wielokrotnością 4, i odmówić z **czytelnym komunikatem**, jeśli nie jest.
  Dziś to założenie jest milczące: naruszenie daje `Width must be a multiple
  of 4` z wnętrza IronRDP, bez kontekstu.

### Warianty awaryjne

- **F-004.10** (MUST) Enkoder MUSI wysłać **pełną klatkę**, gdy: (a) to pierwsza
  klatka tego połączenia, (b) zmieniła się geometria klatki (szerokość, wysokość
  lub stride), (c) udział brudnych kafelków przekracza próg, (d) liczba
  regionów po scaleniu przekroczyłaby limit na klatkę.
- **F-004.11** (MUST) Progi z F-004.10 (c) i (d) oraz rozmiar kafelka MUSZĄ być
  stałymi nazwanymi w jednym miejscu, z uzasadnieniem wartości w komentarzu.
- **F-004.12** (MUST) Klatka bez żadnych zmian MUSI dać **zero aktualizacji**
  (pusty `Vec`). `MacRdpDisplayUpdates::next_update` już to obsługuje — pętla
  czeka na następną klatkę (`src/session/display.rs:65-79`).

### Warstwa sesji

- **F-004.13** (MUST) `FrameUpdate` dostaje wariant `Region { x, y, frame }`,
  zgodnie z `01-architecture.md` §2 (dziś enum ma tylko `Full`,
  `src/encoder/mod.rs:19`).
- **F-004.14** (MUST) `session::convert::display_update` MUSI mapować `Region`
  na `DisplayUpdate::Bitmap` z niezerowym `x`/`y`. Funkcja
  `convert::bitmap_update(frame, x, y)` już przyjmuje offset
  (`src/session/convert.rs:33`) — nie powstaje nowa ścieżka konwersji.
- **F-004.15** (MUST) `session::run` MUSI zbudować `DamageEncoder` zamiast
  `FullFrameEncoder` (`src/session/mod.rs:58`). `FullFrameEncoder` **zostaje
  w drzewie** jako referencja i ścieżka diagnostyczna.
- **F-004.16** (SHOULD) Wybór enkodera POWINIEN być przełączalny flagą
  (`--encoder full|damage`), żeby przy diagnozie obrazu dało się wrócić do
  ścieżki znanej z Fali 1 bez rekompilacji.

### Pomiar

- **F-004.17** (MUST) Enkoder MUSI liczyć i logować okresowo: klatki wejściowe,
  klatki pominięte jako niezmienione, wysłane regiony, wysłane bajty, udział
  powierzchni. Bez tego „jest szybciej" jest opinią.
- **F-004.18** (MUST) Zysk MUSI być **zmierzony liczbą** w dwóch scenariuszach:
  statyczny pulpit i przewijanie tekstu. Wynik trafia do §7 tego speca.

### Testy

- **F-004.19** (MUST) Testy jednostkowe (czyste, bez GUI i bez RDP) MUSZĄ pokryć:
  identyczne klatki ⇒ zero aktualizacji; zmiana jednego piksela ⇒ dokładnie
  jeden region zawierający ten piksel; zmiana w dwóch odległych rogach ⇒ dwa
  rozłączne regiony; zmiana geometrii ⇒ pełna klatka; przekroczenie progu ⇒
  pełna klatka; `stride > width*4` ⇒ poprawne regiony (F-004.4);
  wyrównanie szerokości do wielokrotności 4 (F-004.6) w tym przy krawędzi
  prawej (F-004.7).
- **F-004.20** (MUST) Test na pikselach u klienta (rozszerzenie
  `tests/rdp_pixels.rs`) MUSI potwierdzić, że po aktualizacji regionalnej
  **region ma nową treść, a reszta ekranu starą i nienaruszoną**. To jest
  asercja wprost z lekcji ADR-0006 §„Czego ten incydent nauczył o testach":
  każda zmiana tego, co widzi użytkownik, dostaje asercję na pikselach.
- **F-004.21** (SHOULD) Test regresyjny na duchy: sekwencja klatek, w której
  region zmienia się i wraca do poprzedniej treści, POWINNA dać u klienta
  obraz zgodny z ostatnią klatką źródłową.

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-004.6, F-004.9 | `ironrdp-server-0.13.0/src/encoder/bitmap.rs:30-35` — `Width must be a multiple of 4` |
| F-004.8 | `ironrdp-server-0.13.0/src/encoder/bitmap.rs:44,50,56-58` — cięcie `data` po `stride`, czytanie `&row[..row_len]` |
| F-004.13, F-004.14 | `01-architecture.md` §2 (`FrameUpdate::Region`); MS-RDPBCGR 2.2.9.1.1.3.1.2 (Bitmap Update) |
| F-004.2 | ADR-0009; `01-architecture.md` §4b (koalescencja przez `watch`) |
| F-004.4 | ADR-0005 |
| — | MS-RDPBCGR 2.2.9.1.1.3.1.2.2 (Bitmap Data), 3.3.5.5.1 (Sending Bitmap Updates) |

## 5. Kryteria akceptacji

1. **AC-1** CI zielone: `fmt`, `clippy -D warnings`, wszystkie testy, w tym
   nowe testy jednostkowe damage'u i asercja regionalna w `tests/rdp_pixels.rs`.
2. **AC-2** **Statyczny pulpit, prawdziwy klient:** po ustabilizowaniu obrazu
   ruch sieciowy spada do bliskiego zera. W logu widać klatki pomijane jako
   niezmienione. Obraz **nie** znika ani nie zamiera — po ruszeniu myszą
   wszystko wraca natychmiast.
3. **AC-3** **Pisanie w edytorze:** znaki pojawiają się bez opóźnienia
   większego niż dotąd; wysyłane są regiony, nie pełne klatki (widoczne w logu).
4. **AC-4** **Przewijanie strony w przeglądarce:** obraz jest spójny — bez
   pasów starej treści, bez przesunięć, bez skosu. To najostrzejszy test
   scalania regionów.
5. **AC-5** **Brak duchów po zmianie okna:** przełączenie aplikacji (Cmd+Tab),
   zamknięcie okna, przeciągnięcie okna przez ekran — nigdzie nie zostaje
   fragment poprzedniej treści. Sprawdzone także przy **sztucznie spowolnionym
   kliencie** (niski `--max-fps`), bo to scenariusz, w którym koalescencja gubi
   najwięcej klatek (ADR-0009 §2).
6. **AC-6** **Zmierzony zysk** (F-004.18) wpisany do §7: bajty/s i klatki/s
   przed i po, dla statycznego pulpitu i dla przewijania.
7. **AC-7** Wymuszenie pełnej klatki (`--encoder full`) nadal działa i daje
   obraz identyczny z Falą 1 — ścieżka diagnostyczna jest sprawna.

## 6. Poza zakresem

- **Wycinek zero-copy regionu** — ADR-0009 §4 i warunek bezpieczeństwa tam
  zapisany. Optymalizacja na później, po pomiarze.
- **`dirty_rects()` jako podpowiedź zawężająca porównanie** — iteracja 2 F-004,
  wymaga sumowania po `sequence` (ADR-0009 §3). Nie zmienia poprawności.
- **Kursor** — F-005. Uwaga: bez F-005 kursor jest wypalany w klatkę
  (`with_shows_cursor(true)`, `src/capture/screen.rs:439`), więc **sam ruch
  myszy będzie generować damage**. To jest oczekiwane i nie jest błędem
  F-004; domyka to dopiero F-005.
- **Nowsze kodeki (RemoteFX/EGFX)** — F-104, zablokowane nierozstrzygniętą
  przyczyną z ADR-0006 §„Otwarta kwestia".
- **Zmiana rozmiaru pulpitu w trakcie sesji** — F-006. F-004 na zmianę
  geometrii reaguje pełną klatką (F-004.10 b), nie resize'em.

## 7. Wyniki pomiaru

**Zmierzone 2026-08-25.** Sprzęt: jeden ekran 5120×1440, `scale_factor = 1.0`,
`stride = 20480 = width × 4` (bez dopełnienia). Klient na osobnej maszynie
w LAN, `--bind-addr 0.0.0.0`, `--max-fps 30`, enkoder `damage`.

### Metoda — jedna pułapka, która kosztowała nas pierwszy przebieg

**Okno, w którym obserwujesz pomiar, nie może stać na przechwytywanym ekranie.**
Brzmi banalnie, ale przy jednym ekranie nie ma gdzie uciec, a skutek jest
drastyczny: pierwsze 170 sekund pomiaru miało `unchanged = 0` przez **4867
kolejnych klatek**, bo na pulpicie stało okno terminala ze strumieniowanym
tekstem. Przemalowywało się w rytm klatek i pulpit nigdy nie był statyczny.

Wnioski, żeby nie powtarzać: przekierowuj log **do pliku, bez `tee`**,
i minimalizuj okna na czas pomiaru. `unchanged = 0` przy rzekomo statycznym
pulpicie jest sygnałem, że mierzysz coś innego, niż myślisz — a nie usterką
enkodera.

### Wyniki

Oba wiersze pochodzą z **tej samej sesji i tego samego połączenia** — zmieniło
się wyłącznie to, co było na ekranie. To pomiar różnicowy, nie dwa niezależne
przebiegi, więc różnica nie może wynikać z innego klienta, enkodera czy stanu
sieci.

| Scenariusz | okno | `bytes_if_full` | `bytes_sent` | `saved_pct` | `unchanged` / `frames_in` | ruch |
|---|---|---|---|---|---|---|
| **Statyczny pulpit** | 89 s | 10 085 990 400 | 6 864 896 | **99,93%** | 74 / 342 | **77 KB/s** |
| Ruchliwy pulpit (okno ze strumieniowanym tekstem) | 10 s | 8 640 921 600 | 114 262 016 | 98,68% | 0 / 293 | 11,4 MB/s |

Cała sesja narastająco (10 063 klatki, ~16 min): `bytes_sent = 2 958 819 328`,
`bytes_if_full = 296 769 945 600`, `saved_pct = 99`, `full_frames = 2`.

**Zysk na statycznym pulpicie to 152× mniej ruchu** niż na ruchliwym i
**1400× mniej** niż pełna klatka za każdym razem (77 KB/s zamiast 113 MB/s).

### Trzy obserwacje, których wymaganie nie przewidywało

1. **`frames_in` spada z ~29 do ~3,8 klatki na sekundę**, gdy ekran stoi.
   ScreenCaptureKit **sam** przestaje produkować klatki — więc oszczędzamy
   także CPU na porównywaniu, zanim damage tracking się w ogóle odezwie.
   To odpowiada częściowo na ryzyko §8 pkt 1: koszt `memcmp` nie jest ponoszony
   30 razy na sekundę na statycznym pulpicie, bo nie ma czego porównywać.
2. **Próg „za dużo regionów" zadziałał raz i słusznie.** Jedyna pełna klatka
   poza pierwszą poszła z powodem `too many separate regions`, w ruchliwym
   okresie — czyli reguła F-004.10 c) odpaliła tam, gdzie miała.
3. **Kursor wypalony w klatkę jest widoczny gołym okiem jako artefakt.**
   Przy ruchu myszą klient pokazuje **dwa** wskaźniki: własny, rysowany
   lokalnie i natychmiast, oraz kursor Maca docierający kafelkami z opóźnieniem
   rundy. Dokładnie to, co §6 zapowiadało jako świadomy koszt — i niezależne
   potwierdzenie, że F-005 musi iść zaraz po F-004.

### Stan kryteriów akceptacji

| AC | Stan |
|---|---|
| AC-1 CI | **Zaliczone** |
| AC-2 statyczny pulpit | **Zaliczone** (2026-08-25) — liczby wyżej; obraz nie zamarł, po ruchu myszy wraca natychmiast |
| AC-3 pisanie w edytorze | Do sprawdzenia |
| AC-4 przewijanie strony | Do sprawdzenia |
| AC-5 brak duchów | **Częściowo** — przeciąganie okna i animacja minimalizacji: aktualizacja postępuje kafelkami przy szybkim ruchu, ale po zatrzymaniu obraz **sam** staje się poprawny, bez resztek. Zostaje Cmd+Tab, zamknięcie okna i wersja ostra z niskim `--max-fps` |
| AC-6 zmierzony zysk | **Zaliczone** — ta sekcja |
| AC-7 `--encoder full` | **Zaliczone** (2026-08-25) — obraz poprawny, jak w Fali 1, potwierdzone wzrokowo przez właściciela; liczby niżej |

### AC-7 — porównanie z `--encoder full`, ta sama treść

Ścieżka diagnostyczna nie tylko działa; przy okazji dała najmocniejszy dotąd
argument za samym F-004. `FullFrameEncoder` nie robi nic poza przepakowaniem
klatki (`src/encoder/full_frame.rs:22`), więc mierzy się tu koszt oddania
IronRDP-owi pełnych 29,5 MB co klatkę.

Miarą jest `delivered` z `capture stats` — ile klatek dotarło do enkodera.
Capture w obu trybach produkował równe ~29,3 klatki na sekundę; różnica to
wyłącznie to, ile konsument zdążył przerobić, a resztę zjadła koalescencja
(`01-architecture.md` §4b).

| Scena | `--encoder damage` | `--encoder full` |
|---|---|---|
| Film **w oknie** | **28,4–29,3 fps** | 5,4–9,6 fps |
| Film **pełny ekran** | 2,0–3,6 fps | ~1,9 fps |

**Wniosek, który obala hipotezę postawioną w trakcie pomiaru.** W trakcie
testów pojawiło się podejrzenie, że damage tracking dubluje pracę, bo
`ironrdp-server` prowadzi **własny** diff względem własnego framebuffera
(`ironrdp-server-0.13.0/src/encoder/mod.rs:240-242`, `USE_DIFFS = true`), więc
F-004 mógłby być zbędny, a ADR-0009 do rewizji. **Pomiar mówi odwrotnie:**
przy filmie w oknie F-004 daje **pięciokrotnie więcej klatek**. ADR-0009
zostaje bez zmian, a jego uzasadnienie z argumentu z rozsądku staje się liczbą.

**Sufit jest wspólny.** Na pełnym ekranie oba tryby zrównują się przy ~2 fps.
Gdy zmienia się cała powierzchnia, nie ma czego nie wysyłać i decyduje wyłącznie
przepustowość kopiowania i kompresji RDP 6.0 dla 5120×1440. To jest punkt
odniesienia dla **F-104** (EGFX/nowsze kodeki) i **F-107** (zero-copy).

### Jak powtórzyć pomiar

Enkoder sam liczy wszystko, czego ten pomiar potrzebuje, i wypisuje to co 10 s:

```
MAC_RDP_LOG="info,mac_rdp_server=debug" ./target/release/mac-rdp-server
...
DEBUG damage stats  frames_in=300 unchanged=284 full_frames=1 regions=37
                    bytes_sent=4718592 bytes_if_full=8847360000 saved_pct=99
```

- `bytes_if_full` — ile poszłoby przy zachowaniu z Fali 1 (ścieżka `--encoder full`).
- `bytes_sent` — ile faktycznie poszło.
- `saved_pct` — różnica, czyli wynik.

Sesja musi trwać **ponad 10 s**, bo tyle wynosi interwał; licznik jest
per-połączenie, więc nowe połączenie zaczyna od zera.

Wyniki są w tabelach wyżej. Scenariusze jeszcze niezmierzone — **pisanie
w edytorze (AC-3)** i **przewijanie strony (AC-4)** — dopisujemy tam, nie tutaj.

## 8. Ryzyka i otwarte kwestie

1. **Koszt `memcmp` całej klatki.** Przy braku zmian to ~714 MB/s skanowania
   pamięci (30 fps × 23,8 MB). Powinno być ułamkiem rdzenia, ale **to jest
   założenie do zweryfikowania pomiarem**, nie fakt. Jeśli okaże się istotne,
   iteracja 2 z `dirty_rects()` zbija to do zera dla statycznego pulpitu.
2. **Rozmiar kafelka i próg pełnej klatki** to parametry, których optymalna
   wartość zależy od obciążenia. Start 64×64 i 60% jest zgadywanką popartą
   rozsądkiem — po pomiarze mogą się zmienić i to nie będzie porażką projektu.
3. **Scalanie regionów przy przewijaniu.** Przewijanie brudzi cały widoczny
   obszar, więc scalanie prawdopodobnie od razu przekroczy próg i wyśle pełną
   klatkę. To poprawne zachowanie, ale znaczy, że **przewijanie nie zyska na
   F-004** — zysk jest przy pracy punktowej. Warto to wiedzieć przed pomiarem,
   żeby nie uznać poprawnego wyniku za błąd.
4. **Znany efekt uboczny: obraz składa się kafelkami i oko czyta to jako
   szarpanie.** Zaobserwowane 2026-08-25, uznane przez właściciela za znany
   efekt uboczny do ewentualnej poprawy później.

   Przy szybkim ruchu — animacja minimalizacji okna, przeciąganie okna, wideo —
   widać moment, w którym część obrazu jest już nowa, a część jeszcze stara.
   Każdy region jedzie osobnym `BitmapUpdate`, a klient rysuje go, gdy tylko
   dotrze, więc nie ma na drucie niczego, co powiedziałoby „ta klatka jest
   kompletna". Efekt **mija sam** po zatrzymaniu ruchu: obraz staje się
   poprawny bez interwencji, więc **to nie są duchy** i nie narusza AC-5.

   Paradoks, który to rodzi i który trzeba znać przed czytaniem pomiarów:
   `--encoder full` potrafi *wyglądać* płynniej, mimo że dostarcza **pięć razy
   mniej klatek**. Daje ich mało, ale każda jest kompletna. Ocena „płynniejsze"
   gołym okiem mierzy więc spójność klatki, a nie liczbę klatek — i te dwie
   rzeczy idą tu w przeciwne strony.

   **Czy da się to naprawić wg specyfikacji — zbadane 2026-08-25.**
   Protokół ma na to mechanizm: **Frame Marker**, którym serwer oznacza początek
   i koniec spójnej klatki, żeby klient narysował ją w całości zamiast kawałkami.
   Problem w tym, że oba jego warianty są dla nas dziś zamknięte:

   - **Surface Frame Marker.** `ironrdp-pdu` 0.9.0 **ma** ten PDU
     (`SurfaceCommand::FrameMarker`, `src/basic_output/surface_commands/`), ale
     jest on częścią Surface Commands — ścieżki, której świadomie **nie
     używamy**: reklamujemy pustą listę kodeków, żeby zostać przy gołych Bitmap
     Updates (ADR-0006). Włączenie go znaczy zmianę ścieżki graficznej, nie
     dodanie jednego PDU.
   - **Alternate Secondary Frame Marker.** Flaga zdolności istnieje
     (`ALTSEC_FRAME_MARKER_SUPPORT`, `rdp/capability_sets/order/mod.rs:68`), ale
     `ironrdp-server` ogłasza `OrderFlags::empty()` i `OrderSupportExFlags::empty()`
     (`src/capabilities.rs:46-52`), czyli **żadnego wsparcia dla rozkazów
     rysowania**.

   Rozstrzygające: **w całym `ironrdp-server` 0.13.0 nie ma ani jednego
   odwołania do frame markera.** Serwer ich nie wysyła i nie ma jak go o to
   poprosić.

   Wniosek: to **nie jest poprawka w F-004**. Drogi są trzy — przejście na
   Surface Commands / EGFX (**F-104**), PR upstream do IronRDP, albo własna
   implementacja (**F-200**). Do rozważenia przy F-104, gdzie i tak zmienia się
   ścieżka graficzna; wtedy ramkowanie klatek jest efektem ubocznym tej zmiany,
   a nie osobną pracą.

5. **Pamięć na kopię klatki na połączenie.** ~23,8 MB przy Retinie. Nieistotne
   dla jednego klienta, warte odnotowania przy F-105 (multi-monitor).

## 9. Status

**Approved** (2026-08-07). **Implemented** (2026-08-07).

**Weryfikacja 2026-08-25:** zaliczone **AC-1, AC-2, AC-6, AC-7**; AC-5
częściowo (przeciąganie okna i animacja — bez duchów; zostaje Cmd+Tab,
zamknięcie okna i wersja z niskim `--max-fps`). Do zrobienia **AC-3**
(pisanie) i **AC-4** (przewijanie). Pomiary i wnioski w §7, nowy znany efekt
uboczny w §8 pkt 4.

Status **Verified** nadaje się fali, nie specowi (F-007 §8) — F-004 czeka
więc na weryfikację zamykającą Falę 3 razem z F-007 AC-3/AC-6.

Zaimplementowane: F-004.1…F-004.17, F-004.19, F-004.20. Nowy moduł
`src/encoder/damage.rs`, wariant `FrameUpdate::Region`, przełącznik
`--encoder full|damage` (domyślnie `damage`), walidacja szerokości pulpitu
w `main`. CI: **121 testów jednostkowych** + 4 integracyjne pikselowe,
clippy `-D warnings`, `check --locked`.

### Lekcja z pisania testu F-004.20

Pierwsza wersja testu integracyjnego **przechodziła przy celowo zepsutym
umiejscowieniu regionów** — sprawdzone przez podmianę `x`/`y` na zera
w `session::convert`. Powód: sesja zaczyna się pełną klatką, która pasuje
idealnie od razu, więc asercja „obraz zgadza się z wzorcem" była spełniona,
zanim doleciał pierwszy region.

Poprawka: dopasowanie liczy się **dopiero po zastosowaniu aktualizacji
mniejszej niż cały ekran**. Po niej sabotaż jest wykrywany — wszystkie regiony
zwijają się do `(0,0,127,127)` i test pada z tym w komunikacie.

To jest dokładnie mechanizm z ADR-0006 §„Czego ten incydent nauczył o testach",
zastosowany do samego testu: **test, którego nie widziano padającego, nie jest
dowodem.**
