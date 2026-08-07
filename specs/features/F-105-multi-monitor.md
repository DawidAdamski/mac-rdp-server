# F-105 — Wiele ekranów Maca na jednym ekranie klienta: tryby prezentacji

- **Fala:** 6 (Komfort) — przeniesione z backlogu
- **Status:** Draft
- **Data:** 2026-08-07
- **ADR:** ADR-0002 (SCK), ADR-0005 (stride), ADR-0009 (damage)
- **Zależności:** **F-006** (dynamiczny resize) dla trybów B i C;
  **F-004** (damage) dla sensownej wydajności trybu A
- **Powód powstania:** obserwacja właściciela z weryfikacji F-007 (2026-08-07):
  po otwarciu klapy macOS przenosi okna na ekran wbudowany, który staje się
  głównym, a sesja RDP pokazuje wtedy pusty ekran zewnętrzny. Serwer nie ma
  dziś żadnego sposobu, żeby użytkownik zobaczył drugi ekran.

## 1. Cel

Użytkownik wybiera, **jak** wiele ekranów Maca ma być pokazane na jednym
ekranie klienta RDP, zamiast być na stałe przypisany do jednego ekranu
wybranego indeksem przy starcie.

## 2. Stan faktyczny — dlaczego „jeden ekran klienta" (zbadane 2026-08-07)

Właściciel założył, że RDP nie umie wielu ekranów po stronie klienta.
**Protokół umie** — `mstsc /multimon`, MS-RDPBCGR 2.2.1.3.6 (Client Monitor
Data), MS-RDPEDISP. Ograniczenie leży w silniku:

| Fakt | Miejsce | Konsekwencja |
|---|---|---|
| Akceptor **zawsze** wysyła klientowi layout z **jednym** monitorem, na sztywno `left:0, top:0, right:desktop.width, bottom:desktop.height, PRIMARY` | `ironrdp-acceptor-0.10.0/src/connection.rs:786-796` | Nie da się przez IronRDP wystawić N monitorów jako N monitorów |
| Klient **może** zgłosić nam swój layout przez MS-RDPEDISP | `RdpServerDisplay::request_layout(DisplayControlMonitorLayout)`, `ironrdp-server-0.13.0/src/display.rs:304` | Wiemy, ile ekranów ma klient, choć nie umiemy ich obsłużyć osobno |
| Klient podaje swój rozmiar pulpitu przy negocjacji | `RdpServerDisplay::request_initial_size(client_size) -> DesktopSize`, `display.rs:295` | **Mozaikę można dopasować do ekranu klienta**, zamiast wypychać mu ogromny pulpit |
| `MonitorLayoutEntry` waliduje szerokość `200..=8192` i wymaga **parzystej** | `ironrdp-displaycontrol-0.8.0/src/pdu/mod.rs:367-374` | Górny limit rozmiaru mozaiki |
| Enkoder bitmap wymaga szerokości podzielnej przez 4 i `width ≤ 16383` przy 4 bpp | `ironrdp-server-0.13.0/src/encoder/bitmap.rs:30,39` | Patrz F-004.6 |

**Wniosek:** „jeden ekran klienta" jest ograniczeniem **IronRDP**, nie RDP.
Zdjęcie go to F-200 (własne MS-RDPBCGR) albo PR upstream — nie F-105.
Ten spec robi najlepszą rzecz możliwą **w granicach silnika**.

## 3. Trzy tryby (propozycja właściciela, 2026-08-07)

Konfiguracja: `display_mode = "single" | "mosaic" | "switch"`.

### Tryb A — `single` (dzisiejsze zachowanie, domyślne)

Jeden ekran wybrany indeksem albo `DisplayId`. Rozmiar pulpitu = rozmiar tego
ekranu w pikselach. To jest stan obecny i **zostaje domyślny**: jest najprostszy
i dla jednego monitora niczego nie brakuje.

### Tryb B — `mosaic`

Wszystkie ekrany (albo wybrany podzbiór) złożone na jednym pulpicie RDP,
rozmieszczone zgodnie z ich układem w przestrzeni macOS, a puste miejsca
wypełnione czarnym tłem.

Kluczowa decyzja projektowa: **mozaika jest skalowana tak, żeby zmieścić się
w ekranie klienta**, a nie wypychana w natywnej rozdzielczości. Dwa ekrany
5120×1440 i 3024×1964 obok siebie to pulpit 8144×1964 — na kliencie 1920×1080
tekst byłby nieczytelny, a przy okazji ociera się o limit 8192 z MS-RDPEDISP.
Rozmiar klienta znamy z `request_initial_size`, więc dopasowanie jest możliwe
od pierwszej klatki.

### Tryb C — `switch`

Jeden ekran naraz, jak w trybie A, ale z **przełączaniem skrótem klawiszowym**
bez rozłączania sesji.

## 4. Wymagania

### Wspólne

