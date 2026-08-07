# ADR-0009: Źródło damage'u — własny diff wobec ostatniej **wysłanej** klatki, nie `dirty_rects()` z ostatniej odebranej

- **Status:** Proposed
- **Data:** 2026-08-07
- **Dotyczy:** F-004 (Fala 3), pośrednio F-005 i F-007
- **Powód powstania:** projekt damage trackingu; `waves.md` zapowiadał
  „F-004 bierze damage od systemu", a to okazuje się niewystarczające

## Kontekst

F-004 ma przestać wysyłać całą klatkę i wysyłać wyłącznie zmienione regiony.
Trzeba rozstrzygnąć **skąd bierzemy informację o tym, co się zmieniło**.

`waves.md` (Fala 1, na podstawie ADR-0002) zapowiadał odpowiedź:
„`screencapturekit` **wystawia** `dirty_rects()`. F-004 bierze damage od
systemu, a własny diff klatek zostaje najwyżej fallbackiem". Ta zapowiedź jest
**nie do utrzymania w obecnej architekturze** i ten ADR wyjaśnia dlaczego.

## Zbadany stan faktyczny (2026-08-07)

### 1. `dirty_rects()` istnieje i jest dostępne

`screencapturekit` 8.0.1 wystawia w `CMSampleBufferSCExt`:

```rust
fn dirty_rects(&self) -> Option<Vec<crate::cg::CGRect>>;   // src/cm/sample_buffer.rs:104
```

Pod spodem FFI `cm_sample_buffer_get_dirty_rects` (`src/cm/ffi.rs:62`).
Zwraca `None`, gdy atrybutu nie ma **albo gdy lista jest pusta** — te dwa
przypadki są nierozróżnialne w API bindingu (`src/cm/sample_buffer.rs:228-250`,
gałąź `if rects_ptr.is_null() || count == 0 { return None }`).

### 2. …ale kanał `watch` koalescuje klatki, a damage jest per-klatka

To jest sedno problemu. `01-architecture.md` §4b, świadoma i słuszna decyzja:
klatki idą przez `tokio::sync::watch`, który **z definicji trzyma tylko
najnowszą wartość**. Wolny klient dostaje mniej klatek, ale zawsze aktualną.

`dirty_rects()` opisuje różnicę **między klatką N-1 a N**. Konsument, który
odebrał klatkę N-3, a potem N (bo N-2 i N-1 przepadły w koalescencji),
dostanie prostokąty opisujące wyłącznie przejście N-1 → N. Regiony zmienione
w N-2 i N-1, a nietknięte w N, **nigdy nie zostaną wysłane** — u klienta
zostanie stara treść.

Objaw: duchy. Fragmenty poprzedniego stanu ekranu, które nie znikają, dopóki
coś ich ponownie nie zabrudzi. Tym gorsze, im wolniejszy klient — czyli
najgorsze dokładnie w scenariuszu LAN, pod który celujemy w Fali 4.

**Nie da się tego naprawić po stronie konsumenta bez dodatkowego stanu**, bo
konsument z definicji nie widział klatek, które przepadły.

### 3. `FrameUpdate::Region` jeszcze nie istnieje

`01-architecture.md` §2 opisuje trzy warianty (`Full`, `Region`, `Resize`).
Kod ma jeden (`src/encoder/mod.rs:19`), z komentarzem mówiącym wprost, że
pozostałe czekają na approve F-004 i F-006. Doc jest planem, nie stanem.

### 4. Enkoder bitmap IronRDP nakłada twarde ograniczenia na region

Przeczytane w `ironrdp-server-0.13.0/src/encoder/bitmap.rs`:

| Ograniczenie | Miejsce | Skutek naruszenia |
|---|---|---|
| **`width` musi być wielokrotnością 4** | `:30-35` | `BitmapEncodeError::Encode("Width must be a multiple of 4")` — komentarz `FIXME` mówi, że to znane ograniczenie |
| `data` jest cięte przez `chunks(stride * chunk_height)`, wysokość liczona jako `chunk.len() / stride` | `:44`, `:50` | długość niebędąca wielokrotnością `stride` ⇒ obcięty lub zgubiony wiersz |
| Każdy wiersz czytany jako `&row[..width * bpp]` | `:56-58` | ostatni, krótszy wiersz ⇒ **panika** na indeksowaniu wycinka |
| `chunk_height = u16::MAX / (width * bpp)` | `:39` | `width > 16383` przy 4 bpp ⇒ `chunk_height == 0` ⇒ `chunks(0)` panikuje |

