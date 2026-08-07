# F-004 — Damage tracking: wysyłamy tylko to, co się zmieniło

- **Fala:** 3 (Jakość MVP)
- **Status:** Draft
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

*(do wypełnienia przy implementacji — F-004.18, AC-6)*

| Scenariusz | Bajty/s przed | Bajty/s po | Klatki wysłane przed | Po |
|---|---|---|---|---|
| Statyczny pulpit | | | | |
| Pisanie w edytorze | | | | |
| Przewijanie strony | | | | |

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
4. **Pamięć na kopię klatki na połączenie.** ~23,8 MB przy Retinie. Nieistotne
   dla jednego klienta, warte odnotowania przy F-105 (multi-monitor).

## 9. Status

**Draft** — czeka na approve właściciela. Approve F-004 = zgoda na decyzję
z **ADR-0009** (diff jako źródło prawdy, `dirty_rects()` dopiero jako
optymalizacja), która **odwraca** zapowiedź z `waves.md` Fala 1.