- **F-105.1** (MUST) Tryb prezentacji MUSI być konfigurowalny
  (`display_mode` w TOML + flaga CLI), z domyślnym `single` — zmiana zachowania
  domyślnego byłaby regresją dla użytkownika z jednym monitorem.
- **F-105.2** (MUST) Wybór ekranów MUSI dać się określić jawnie (lista indeksów
  lub `DisplayId`), a nie tylko „wszystkie".
- **F-105.3** (MUST) Odwzorowanie współrzędnych wejścia MUSI uogólnić się
  z jednej geometrii na **układ**: piksel pulpitu RDP → (ekran, punkt macOS).
  To zastępuje dzisiejszy pojedynczy `ScreenGeometry` w `CgEventSink`
  (F-007.17). Odwzorowanie MUSI być **czystą funkcją** i pokryte testami
  jednostkowymi bez macOS.
- **F-105.4** (MUST) Kliknięcie w czarne tło (obszar niepokryty żadnym ekranem)
  NIE MOŻE być wstrzyknięte nigdzie — MUSI być po cichu porzucone. Wstrzyknięcie
  go w najbliższy ekran dałoby kliknięcia w miejsca, których użytkownik nie
  widzi.
- **F-105.5** (MUST) Dodanie lub odjęcie ekranu w trakcie sesji MUSI być
  obsłużone spójnie z wybranym trybem, bez zamrożenia obrazu (ścieżka F-007).

### Tryb `mosaic`

- **F-105.6** (MUST) Źródło MUSI przechwytywać N ekranów równolegle. `SCContentFilter`
  filtruje **jeden** ekran (`with_display`), więc oznacza to N osobnych
  `SCStream` — każdy z własnym nadzorem i odtwarzaniem (F-007.2).