Dwie konsekwencje warte zapamiętania:

- **Dzisiejsza ścieżka pełnoklatkowa już milcząco wymaga, żeby szerokość
  pulpitu była wielokrotnością 4.** Działa, bo realne rozdzielczości takie są
  (3024, 5120, 1280). Nie jest to nigdzie zapisane — teraz jest.
- **Wycinek „zero-copy" regionu jest kuszący, ale nie zawsze bezpieczny.**
  `Bytes::slice` jest liczone referencjami, więc region można by wysłać bez
  kopiowania jako `data[y*stride + x*bpp ..]` z zachowanym `stride` — IronRDP
  i tak czyta z każdego wiersza tylko pierwsze `width*bpp` bajtów. Ale wycinek
  musi mieć **dokładnie `h * stride` bajtów**, a to wymaga
  `y*stride + x*bpp + h*stride <= data.len()`. Dla regionu dotykającego
  **dolnej krawędzi** przy `x > 0` warunek nie zachodzi: zabrakłoby `x*bpp`
  bajtów. Klatki z CoreVideo nie mają zapasu — `Frame::required_len()` to
  dokładnie `height * stride`.

## Decyzja

### 1. Źródłem prawdy jest **diff wobec ostatniej klatki wysłanej temu klientowi**

Enkoder damage'u trzyma kopię tego, co **faktycznie poszło na drut**, i porównuje
z nią każdą nową klatkę. Nie porównuje z klatką poprzednio *odebraną* i nie ufa
w tej roli systemowi.

Uzasadnienie: to jedyny wariant **odporny na koalescencję z definicji**, a nie
przez staranność. Jeżeli klatka przepadła, jej zmiany i tak są zawarte
w różnicy wobec ostatniej wysłanej. Stan jest per-połączenie, więc dwóch
klientów o różnej przepustowości nie psuje sobie nawzajem obrazu —
`MacRdpDisplay::updates()` już buduje osobny enkoder na połączenie
(`src/session/display.rs:38-46`), więc architektura na to czeka.

### 2. Porównanie jest **kafelkowe**, nie pikselowe

Klatka jest dzielona na siatkę kafelków o stałym rozmiarze (start: 64×64
piksele, wartość do zmierzenia). Kafelek uznajemy za brudny, gdy różni się
choćby jednym bajtem. Brudne kafelki są scalane w prostokąty, prostokąty
wyrównywane do wymagań enkodera (§4 wyżej) i wysyłane.

Porównanie kafelka to `memcmp` wiersz po wierszu — **arytmetyka wyłącznie przez
`y * stride + x * bpp`** (ADR-0005), nigdy `y * width * bpp`.

### 3. `dirty_rects()` **nie wchodzi do iteracji 1**

Wchodzi jako **podpowiedź zawężająca obszar porównania** dopiero w iteracji 2,
po pomiarze, i wyłącznie jako optymalizacja — nigdy jako źródło prawdy.
Jeżeli podpowiedź jest niedostępna (`None`) albo niekompletna, obraz nadal
jest poprawny, bo prawdę daje diff.

Żeby podpowiedź była poprawna mimo koalescencji, będzie musiała być **sumą
prostokątów po numerach `sequence`** (pole `sequence` w `Frame` już istnieje,
`src/capture/mod.rs`), z awaryjnym „luka większa niż bufor ⇒ porównaj całość".
To jest osobna, mierzalna zmiana i nie ma powodu wiązać jej z poprawnością.

### 4. Region jest wysyłany jako **zwarta kopia**

`FrameUpdate::Region` niesie bufor o `stride == width * bpp`, skopiowany
z klatki źródłowej. Nie stosujemy wycinka zero-copy w iteracji 1.

Uzasadnienie: kopia regionu jest z definicji mała (o to chodzi w damage
trackingu), a wariant zero-copy wymaga warunku brzegowego z §4 wyżej, którego
naruszenie daje **panikę w cudzym kodzie**, nie nasz błąd walidacji. Ścieżka
pełnoklatkowa (`FrameUpdate::Full`) zostaje bez zmian, zero-copy.

Zero-copy dla regionów zostaje udokumentowaną optymalizacją na później,
z warunkiem bezpieczeństwa zapisanym wprost:
`y*stride + x*bpp + h*stride <= data.len()`.

### 5. Pełna klatka jako jawny wariant awaryjny

Enkoder wysyła całość, gdy: to pierwsza klatka połączenia, gdy zmieniła się
geometria klatki, gdy brudnych kafelków jest więcej niż próg (wysłanie całości
jest wtedy tańsze niż wiele małych aktualizacji), albo gdy liczba regionów
przekroczyłaby limit na klatkę. Każdy z tych progów jest konfigurowalny
i pokryty testem jednostkowym.

## Konsekwencje

- **Koszt pamięci:** jedna klatka na połączenie. Dla 3024×1964 BGRA to ~23,8 MB.
  Akceptowalne — serwujemy konsolę jednego użytkownika, nie farmę sesji.
- **Koszt CPU:** przy braku zmian na ekranie to `memcmp` całej klatki na każdą
  klatkę wejściową (~714 MB/s przy 30 fps i Retinie). `memcmp` chodzi
  w granicach kilkunastu GB/s, więc to ułamek rdzenia, ale **to jest liczba do
  zmierzenia w F-004, nie do założenia**. Jeśli okaże się istotna, iteracja 2
  z `dirty_rects()` jako podpowiedzią zbija ją do zera dla statycznego pulpitu.
- **Cała logika cięcia jest czysta** — bez SCK, bez RDP, bez GUI. Testowalna
  jednostkowo na syntetycznych klatkach, zgodnie z `01-architecture.md` §2.
  To był główny powód, dla którego `encoder` jest osobnym modułem.
- **Zapowiedź z `waves.md` zostaje odwrócona** i to jest świadome: nie „damage
  od systemu z diffem jako fallbackiem", tylko „diff jako prawda z damage'em
  systemowym jako przyspieszeniem". `waves.md` został poprawiony.
- **Zapisujemy niewypowiedziane dotąd założenie:** szerokość pulpitu musi być
  wielokrotnością 4. F-004 dokłada jawną walidację z czytelnym komunikatem,
  zamiast pozwalać enkoderowi IronRDP zwrócić „Width must be a multiple of 4"
  bez kontekstu.

## Odrzucone alternatywy

### A. `dirty_rects()` z ostatniej odebranej klatki, wprost

Odrzucone: **niepoprawne** przy koalescencji (§2 wyżej). Duchy na ekranie,
tym częstsze, im wolniejsze łącze. Klasa błędu identyczna z ADR-0006 — cicha
regresja obrazu, której nie widać w testach jednostkowych, tylko u użytkownika.

### B. Suma `dirty_rects()` po numerach `sequence`, bez własnego diffu

Odrzucone jako **jedyne** źródło prawdy, przyjęte jako przyszła optymalizacja
(§3). Powody odrzucenia w roli źródła prawdy:

1. Poprawność zależy w całości od tego, czy SCK zawsze raportuje komplet
   prostokątów. Nie mamy jak tego udowodnić ani przetestować bez GUI.
2. Binding nie odróżnia „brak zmian" od „brak atrybutu"
   (`Option<Vec<CGRect>>`, `None` w obu przypadkach) — pomyłka w interpretacji
   daje zamrożony obraz.
3. Po odtworzeniu strumienia (F-007.2, nowy `SCStream`) numeracja i ciągłość
   damage'u są nieoczywiste. Diff nie ma tego problemu w ogóle.

### C. Diff wobec ostatniej **odebranej** klatki, stan w źródle

Odrzucone: ta sama wada co A. Źródło nie wie, którą klatkę widział który
konsument, więc różnica liczona w źródle opisuje przejście, którego dany
klient nie oglądał.

### D. Przejście na EGFX/RemoteFX zamiast damage trackingu

Odrzucone na tym etapie: to F-104, wymaga własnego speca, a ADR-0006 zostawił
**nierozstrzygniętą przyczynę** czarnego ekranu na ścieżce kodekowej. Wchodzenie
tam przed jej wyjaśnieniem oznaczałoby budowanie na znanym, niezdiagnozowanym
błędzie.

## Do rozstrzygnięcia w trakcie F-004 (nie blokuje decyzji)

1. **Rozmiar kafelka.** Start 64×64. Mniejszy = dokładniejszy damage, więcej
   regionów i większy narzut nagłówków. Do zmierzenia na realnym obciążeniu
   (przewijanie tekstu vs statyczny pulpit vs odtwarzanie wideo).
2. **Próg „za dużo kafelków ⇒ wyślij całość".** Start: 60% powierzchni.
3. **Strategia scalania.** Najprostsza sensowna: scalanie poziome w wierszu
   kafelków, potem pionowe scalanie identycznych zakresów. Alternatywa
   (prostokąty pokrywające o minimalnej powierzchni) jest droższa i prawie na
   pewno nieopłacalna — do potwierdzenia pomiarem, nie intuicją.