- **F-105.7** (MUST) Składanie MUSI iść przez `capture::canvas`, rozszerzony
  o prymityw **blit** (kopiowanie klatki źródłowej w prostokąt docelowy,
  świadome stride'u po obu stronach — ADR-0005). Dziś canvas ma `fill` i `rect`;
  blit jest jedynym brakującym prymitywem.
- **F-105.8** (MUST) Rozmiar pulpitu MUSI być dopasowany do rozmiaru klienta
  z `request_initial_size`, z zachowaniem proporcji każdego ekranu. Obszary
  niepokryte MUSZĄ być czarne, nie śmieci i nie ostatnia treść.
- **F-105.9** (MUST) Skalowanie MUSI być spójne z odwzorowaniem wejścia
  (F-105.3): ten sam współczynnik w obie strony, inaczej kursor przestanie
  trafiać. Test jednostkowy MUSI to sprawdzić jako **round-trip**:
  piksel pulpitu → punkt macOS → z powrotem daje ten sam piksel.
- **F-105.10** (SHOULD) Ekran, który zniknął, POWINIEN zostawić w mozaice czarny
  prostokąt i wrócić na swoje miejsce po powrocie, zamiast przebudowywać cały
  układ — przesuwanie się reszty ekranów pod kursorem jest gorsze niż dziura.

### Tryb `switch`

- **F-105.11** (MUST) Skrót przełączający MUSI być rozpoznawany **przed**
  wstrzyknięciem i NIE MOŻE być przekazany do macOS. Wykrywanie MUSI być
  czystą funkcją nad `KeyEvent`, testowalną bez macOS.
- **F-105.12** (MUST) Skrót MUSI być konfigurowalny, a domyślny MUSI być
  nieoczywisty do przypadkowego trafienia i wolny od kolizji ze skrótami macOS
  (propozycja: `Ctrl+Alt+Shift+Strzałka`). Uzasadnienie wyboru — w ADR.
- **F-105.13** (MUST) Przełączenie na ekran o **innym rozmiarze** wymaga
  **F-006**. Bez F-006 tryb `switch` MUSI być odrzucony przy starcie
  z czytelnym komunikatem, zamiast dawać zniekształcony obraz.
- **F-105.14** (SHOULD) Po przełączeniu klient POWINIEN dostać krótką informację,
  na którym ekranie jest — inaczej przy dwóch podobnych pulpitach nie wiadomo.

### Testy

- **F-105.15** (MUST) Testy jednostkowe odwzorowania układu: piksel w każdym
  z N ekranów trafia we właściwy ekran i punkt; piksel w czarnym tle daje
  „nigdzie"; round-trip z F-105.9; ekrany o **różnych `scale_factor`**
  (Retina obok nie-Retiny) — to jest przypadek, w którym pojedynczy
  współczynnik skali z F-007.17 przestaje wystarczać.
- **F-105.16** (MUST) Test składania mozaiki na syntetycznych klatkach: każdy
  ekran ląduje w swoim prostokącie, tło jest czarne, `stride > width*4` po obu
  stronach nie psuje obrazu.

## 5. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| §2 (limit jednego monitora) | `ironrdp-acceptor-0.10.0/src/connection.rs:786-796` |
| F-105.8 | `RdpServerDisplay::request_initial_size`, `ironrdp-server-0.13.0/src/display.rs:295` |
| F-105.13 | MS-RDPBCGR §1.3.1.3 (Deactivation-Reactivation), MS-RDPEDISP |
| F-105.6 | `screencapturekit` 8.0.1: `SCContentFilter::with_display` (jeden ekran na filtr) |
| — | MS-RDPBCGR 2.2.1.3.6 (Client Monitor Data) — czego nie robimy i dlaczego |

## 6. Kryteria akceptacji

1. **AC-1** CI zielone, w tym testy odwzorowania układu i składania mozaiki.
2. **AC-2** `display_mode = "single"` zachowuje się **dokładnie** jak dziś —
   brak regresji dla jednego monitora.
3. **AC-3** `mosaic` z dwoma ekranami: widzę oba naraz, każdy w proporcjach,
   tło czarne, klik trafia we właściwy ekran i we właściwe miejsce.
4. **AC-4** `mosaic` z ekranami o różnym `scale_factor` (Retina + zewnętrzny):
   kliknięcie trafia na obu z tą samą dokładnością.
5. **AC-5** Kliknięcie w czarne tło nic nie robi — nie trafia w żaden ekran.
6. **AC-6** `switch`: skrót przełącza ekran w trakcie sesji, bez rozłączenia;
   skrót **nie** dociera do macOS (nie widać go w aplikacji na Macu).
7. **AC-7** Otwarcie klapy w trybie `mosaic`: nowy ekran pojawia się w mozaice
   (albo, jeśli nie był wybrany, nie pojawia się) — bez zamrożenia obrazu.

## 7. Poza zakresem

- **Prawdziwy multi-monitor RDP** — N monitorów jako N monitorów u klienta.
  Zablokowane przez IronRDP (§2). To jest **F-200**, nie F-105.
- **Przenoszenie okien między ekranami Maca z poziomu klienta** — to robi
  użytkownik w macOS, nie serwer.
- **Zmiana, który ekran macOS jest głównym** — poza zakresem; „nowe okna
  otwierają się na ekranie głównym" to zachowanie systemu (F-007 §8b).
- **Audio i clipboard per-ekran** — bez sensu, to zasoby całej sesji.

## 8. Ryzyka i otwarte kwestie

1. **Koszt trybu `mosaic` jest wielokrotnością trybu `single`.** N strumieni,
   N kopii do składania, jedna klatka wynikowa o powierzchni sumy. Bez **F-004**
   (damage tracking) mozaika dwóch ekranów to ~50 MB na klatkę. F-004 jest tu
   nie optymalizacją, tylko warunkiem sensowności.
2. **Czytelność mozaiki.** Dwa ekrany złożone i wpisane w 1920×1080 to każdy
   ekran w ~40% liniowo. Do czytania tekstu to się nie nada — tryb `mosaic` jest
   do **orientacji i przenoszenia okien**, nie do pracy. To trzeba powiedzieć
   użytkownikowi wprost w dokumentacji, żeby nie odebrał tego jako wady.
3. **Skalowanie a jakość.** Pomniejszanie bez filtrowania da schodki na tekście.
   Filtrowanie kosztuje CPU. Do rozstrzygnięcia pomiarem — i to jest naturalny
   moment na rozważenie ścieżki GPU (SCK oddaje `IOSurface`, F-107).
4. **N strumieni a odporność.** Każdy strumień może zdegradować się osobno
   (F-007). Nadzór i watchdog muszą działać **per strumień**, a awaria jednego
   ekranu nie może zabić mozaiki — powinna dać czarny prostokąt (F-105.10).
5. **Kolizja skrótu w trybie `switch`.** Każdy skrót, który przechwycimy,
   przestaje działać w macOS na czas sesji. Dlatego konfigurowalny i domyślnie
   egzotyczny.

## 9. Kolejność prac (propozycja)

Tryby nie są równe kosztem ani zależnościami:

| Tryb | Koszt | Blokady | Wartość |
|---|---|---|---|
| `single` | zrobione | — | — |
| `switch` | mały–średni | **F-006** (rozmiary ekranów się różnią) + F-007.4 (przełączanie ekranu, już zapisane) | Duża: „widzę drugi ekran" za niewielką pracę |
| `mosaic` | duży | **F-004** (bez damage'u nieopłacalny), N strumieni, kompozytor, układ wejścia | Duża, ale to osobna iteracja |

Rekomendacja: **`switch` przed `mosaic`**. Prawie cała jego maszyneria już
istnieje albo jest zapisana (F-007.4 przełączanie ekranu, F-006 resize),
a dokłada tylko czysty detektor skrótu. `mosaic` to nowy podsystem
(kompozytor + N strumieni) i ma sens dopiero po F-004.

## 10. Status

**Draft** — czeka na approve właściciela. Approve oznacza zgodę na:
(a) trzy tryby z domyślnym `single`, (b) uogólnienie odwzorowania wejścia
z pojedynczej geometrii na układ (zmiana wobec F-007.17), (c) kolejność
`switch` przed `mosaic`.
