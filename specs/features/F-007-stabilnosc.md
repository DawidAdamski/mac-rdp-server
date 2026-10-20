# F-007 — Stabilność: przeżyć rekonfigurację ekranu, sen/budzenie i reconnect

- **Fala:** 3 (wyciągnięta na początek Fali 3, bo blokuje realne używanie)
- **Status:** Implemented (iteracje 1, 1b, 1c, 1d, 1e, 1f) — AC-1/2/5/7/8
  zaliczone, AC-3 i AC-10 zaliczone automatycznie, AC-9 w części automatycznej,
  AC-6 do weryfikacji zamykającej falę, AC-4 czeka na F-006
- **Data:** 2026-07-24
- **ADR:** ADR-0002 (SCK), ADR-0001 (silnik)
- **Powód powstania:** otwarcie klapy MacBooka w trakcie sesji zabiło strumień
  ScreenCaptureKit i całą sesję (zgłoszone 2026-07-24).

## 1. Cel

Serwer przeżywa zdarzenia, które w realnym użyciu są normą: uśpienie ekranu,
zmiana konfiguracji monitorów (otwarcie/zamknięcie klapy, podłączenie ekranu),
zmiana rozdzielczości oraz kolejne łączenia i rozłączenia klientów — bez
zamrożenia obrazu i bez potrzeby restartu procesu.

## 2. Zdiagnozowana przyczyna (2026-07-24)

Otwarcie klapy w trakcie sesji dołożyło ekran (3024×1964) i **zmieniło
konfigurację wyświetlania**. `SCStream` zbudowany na starcie ze stałego
`SCContentFilter` **nie podąża** za rekonfiguracją: zgłasza
`No capture source provided`, a potem oddaje próbki bez bufora obrazu
(`sample buffer carries no image buffer`). Obraz u klienta zamarza, klient się
rozłącza, a kolejne próby połączenia trafiają w serwer w zdegradowanym stanie.

Potwierdzone osobno: **reconnect przy zdrowym capture działa** (trzy pod rząd,
99,9% pikseli) — więc to nie jest błąd protokołu, tylko brak odporności capture.

## 3. Wymagania

### Odtwarzanie strumienia capture

- **F-007.1** (MUST) `ScreenCaptureKitSource` MUSI wykrywać śmierć strumienia
  przez `SCStreamDelegateTrait::did_stop_with_error` (kanoniczne powiadomienie,
  potwierdzone w źródle 8.0.1). Strumień MUSI być tworzony przez
  `SCStream::new_with_delegate`.

  > **Niewystarczające — patrz §8 (weryfikacja 2026-08-07).** To wymaganie jest
  > poprawne, ale **nie pokrywa trybu awarii opisanego w §2**: przy
  > rekonfiguracji ekranów strumień nie umiera i `did_stop_with_error` się nie
  > wywołuje — strumień dalej oddaje próbki, tyle że bez bufora obrazu.
  > Drugi wyzwalacz: **F-007.14** (watchdog zastoju).
- **F-007.2** (MUST) Po `did_stop_with_error` źródło MUSI spróbować **odtworzyć
  strumień** dla tego samego logicznego ekranu (po `DisplayId`): ponowna
  enumeracja `SCShareableContent`, nowy `SCContentFilter`, nowy `SCStream`,
  publikacja do **tego samego** kanału `watch` (klient nie musi się łączyć od nowa).
- **F-007.3** (MUST) Odtwarzanie MUSI mieć **backoff** (np. 0,5 s → 1 s → 2 s,
  maks. kilka sekund), żeby przy trwałej awarii nie kręcić pętli. Każda próba
  MUSI być logowana.
- **F-007.4** (MUST) Jeżeli przechwytywany ekran **zniknął** (np. był to ekran
  wbudowany, klapa zamknięta), a **jest inny aktywny ekran**, źródło MUSI
  **przełączyć się na niego** (decyzja właściciela, 2026-07-24). Wybór: ten sam
  `DisplayId` jeśli wrócił, inaczej pierwszy dostępny. Przełączenie zmienia
  rozmiar pulpitu → patrz F-007.6 i zależność od F-006.
- **F-007.4a** (MUST) Jeżeli **żaden** ekran nie jest dostępny (rzadkie: wszystkie
  śpią), źródło MUSI serwować placeholder „ekran niedostępny" (nowy wariant,
  wizualnie różny od istniejących) i próbować dalej z backoffem — bez paniki,
  bez spamu.
- **F-007.5** (MUST) Log „dropping frame: no image buffer" MUSI zostać
  ograniczony tak, by trwała awaria nie zalewała logu (mechanizm rate-limit
  już istnieje; MUSI obejmować też ten przypadek).

### Zmiana rozmiaru pulpitu

- **F-007.6** (MUST) Przełączenie na inny ekran (F-007.4) zwykle **zmienia
  rozmiar pulpitu**. Źródło NIE MOŻE wysyłać bitmap o wymiarach niepasujących do
  wynegocjowanego pulpitu (to właśnie myliło klienta: `Client size doesn't fit`).
  Do czasu F-006 minimalne zachowanie: **skalować lub kadrować** klatkę nowego
  ekranu do pierwotnego rozmiaru pulpitu, spójnie (bez skosu), i wpisać
  rozbieżność do logu. To brzydkie, ale poprawne — lepsze niż zamrożony obraz.
- **F-007.6a** (MUST) **Zależność:** pełne, poprawne przełączanie ekranów o
  różnych rozmiarach wymaga dynamicznego resize sesji (**F-006**,
  `RdpServdisplay::request_layout`, deactivation-reactivation). Kolejność prac:
  **F-006 przed F-007** albo F-006 jako część F-007. Bez F-006 przełączenie na
  ekran o innym rozmiarze daje skalowanie/kadrowanie, nie natywny obraz.
- **F-007.7** (SHOULD) Jeśli F-006 jest już zaimplementowane, F-007 POWINNO
  wyzwolić resize zamiast trzymać stary rozmiar. (Zależność: F-006 przed F-007
  w tej części — do rozstrzygnięcia w kolejności prac.)

### Sen / budzenie i cykl życia klienta

- **F-007.8** (MUST) Uśpienie i wybudzenie Maca (lub uśpienie samego ekranu)
  NIE MOŻE trwale zamrozić obrazu — po wybudzeniu strumień MUSI wrócić
  (ta sama ścieżka co F-007.2).
- **F-007.9** (MUST) Kolejne łączenia i rozłączenia klientów MUSZĄ działać
  bez restartu procesu, także gdy poprzedni klient rozłączył się nieczysto
  (reset TCP). To już działa dla zdrowego capture — F-007 MUSI to utrzymać
  po odtworzeniu strumienia.
- **F-007.10** (SHOULD) Nieczysty rozłączenie klienta w trakcie handshake
  (`failed to accept client during finalize: Connection reset by peer`)
  POWINNO być logowane na poziomie `debug`/`warn`, nie `error` — to normalne
  zachowanie klienta, nie awaria serwera. (Jeśli to log IronRDP, a nie nasz,
  udokumentować, że jest oczekiwany.)

### Testy

- **F-007.11** (MUST) Test integracyjny MUSI sprawdzić **wiele reconnectów**
  (≥3) przeciw żywemu serwerowi z wzorcem testowym i asercjonować, że każdy
  dostaje niepuste piksele. (Rozszerzenie `tests/rdp_pixels.rs`.)
- **F-007.12** (MUST) Test jednostkowy MUSI pokryć logikę backoffu odtwarzania
  (czysta, bez SCK): sekwencja opóźnień, reset po sukcesie, limit górny.
- **F-007.13** (SHOULD) Odtwarzanie strumienia po `did_stop_with_error`
  sprawdzić ręcznie: uśpić ekran / otworzyć klapę w trakcie sesji i potwierdzić,
  że obraz wraca bez restartu (checklista w kryteriach akceptacji).

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-007.1, F-007.2 | `screencapturekit` 8.0.1: `SCStreamDelegateTrait::did_stop_with_error`, `SCStream::new_with_delegate`, `update_content_filter` |
| F-007.6 | MS-RDPBCGR §1.3.1.3 (Deactivation-Reactivation Sequence) — pełny resize to F-006 |
| F-007.9 | Fala 0 AC-9 (reconnect już zweryfikowany dla zdrowego capture) |

## 5. Kryteria akceptacji

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, wszystkie testy, w tym
   nowy test wielu reconnectów.
2. **AC-2** **Otwarcie/zamknięcie klapy** w trakcie sesji: obraz na chwilę
   zamiera (placeholder „ekran niedostępny" lub krótka pauza), po czym **wraca
   sam**, bez restartu serwera i bez rozłączenia klienta.
3. **AC-3** **Uśpienie i wybudzenie ekranu** (lub `pmset displaysleepnow`):
   po wybudzeniu obraz wraca.
4. **AC-4** **Zmiana rozdzielczości** przechwytywanego ekranu nie psuje obrazu
   (nie ma skosu ani śmieci); w najgorszym razie obraz jest przeskalowany do
   pierwotnego rozmiaru pulpitu, ale spójny.
5. **AC-5** Trwała awaria (ekran wbudowany, klapa zamknięta na stałe) daje
   placeholder i log z backoffem, bez zalewania logu i bez paniki.
6. **AC-6** Pięć kolejnych cykli połącz–rozłącz (w tym twarde zamknięcie
   klienta) — serwer działa dalej, każde nowe połączenie pokazuje obraz.
7. **AC-7** Log przy trwałej awarii jest czytelny: kilka linii z backoffem,
   nie setki identycznych.
8. **AC-8** Skryptowalna wersja AC-3: `pmset displaysleepnow`, odczekanie,
   wybudzenie — w logu pojawia się `capture recovered`, a licznik `received`
   znów rośnie w ciągu 30 s od wybudzenia. Sonda
   `specs/probes/f007-ac3-displaysleep/` robi dokładnie to i dziś kończy się
   wynikiem negatywnym (§8c).

## 6. Poza zakresem

- **Pełny dynamiczny resize sesji RDP** (klient dostaje nowy rozmiar pulpitu) —
  F-006. F-007 tylko nie psuje obrazu przy zmianie rozmiaru.
- **Multi-monitor** (kilka ekranów naraz w sesji) — F-105.
- **Migracja przechwytywania na inny ekran, gdy wybrany zniknął** — poza F-007;
  tu serwujemy placeholder i czekamy na powrót wybranego ekranu.
- **Reconnect po stronie klienta** (to robi klient RDP sam) — nie nasz zakres.

## 7. Ryzyka i otwarte kwestie

1. **Wątek delegata.** `did_stop_with_error` przychodzi z wątku SCK. Odtwarzanie
   strumienia NIE MOŻE blokować tego wątku — powinno tylko zasygnalizować
   (np. przez kanał / atomik) zadaniu odtwarzającemu, analogicznie do zasady
   „handler nie blokuje" z capture (F-002.11).
2. **Kolejność z F-006.** Pełny resize (F-006) i odtwarzanie (F-007) dotykają
   tego samego miejsca (`request_layout`, rozmiar pulpitu). Do ustalenia, czy
   F-006 idzie przed częścią F-007.6/7 — patrz waves.md.
3. **`did_stop_with_error` a nasz własny `stop_capture`.** Czysty stop
   (nasze zamknięcie) NIE jest raportowany do delegata (potwierdzone w źródle),
   więc odtwarzanie nie wyzwoli się przy normalnym Ctrl-C — dobrze. Ale trzeba
   uważać, by przy `stop()` nie wyścignąć się z trwającym odtwarzaniem.

## 8. Wynik weryfikacji iteracji 1 (2026-08-07)

**Weryfikacja ręczna właściciela: NIEZALICZONA.** AC-2 nie przechodzi.

### Przebieg

Serwer na ekranie zewnętrznym (`Display 3`, 5120×1440, scale 1.0), klapa
MacBooka zamknięta, klient podłączony. **Otwarcie klapy** dołożyło ekran
wbudowany. Obserwacja właściciela: połączenie RDP **nie zostało zerwane**,
obraz u klienta **zamarł i nie wrócił**, na fizycznych ekranach Mac reagował
normalnie. **Zamknięcie klapy** przywróciło działanie.

### Co zadziałało

- **F-007.9 — połączenie przeżyło rekonfigurację.** To jest poprawa wobec
  incydentu z 2026-07-24, gdzie sesja padała. Klient wytrzymał.
- **F-007.5 / AC-7 — rate limiting działa wzorowo.** Trwała awaria dała
  **13 linii** przez ~110 s zamiast tysięcy. Zgadza się co do jednej
  z limiterem `n < 3 || n.is_power_of_two()` (`CaptureStats::should_log_rejection`):
  wpisy dla n = 0, 1, 2, 4, … 2048, czyli **ponad 2048 odrzuconych klatek**.
- **F-007.10 — potwierdzony, niezałatwiony.** W logu jest
  `ERROR ironrdp_server::server: Connection error … Connection reset by peer
  (os error 54)`. To log IronRDP, nie nasz, i nadal na poziomie `ERROR`.

### Przyczyna niezaliczenia AC-2 — błąd w samym F-007.1

**Wyzwalacz odtwarzania jest niewłaściwy.** W logu **nie ma** ani jednej linii
`capture stream stopped unexpectedly; recovering`, ani
`recovery attempt failed; backing off`. Znaczy to, że
`SCStreamDelegateTrait::did_stop_with_error` **nigdy się nie wywołał**, a więc
cała ścieżka odtwarzania (`supervise` → `recover` → `try_rebuild`,
`src/capture/screen.rs:477-617`) nie ruszyła ani razu.

Strumień **nie umarł**. Strumień **zdegradował się po cichu**: dalej wołał
`did_output_sample_buffer`, tyle że z próbkami bez bufora obrazu
(`FrameError::NoImageBuffer`).

To jest błąd w wymaganiu, nie w implementacji. F-007.1 nazywa
`did_stop_with_error` „kanonicznym powiadomieniem" o śmierci strumienia — i to
jest prawda, ale **tryb awarii opisany w §2 tego samego speca nie jest śmiercią
strumienia.** §2 opisał go dokładnie („oddaje próbki bez bufora obrazu"),
a mimo to iteracja 1 podpięła odtwarzanie pod zdarzenie, które w tym trybie
nie występuje. Warunek wyzwolenia nie pokrywa zdiagnozowanego objawu.

### Zmiana speca — iteracja 1b (**Approved** 2026-08-07)

- **F-007.14** (MUST) Odtwarzanie strumienia MUSI mieć **drugi wyzwalacz,
  niezależny od delegata: watchdog zastoju.** Jeżeli przez czas dłuższy niż
  próg licznik `rejected` rośnie, a `received` nie drgnął, źródło MUSI uznać
  strumień za martwy i wejść w tę samą ścieżkę co przy `did_stop_with_error`
  (`recover`, z backoffem F-007.3).
- **F-007.14a** (MUST) Watchdog NIE MOŻE reagować na **statyczny pulpit**.
  Rozróżnienie jest już w licznikach i jest jednoznaczne
  (`src/capture/mod.rs`, `CaptureStats`):

  | Sytuacja | `received` | `skipped` | `rejected` |
  |---|---|---|---|
  | Zdrowy, coś się dzieje | rośnie | — | — |
  | Zdrowy, pulpit statyczny | stoi | **rośnie** | stoi |
  | **Filtr unieważniony (ten incydent)** | stoi | — | **rośnie** |

  Wyzwalaczem jest wyłącznie trzeci wiersz. Pulpit statyczny podbija `skipped`
  (klatki `Idle`/`Blank`), więc watchdog oparty na „braku klatek" byłby błędny
  i przebudowywałby strumień co kilka sekund na bezczynnym Macu.
- **F-007.15** (MUST) Logika watchdoga MUSI być czysta i pokryta testem
  jednostkowym bez SCK: decyzja liczona z dwóch migawek `StatsSnapshot`
  i upływu czasu. Testy: zastój odrzuceń ⇒ wyzwolenie; pulpit statyczny
  (rosnący `skipped`) ⇒ brak wyzwolenia; powrót `received` ⇒ reset licznika.
- **F-007.16** (SHOULD) Log przy wyzwoleniu watchdoga POWINIEN wprost mówić,
  że to zastój, a nie zgłoszona śmierć strumienia — inaczej przy następnej
  diagnozie znów nie będzie wiadomo, którą ścieżką poszło odtwarzanie.

### Otwarte kwestie z tej weryfikacji

1. **Czy `try_rebuild` w ogóle wyleczy ten przypadek?** Ścieżka nigdy nie
   ruszyła, więc **nie wiemy**. Hipoteza (§2 tego speca): rekonfiguracja
   unieważnia `SCContentFilter`, a `try_rebuild` buduje świeży filtr z nowo
   wyliczonej `SCShareableContent`, więc powinno pomóc. **To jest hipoteza do
   potwierdzenia pomiarem, nie fakt.** Gdyby się nie potwierdziła, kolejnym
   krokiem jest `CGDisplayRegisterReconfigurationCallback` — kanoniczne
   powiadomienie o rekonfiguracji ekranów, czyli wyzwalacz trafiający
   w przyczynę, a nie w objaw. Kosztuje dodatkowe FFI, więc nie wchodzi
   „na wszelki wypadek".
2. **Geometria inputu jest zamrożona na starcie.** `selected_geometry`
   (`src/main.rs:41`) liczy `origin_x`/`origin_y`/`scale_factor` raz i przekazuje
   do `CgEventSink`. Po rekonfiguracji układu ekranów origin wybranego ekranu
   może się przesunąć, a wtedy współrzędne bezwzględne trafiają obok.
   **Ten incydent tego nie potwierdza** — obraz był zamrożony, więc nie dało
   się ocenić trafności kliknięć. Tania weryfikacja: przy otwartej klapie
   sprawdzić, czy kliknięcie trafia tam, gdzie wskazuje kursor. Wymaganie
   dopiszemy dopiero po potwierdzeniu.
3. **AC-3 (sen/budzenie) i AC-6 (pięć cykli) nie zostały sprawdzone** — sesja
   zakończyła się na scenariuszu z klapą.

## 8a. Wynik weryfikacji iteracji 1b (2026-08-07, drugie podejście)

**Watchdog działa. AC-2 nierozstrzygnięte. Ujawniona nowa, potwierdzona usterka.**

### Przebieg

Ten sam scenariusz (ekran zewnętrzny 5120×1440, klapa zamknięta, klient
podłączony), otwarcie klapy o ~11:40:27. Obserwacja właściciela — **inna niż
poprzednio**: obraz RDP **nie zamarł**. Zniknęły z niego okna, bo macOS
przeniósł je na dołożony ekran wbudowany. Kursor prowadzony w lewą część
pulpitu RDP **wychodził na ekran laptopa**. Zamknięcie klapy przywróciło stan
pierwotny.

### Co pokazują liczniki

| Okno 10 s | `received` | `rejected` | `delivered` | % pustych |
|---|---|---|---|---|
| zdrowy baseline | +293 | 0 | — | 0% |
| 40:21 → 40:31 | +228 | +62 | +183 | 21% |
| 40:31 → 40:41 | +85 | +208 | +85 | 71% |
| 40:41 → 40:51 | +56 | +219 | +56 | 80% |
| 40:51 → 41:01 | +93 | +181 | +93 | 66% |
| 41:01 → 41:11 | +237 | +54 | +189 | 19% |

**Awaria jest częściowa, nie całkowita.** To istotna korekta diagnozy z §2
i §8. ScreenCaptureKit przez cały czas oddawał ~29 próbek na sekundę — rosnący
był wyłącznie **udział próbek pustych** (21% → 80%), a nie ich brak. Dlatego
tym razem obraz u klienta żył, tylko z mniejszą płynnością, zamiast zamarznąć.

### Co jest potwierdzone

- **F-007.14/.15/.16 działają.** Watchdog wyzwolił się dwa razy:
  `stalled_for_ms=3499` (11:40:43) i `stalled_for_ms=3000` (11:40:55),
  za każdym razem z następującym `capture recovered`. Reguła „`received` stoi,
  `rejected` rośnie" zadziałała mimo częściowej degradacji, bo przerwy bez
  ani jednej dobrej klatki przekraczały 3 s.
- **Brak zalewania logu.** 11 linii `dropping frame` i **2** linie `recovering`
  przez ~45 s awarii. Kontrola z Testu 5 („`recovering` nie może się mnożyć")
  przechodzi — watchdog nie kręci pętli.
- **F-007.9** — klient nie został rozłączony.

### Czego NIE da się z tego logu orzec

**Czy odtworzenie strumienia cokolwiek naprawiło.** Powrót do zdrowia
(11:41:01 → 11:41:11) nastąpił po drugim `capture recovered`, ale **także po
zamknięciu klapy przez właściciela** — log nie pozwala tych dwóch przyczyn
rozdzielić. Do tego awaria była częściowa, więc obraz i tak by się nie zawiesił.
**AC-2 pozostaje niezaliczone przez brak rozstrzygnięcia**, nie przez porażkę.

Rozstrzygnięcie wymaga jednego celowanego testu: otworzyć klapę i **nie
zamykać jej** przez pełne 60 s, obserwując, czy `received` wraca do ~290/10 s
przy wciąż otwartej klapie.

### Nowa usterka, potwierdzona: geometria wejścia jest zamrożona

Otwarta kwestia 2 z §8 **potwierdziła się**. „Kursor prowadzony w lewą część
pulpitu RDP wychodził na ekran laptopa" to dokładnie objaw nieaktualnego
origin: `selected_geometry` (`src/main.rs:41`) wylicza `origin_x`/`origin_y`/
`scale_factor` **raz, przy starcie**, i przekazuje je do `CgEventSink`.
Dołożenie ekranu przesuwa układ współrzędnych macOS, a my dalej przeliczamy
piksele pulpitu przez stary origin — więc współrzędne lądują na sąsiednim
ekranie.

To jest poważniejsze dla użytkownika niż sama płynność obrazu: sesja, w której
kursor ucieka poza pulpit, jest **bezużyteczna**, nawet gdy obraz jest żywy.

- **F-007.17** (MUST) Geometria używana do przeliczania współrzędnych wejścia
  MUSI być odświeżana po rekonfiguracji ekranów, nie tylko przy starcie.
  Kliknięcie MUSI trafiać tam, gdzie wskazuje kursor, również po dołożeniu
  lub odjęciu ekranu w trakcie sesji.
- **F-007.17a** (MUST) Odświeżenie NIE MOŻE łamać granicy z
  `01-architecture.md` §1: `capture` nadal nie zna wejścia, `input` nadal nie
  zna ScreenCaptureKit. Wymianę pośredniczy nasz własny typ (`DisplayInfo` /
  `ScreenGeometry`) przez kanał, którego oba końce wpina `main` — tak samo,
  jak dziś wpina `FrameStream` i `InputDispatcher`.
- **F-007.17b** (MUST) Test jednostkowy MUSI pokryć samo przeliczanie przy
  zmienionym origin (czyste, bez macOS): ten sam piksel pulpitu przed i po
  przesunięciu origin daje różne punkty macOS, zgodnie z nowym układem.

### Drobiazg wart naprawienia przy okazji

- **F-007.18** (SHOULD) Licznik rate-limitu logu odrzuceń
  (`CaptureStats::should_log_rejection`) jest **kumulatywny i nigdy się nie
  zeruje**. Po tej sesji stoi na 512, więc pierwsza linia **następnej** awarii
  pojawi się dopiero przy 1024. odrzuceniu — czyli po ~35 s ciszy w logu.
  Licznik POWINIEN być zerowany po udanym odtworzeniu strumienia, żeby kolejny
  incydent był widoczny od razu.

### Zachowanie macOS, którego nie naprawimy — do dokumentacji użytkownika

Po otwarciu klapy **macOS sam przeniósł okna** na ekran wbudowany, bo stał się
on ekranem głównym. Z punktu widzenia użytkownika RDP wygląda to jak „zniknęły
mi okna", ale to nie jest zachowanie serwera i nie da się go stąd zmienić.
Realne odpowiedzi to multi-monitor (**F-105**) albo przechwytywanie ekranu
głównego zamiast wybranego indeksem. Do opisania wprost w dokumentacji
użytkownika (**F-110**) jako znane zachowanie systemu.

## 8b. Wynik weryfikacji iteracji 1c (2026-08-07, trzecie podejście)

**AC-2 zaliczone. F-007.17 zweryfikowane. Jedna nowa kwestia otwarta.**

### F-007.17 — działa, potwierdzone dwoma niezależnymi dowodami

W logu, dokładnie w momentach zmiany układu ekranów:

```
12:27:46  captured display moved; input coordinates re-based  origin_x=-5120.0 origin_y=-171.0 scale=1.0
12:29:26  captured display moved; input coordinates re-based  origin_x=0.0     origin_y=0.0    scale=1.0
```

Przesunięcie o **-5120 punktów w poziomie i -171 w pionie** — czyli macOS
położył ekran zewnętrzny na lewo od wbudowanego i lekko wyżej. To jest
dokładnie skala błędu, który wcześniej wyprowadzał kursor poza pulpit.

Obserwacja właściciela zgadza się z logiem: *„myszka lepiej działała; jak
przeszedłem w prawy róg ekranu, to nie mogłem wejść na ekran laptopa"*.
Kursor przestał uciekać — pulpit RDP mapuje się teraz wyłącznie na
przechwytywany ekran, także po rekonfiguracji.

### AC-2 — zaliczone

Klapa była otwarta przez **100 sekund** (12:27:46 → 12:29:26). Przyrosty
`received` na 10 s w tym okresie: 244, 265, 230, 113, 231, 209, 203, 289,
230, 202 — przy zdrowym baseline ~290. `delivered` rosło monotonicznie przez
cały czas (129 → 2304).

**Obraz ani razu nie zamarł, klient nie został rozłączony, serwer nie był
restartowany.** To jest treść AC-2. Dla porównania: w incydencie z 2026-07-24
obraz zamarł na stałe i nie wrócił.

Watchdog wyzwolił się w tym czasie **raz** (12:28:16, `stalled_for_ms=3000`)
i odtworzył strumień w 0,6 s.

### Nowa kwestia otwarta: odrzucenia zdarzają się także poza awarią

Tego wcześniej nie widzieliśmy, bo poprzednie logi zaczynały się od zdrowego
okresu z `rejected=0`. Tutaj **odrzucenia rosły od samego początku, na długo
przed dotknięciem klapy**:

| okno | `received` Δ | `rejected` Δ | klapa |
|---|---|---|---|
| 12:27:04 → 14 | +190 | +105 | zamknięta |
| 12:27:14 → 24 | +130 | +165 | zamknięta |
| 12:27:24 → 34 | +73 | +222 | zamknięta |
| 12:27:34 → 44 | +247 | +47 | zamknięta, klient wszedł |

Czyli `sample buffer carries no image buffer` **nie jest wyłącznie sygnałem
awarii**. Zdarza się w normalnej pracy, momentami w dużym natężeniu, i mija
samo. W pierwszym logu (2026-08-07, 11:04) tego nie było — tam osiem
kolejnych okien miało `rejected=0`. **Nie wiemy, co różni te dwa przebiegi.**

Warto zauważyć, czego to **nie** psuje: watchdog wymaga **zera** klatek
dobrych przez 6 kolejnych próbek. Przy ~26 klatkach na sekundę w tle takie
okno nie zdarza się przypadkiem, więc swoistość reguły zostaje wysoka mimo
szumu. Potwierdza to log: przez 100 s z otwartą klapą i tysiącem odrzuceń
watchdog wyzwolił się raz.

Do rozstrzygnięcia przy okazji F-004 (który i tak dotyka tej ścieżki):

1. **Czy `skipped_idle` w ogóle kiedykolwiek rośnie?** W całym logu wynosi 0,
   co znaczy, że `frame_status()` nigdy nie zwrócił `Idle`/`Blank` — albo
   zwraca `None` i sprawdzenie jest pomijane. Jeśli tak, to **puste próbki
   mogą być tym, czym na tej wersji SCK jest „brak nowej treści"** — czyli
   część odrzuceń to normalny sygnał, a nie błąd, i logowanie ich jako `WARN`
   wprowadza w błąd.
2. **Czy odtworzenie o 12:28:16 było potrzebne?** Okno tuż przed nim miało
   +230 klatek, a zaraz po nim +113 z serią ośmiu odrzuceń — czyli sama
   przebudowa kosztowała ~2 s. Przy 3-sekundowym progu możliwe, że leczymy
   coś, co minęłoby samo. Próg jest jedną stałą (`WATCHDOG_STALL_THRESHOLD`).

### Potwierdzone zachowanie macOS, nie nasze

*„Byłem w stanie w czasie klapy odpalić okno, tylko leciało od razu na okno
laptopowe, więc laptop był traktowany jako primary."* — dokładnie tak.
Po otwarciu klapy ekran wbudowany staje się głównym i nowe okna otwierają się
na nim, poza kadrem RDP. Serwer nie ma jak temu zaradzić przy jednym
przechwytywanym ekranie. Odpowiedzi: **F-105** (multi-monitor) albo
przechwytywanie ekranu **głównego** zamiast wybranego indeksem — to drugie
zmienia rozmiar pulpitu w trakcie sesji, więc wymaga **F-006**. Do opisania
w dokumentacji użytkownika (**F-110**).

### Czego nadal nie sprawdzono

**AC-3** (sen/budzenie) i **AC-6** (pięć cykli połącz–rozłącz). Przechodzą do
weryfikacji zamykającej Falę 3 — nie blokują startu F-004, bo F-007 jest
jednym ze speców tej fali, a status **Verified** nadaje się fali jako całości.

## 8c. Wynik weryfikacji AC-3 (2026-10-19) — NIEZALICZONE

**AC-3 wykonane po raz pierwszy i niezaliczone: obraz nie wraca po wybudzeniu
ekranu, a proces zostaje trwale zablokowany.** Zgłoszone przez właściciela
w trakcie testów: „po dłuższym czasie nie korzystania rdp freezuje. Ponowne
podłączenie się nie pomaga. Dalej jest zfreezowane."

AC-3 było **niesprawdzone od 2026-08-07** (§8, otwarta kwestia 3) i przeszło
do weryfikacji zamykającej Falę 3. Odtworzone dwa razy wymuszonym
`pmset displaysleepnow` (sonda `specs/probes/f007-ac3-displaysleep/`)
i potwierdzone niezależnie na trzecim procesie — instancji, którą właściciel
miał uruchomioną do własnych testów.

### Co pokazał pomiar

Ekran uśpiony 10:24:20, wybudzony 10:24:55, `MAC_RDP_LOG=debug`:

| Czas | Zdarzenie |
|---|---|
| 10:24:20.649 | `WARN dropping frame error=sample buffer carries no image buffer` — `rejected` 0 → 1 |
| 10:24:20.701 | `WARN capture stream stopped unexpectedly; recovering reason=SCStream error (No capture source provided): Failed to find any displays or windows to capture display=5` |
| 10:24:25 → 10:25:35 | `received` **stoi na 749** — przez całe uśpienie **i 40 s po wybudzeniu** |
| nigdy | brak `capture recovered` **i brak** `recovery attempt failed; backing off` |
| 10:25:39 | `WARN capture supervisor did not stop within the timeout` |

Dwa wnioski od razu:

1. **Wyzwalacz zadziałał poprawnie.** `did_stop_with_error` odpalił, F-007.1
   zrobiło swoje. Watchdog zastoju (F-007.14) jest tu niewinny — nie był do
   tego potrzebny, bo strumień zgłosił śmierć sam.
2. **Zawiodło samo odzyskiwanie, i zawiodło w ciszy.** `recover` loguje
   `recovery attempt failed; backing off` przy **każdej** nieudanej próbie
   (`src/capture/screen.rs:630`), a tej linii nie ma ani razu. Czyli nigdy nie
   wróciliśmy z `try_rebuild`.

### Przyczyna — nazwana w stosie i potwierdzona w źródle crate'a

`sample` zablokowanego procesu pokazał cały 3-sekundowy odcinek (2563 próbki
z 2563) w jednym miejscu:

```
main → tokio block_on → ScreenCaptureKitSource::<FrameSource>::start
  → capture::screen::build_stream
    → screencapturekit::stream::sc_stream::SCStream::start_capture
      → doom_fish_utils::completion::SyncCompletion::wait
        → _pthread_cond_wait → __psynch_cvwait
```

W źródle `screencapturekit 8.0.1` (`src/stream/sc_stream.rs:659`)
`start_capture` kończy się na `completion.wait()`, a `wait`
(`doom-fish-utils 0.3.3`, `src/completion.rs:99`) to:

```rust
self.inner.cvar.wait_while(state, |s| s.result.is_none()).unwrap()
```

**`Condvar::wait_while` bez timeoutu.** W całym crate jest dokładnie jeden
`fn wait` i nie istnieje wariant z ograniczeniem czasu. `stop_capture`
(`:672`) czeka identycznie.

Czyli pełny łańcuch: ekran zasypia → ScreenCaptureKit zatrzymuje strumień
i zgłasza to delegatem → supervisor **poprawnie** wchodzi w odzyskiwanie →
odzyskiwanie woła `stop_capture`/`start_capture` → te **nigdy nie wracają**,
bo handler zakończenia już nie odpali → zadanie supervisora wisi na zmiennej
warunkowej do końca życia procesu.

### Dlaczego ponowne podłączenie nie pomaga

To nie przypadek ani wina sesji, to konsekwencja architektury: `source.start()`
jest wołane **raz**, w `main`, **przed** `serve` (`src/main.rs:56`), a
`FrameStream` jest jeden na proces. Nowa sesja RDP podłącza się do tego samego,
martwego kanału `watch` i dostaje jego ostatnią wartość — czyli dokładnie tę
zamrożoną klatkę, którą widział poprzedni klient. Żadna liczba reconnectów tego
nie ruszy, i nie powinna: naprawa należy do warstwy capture.

### Trzy skutki uboczne, zmierzone przy okazji

1. **Ścieżka startowa ma tę samą blokadę, na głównym wątku.** Drugi przebieg
   sondy zawiesił się już na starcie: log kończy się na `display available`
   i przez 90 s nie ma nic — ani pierwszej klatki, ani ostrzeżenia
   `FIRST_FRAME_TIMEOUT`. Stos to potwierdza. Serwer, który nie wystaje
   i nic nie mówi, jest gorszy niż serwer, który zawodzi z komunikatem.
2. **Żywy zawieszony proces blokuje start capture dla całej maszyny.**
   Dopóki proces zaparkowany na condvarze żył, **każdy** nowy serwer wisiał na
   starcie. Po `kill -9` tego procesu świeży serwer dostał pierwszą klatkę
   natychmiast (`first frame received width=5120 height=1440 stride=20480`).
   Restart `replayd` ani maszyny nie jest potrzebny. Stąd procedura ratunkowa
   na czas przed poprawką: `pkill -9 mac-rdp-server` **przed** ponownym
   uruchomieniem.
3. **`capture supervisor did not stop within the timeout` to ten sam błąd.**
   Czyste zamknięcie woła `stop_capture`, które blokuje się identycznie. Ten
   warning był widoczny od dawna i był objawem tej przyczyny, nie osobną
   sprawą.

Do tego: wywołanie blokujące siedzi w zadaniu tokio, więc **zajmuje wątek
runtime'u** na zawsze, a na starcie blokuje wątek główny w `block_on`.

### Zmiana speca — iteracja 1d (**Approved** 2026-10-19)

- **F-007.19** (MUST) Żadne wywołanie ScreenCaptureKit czekające na handler
  zakończenia NIE MOŻE blokować bez ograniczenia czasu — co najmniej
  `start_capture` i `stop_capture`. Po przekroczeniu limitu wywołanie MUSI być
  uznane za nieudane i trafić w istniejącą ścieżkę backoffu (F-007.3).
- **F-007.20** (MUST) Wywołania blokujące NIE MOGĄ wykonywać się na wątkach
  runtime'u asynchronicznego ani na wątku głównym. Zmierzony skutek złamania
  tego: zajęty na zawsze wątek runtime'u i zawieszony start serwera bez
  jakiegokolwiek komunikatu.
- **F-007.21** (MUST) Każda nieudana próba odzyskania MUSI zostawić w logu
  wpis z powodem i opóźnieniem. Wpis już istnieje, ale jest **nieosiągalny**,
  bo nie wracamy z próby — cisza w logu była połową czasu diagnozy.
- **F-007.22** (MUST) Jeżeli **pierwszy** start strumienia nie zakończy się
  w limicie, serwer MUSI to powiedzieć i podać placeholder (tak jak przy braku
  uprawnień, F-002.13) albo zakończyć się błędem z niezerowym kodem. Wisieć mu
  nie wolno.
- **F-007.23** (MUST) Zamknięcie serwera NIE MOŻE móc zawisnąć na
  `stop_capture`; warning `capture supervisor did not stop within the timeout`
  MUSI przestać być osiągalny z tego powodu.
- **F-007.24** (MUST) Decyzja „próba przekroczyła limit ⇒ log ⇒ backoff ⇒
  następna próba" MUSI być czysta i pokryta testem jednostkowym bez SCK — tak
  samo jak watchdog z F-007.15.
- **F-007.25** (SHOULD) Po wybudzeniu ekranu obraz POWINIEN wrócić **bez**
  rozłączania klienta. Jeżeli pomiar pokaże, że świeży strumień nie wstanie,
  dopóki poprzedni native start jest w locie, to wymaganie spada do „wraca po
  reconnekcie" i dostaje własny wpis z pomiarem.

### Wynik iteracji 1d (2026-10-19) — AC-3 i AC-8 zaliczone automatycznie

Zaimplementowane i zmierzone tego samego dnia. Dwa niezależne przebiegi sondy
po poprawce, oba PASS:

| Przebieg | `received` przed uśpieniem | 40 s po wybudzeniu |
|---|---|---|
| 1 | 295 | **1234** |
| 2 | 295 | **1235** |

Przed poprawką ten licznik stał na 749 **na zawsze**. Oś z przebiegu 1:
`295 → 367 → 367 → 367 → 643 → 938 → 1234` — płasko przez uśpienie, potem
znowu rośnie, a w logu `capture recovered display=5` **cztery sekundy po
wybudzeniu**.

Cisza z §8c zniknęła. Log odzyskiwania wygląda teraz tak:

```
WARN  capture stream stopped unexpectedly; recovering reason=SCStream error (No capture source provided): …
WARN  recovery attempt failed; backing off error=display 5 is not currently available delay_ms=500 suppressed_since_last=0 timed_out_calls=0
DEBUG recovery attempt failed again; backing off … delay_ms=1000
…
INFO  capture recovered display=5
```

**Niespodzianka w przyczynie, warta zapisania:** `timed_out_calls=0`. Czyli
limit oczekiwania ani razu nie musiał się odpalić — po bumpie `try_rebuild`
wraca **normalnie**, z czystym błędem `display 5 is not currently available`,
w pół sekundy. Śpiący ekran po prostu znika z listy udostępnialnych treści.
Na 8.0.1 ta sama ścieżka nie wracała nigdy. Limit z F-007.19 jest więc
zabezpieczeniem, które w tym scenariuszu okazało się niepotrzebne — a mimo to
zostaje, bo to on zamienia „nigdy" na „najwyżej po limicie" w scenariuszach,
których nie zmierzyliśmy.

Zweryfikowane osobno, każde pomiarem:

| Co | Jak sprawdzone | Wynik |
|---|---|---|
| F-007.19 — limit istnieje i jest widoczny | log startowy | `ScreenCaptureKit completion waits are bounded bound_s=30` |
| F-007.19 — brak limitu jest błędem, nie trybem | `SCREENCAPTUREKIT_COMPLETION_TIMEOUT_SECS=0` | start odrzucony, **kod wyjścia 1**, komunikat wskazuje zmienną i skutek |
| F-007.21 — każda próba zostawia wpis | log wyżej | pierwsza głośno z `timed_out_calls`, powtórki na `debug` |
| F-007.23 — zamknięcie nie wisi | `Ctrl-C` | **kod 0**, `capture stopped`, bez ostrzeżenia o grace period |
| F-007.24 — maszyna decyzji czysta | 6 testów jednostkowych | `cargo test` zielone, bez SCK |
| F-002 AC-5 + ADR-0005 po bumpie | prawdziwy obraz | `width=5120 height=1440 stride=20480`, `avg_frame_bytes=29491200` — jak na 8.0.1 |

**Czego to NIE dowodzi, i trzeba powiedzieć wprost: F-007.25 pozostaje
niezweryfikowane.** Sonda nie potrzebuje klienta RDP i żadnego nie miała
podłączonego, więc „obraz wraca **bez rozłączania klienta**" jest na razie
wnioskiem z konstrukcji (`watch` i `tx` przeżywają przebudowę, F-007.2), a nie
pomiarem. To domyka ręczny test C-1 — i dopóki go nie ma, AC-3 jest zaliczone
w części automatycznej, nie w pełnej.

### Czego ten spec świadomie nie rozstrzyga

**Strumień, który milknie całkowicie**, nie jest wykryty przez nic: watchdog
z F-007.14 wymaga rosnącego `rejected` (`new_rejections == 0 ⇒ return None`,
`src/capture/watchdog.rs:114`), a tabela z F-007.14a nie ma wiersza „wszystko
stoi". W tym incydencie to nie zaszkodziło, bo delegat zgłosił śmierć
strumienia. Dopóki nie ma pomiaru, w którym milczenie faktycznie występuje, to
jest **kwestia otwarta, a nie wymaganie** — zgodnie z tym, jak potraktowaliśmy
`CGDisplayRegisterReconfigurationCallback` w §8.

## 8e. Pomiar w boju (2026-10-19) — ekran zasypia pod żywą sesją

Właściciel testował poprawioną binarkę z `mstsc` i `MAC_RDP_LOG=debug` i zgłosił
najpierw: „miałem parę razy taką sytuację, że się zblokowało, ale jak
odczekałem, to wróciło — tak jakby czasami nie nadążał stream", a potem
„zawiesił się" i „o, wróciło". Log z tej sesji rozstrzyga oba zgłoszenia.

### Hipoteza „stream nie nadąża" — OBALONA pomiarem

Z podłączonym `mstsc`, w stanie ustalonym, przyrosty na każde 10 s:

| Licznik | Przyrost |
|---|---|
| `received` | +295 |
| `delivered` | **+295** |
| `coalesced` | **+0** |

Jeden do jednego, przez minuty. `damage stats`: **15 pełnych klatek na 19538**,
`saved_pct=99`, `bytes_sent=3,9 GB` wobec `bytes_if_full=576 GB`. Potok nie ma
problemu z przepustowością: enkoder ma trzydziestokrotny zapas
(`specs/probes/f004-encoder-throughput/`), przechwytywanie trzyma 29,5 fps,
a odbiorca konsumuje każdą klatkę.

**Wcześniejsza konkluzja z `waves.md` §Pomiar 2026-10-19b, że kandydatem jest
ścieżka wysyłki, była błędna** i jest tam poprawiona. Wysyłka nadąża.

### Co naprawdę zamroziło obraz

| Czas | Zdarzenie |
|---|---|
| 12:47:38 | `capture stream stopped unexpectedly; recovering reason=SCStream error (No capture source provided): Failed to find any displays or windows to capture` |
| 12:47:38 → 12:58:28 | **130 prób** odzyskania, co 5 s, wszystkie `display 5 is not currently available` |
| 12:49:02 | klient zminimalizowany → suppress output |
| 12:58:06 | okno przywrócone → `paused_ms=544112`, **`skipped_frames=0`** |
| 12:58:28 | `capture recovered display=5` |

**Przechwytywanie było martwe 10 minut 50 sekund i wróciło samo.** To jest
dokładnie to, po co powstała iteracja 1d: na 0.1.1 zostałoby martwe do końca
procesu. Pacing logu z F-007.21 też się sprawdził w boju — głośno raz na 30 s
z `suppressed_since_last=5`, resztę na `debug`; 130 prób nie zalało logu.

`skipped_frames=0` przy dziewięciominutowej pauzie jest tu najmocniejszym
dowodem: nie było **czego** pomijać, bo capture nie żył. Gdyby chodziło
o suppress output, licznik pokazałby setki klatek — jak przy pierwszej pauzie
tego dnia (`paused_ms=4709 skipped_frames=139`, okno zminimalizowane na
chwilę; właściciel to potwierdził, i to jest **poprawne** zachowanie
protokołu, nie błąd).

Watchdog zastoju (F-007.14) nie wyzwolił się ani razu — zgłosił to delegat.
Czyli martwy punkt z §8c („strumień, który milknie całkowicie") **nadal nie
został sprawdzony pomiarem** i zostaje kwestią otwartą.

### Dlaczego ekran zasnął pod żywą sesją

`displaysleep` na tej maszynie to **10 minut** na zasilaniu. Klient był
podłączony od 12:37, ekran padł 12:47:38 — po około dziesięciu minutach.
Serwer **nie trzyma żadnej asercji**: `pmset -g assertions` pokazuje
`PreventUserIdleDisplaySleep 0`.

To jest sprzeczność w samym pomyśle: serwer pulpitu zdalnego, któremu ekran
zasypia pod spodem, przestaje działać dokładnie wtedy, gdy ktoś go używa
zdalnie i nie dotyka maszyny lokalnie. Odzyskiwanie nie pomoże, bo **nie da
się przechwycić ekranu, którego nie ma** — i to widać w 130 nieudanych
próbach.

**Kwestia otwarta, nierozstrzygnięta pomiarem:** czy wstrzykiwanie zdarzeń
przez `CGEvent` zeruje licznik bezczynności (`HIDIdleTime`). Jeśli tak, sen
łapie tylko sesje faktycznie bezczynne; jeśli nie, łapie każdą dłuższą niż
`displaysleep`. Dla poniższych wymagań to nie ma znaczenia — asercja działa
w obu przypadkach — ale wpływa na to, jak częsty jest ten objaw.

### Druga luka, odsłonięta przy okazji

Przez te 10 minut klient pokazywał **ostatnią zamrożoną klatkę**, bez żadnej
informacji. `Placeholder` ma dziś dwa warianty — `PermissionDenied`
i `WaitingForFirstFrame` (`src/capture/placeholder.rs:18`) — i żadnego na
„przechwytywanie chwilowo niedostępne". Właściciel nie miał skąd wiedzieć, czy
czekać, czy restartować. To jest ta sama zasada, co w F-007.11: cicha porażka
jest gorsza niż widoczna.

### Zmiana speca — iteracja 1e (**Approved** 2026-10-19)

- **F-007.26** (MUST) Dopóki podłączony jest co najmniej jeden klient, serwer
  MUSI nie pozwalać ekranowi zasnąć z bezczynności, i MUSI zwolnić to
  zabezpieczenie, gdy odłączy się ostatni klient. Bez tego sesja dłuższa niż
  `displaysleep` sama się psuje — zmierzone: 10 min 50 s martwego
  przechwytywania przy podłączonym kliencie.
- **F-007.27** (MUST) Gdy przechwytywanie jest niedostępne, klient MUSI
  zobaczyć placeholder mówiący, co się stało, a nie ostatnią klatkę.
  Dotyczy też odzyskiwania trwającego dłużej niż kilka sekund.
- **F-007.28** (SHOULD) Mechanizm z F-007.26 NIE POWINIEN przeżywać procesu
  ani zostawiać niczego po sobie po nieczystym zakończeniu.
- **F-007.29** (MUST) Stan tego zabezpieczenia (założone, zwolnione) MUSI być
  widoczny w logu — inaczej „czy ekran mógł zasnąć" znów będzie się
  rozstrzygało eksperymentem.

**Kryterium akceptacji dla tej iteracji: AC-9** — klient podłączony dłużej niż
`displaysleep` (tu 10 minut), bez dotykania Maca lokalnie: ekran nie zasypia,
`pmset -g assertions` pokazuje trzymaną asercję, obraz żyje przez cały czas,
a po rozłączeniu klienta asercja znika.

**Decyzja właściciela przy approve (2026-10-19): `caffeinate -d -w <pid>`**
jako proces potomny. Zero `unsafe`, zero nowych zależności, narzędzie z systemu,
a `-w <nasz pid>` wiąże jego życie z naszym, co realizuje F-007.28 bez pisania
ani linijki obsługi sygnałów. Odrzucona alternatywa: `IOPMAssertionCreateWithName`
przez FFI — bez procesu potomnego, ale drugie w repo miejsce z `unsafe` po tym,
które dołożył ADR-0016, i to w kodzie, który musi być poprawny przy każdym
wyjściu z procesu. Sprawdzone 2026-10-19, że `caffeinate -d` faktycznie trzyma
`PreventUserIdleDisplaySleep` (widoczne w `pmset -g assertions`).

## 8f. Wynik iteracji 1e (2026-10-19)

**F-007.26 — zaliczone automatycznie.** `session` publikuje liczbę podłączonych
klientów przez `watch::Sender<usize>` (`SessionResetHandler`), `power` decyduje,
co ta liczba znaczy (`power::keep_display_awake`). To ta sama granica, którą już
przechodzi `watch_geometry`: `session` nie wie nic o macOS, `power` nie wie nic
o RDP. `tests/display_wake_lock.rs` przechodzi całą ścieżkę: 0 klientów → brak
procesu i brak asercji; 1 → `caffeinate -d -w <pid serwera>` żyje
**i** `pmset -g assertions` pokazuje `PreventUserIdleDisplaySleep`; 2 → nadal
jedna blokada (nie dwie); z powrotem 1 → nadal trzymana; 0 → zwolniona.
Dodatkowo dwa testy jednostkowe na sam licznik, w tym jeden na to, że nadmiarowe
rozłączenie nie zawija licznika w dół (`saturating_sub`).

**F-007.27 — zaimplementowane, zweryfikowane w części renderowania.** Nowy
wariant `Placeholder::CaptureUnavailable` (amber, pauza), publikowany raz na
awarię po 3 sekundach ciszy, i kasowany przez pierwszą prawdziwą klatkę po
powrocie. Test sprawdza, że jest wizualnie różny od obu pozostałych i że symbol
jest naprawdę narysowany, a nie że tło ma inny kolor. **Publikacja na żywej
awarii nie jest jeszcze zobaczona** — wymaga zgaszenia ekranu pod podłączonym
klientem, a w momencie weryfikacji właściciel miał żywą sesję mstsc.

**F-007.28 — zaliczone.** `-w <pid serwera>` plus `Drop`, który zabija i zbiera
dziecko. Zmierzone na żywo: po `Ctrl-C` nie zostaje żaden `caffeinate`.

**F-007.29 — zaliczone.** Oba zdarzenia widoczne w logu; przy surowym
połączeniu `nc` (które nie jest poprawnym RDP) pojawiły się
`holding the display awake caffeinate_pid=83569 server_pid=83536`
i `released the display; it may idle to sleep again` w tej samej milisekundzie,
co jest dokładnie poprawnym zachowaniem: klient się podłączył i natychmiast
odpadł.

**AC-9: zaliczone w części automatycznej** (asercja widoczna w `pmset`,
zakładana i zwalniana z liczbą klientów). Część ręczna — sesja dłuższa niż
`displaysleep` bez dotykania Maca — czeka na spokojny moment właściciela, razem
z F-007.25 i z żywym potwierdzeniem F-007.27.

## 8g. Pomiar w boju (2026-10-19) — strumień żyje, ale milczy, i nikt tego nie widzi

Właściciel zgłosił zamarznięcie **mimo podłączenia i reconnecta**, już po
iteracji 1d. To **inna przyczyna** niż §8c i inna niż §8e.

**Co pokazał log** (`/tmp/stream4.log`, proces 39926, start 14:35:44 czasu
lokalnego, czasy w logu w UTC):

```
14:25:24Z  capture stats received=73068 …          ← ostatni wzrost
14:39:44Z  capture stats received=73068 delivered=70806 coalesced=1693
                        skipped_idle=1200 rejected=0 avg_frame_bytes=29491200
```

Czternaście minut bez ruchu na **wszystkich trzech** licznikach. Żadnego
`capture stream stopped unexpectedly`, żadnej próby odzyskiwania, żadnego
`capture stream is alive but delivering no usable frames`. `rejected=0` przez
cały przebieg.

**Czego nie było.** Stos (`sample 39926`) nie pokazuje ani jednego wątku
zaparkowanego w `start_capture`/`stop_capture` — błąd z §8c jest naprawiony
i nie wrócił. `client_loop` → `MacRdpDisplayUpdates::next_update` →
`tokio::time::sleep` normalnie się kręci, więc sesja RDP nie jest zakleszczona.
Ekran nie spał: `screencapture -x` w tym samym momencie zrobił poprawny PNG
2,3 MB. Display 5 był dostępny.

**Przyczyna, nazwana w kodzie.** `StallWatchdog::observe`
(`src/capture/watchdog.rs`) ma wczesne wyjście:

```rust
let new_rejections = current.rejected.saturating_sub(previous.rejected);
if new_rejections == 0 {
    // Neither healthy nor failing: an idle desktop looks exactly like this.
    return None;
}
```

Sygnałem awarii jest „przychodzą odrzuty, a nie przychodzą klatki". W tej
awarii nie przychodzi **nic**: delegat nie jest wołany ani razu, więc nie ma
odrzutów do zliczenia i watchdog po konstrukcji milczy. F-007.14 pokrywa
„strumień żyje i sypie śmieciem", nie pokrywa „strumień żyje i milczy".
Reconnect nie pomaga, bo źródło klatek jest wspólne dla procesu, a nowy klient
dostaje z `watch` ostatnią (zamrożoną) wartość.

**Dlaczego cisza jest odróżnialna od bezczynnego pulpitu — i dlaczego to się
da naprawić bez fałszywych alarmów.** `FrameHandler::did_output_sample_buffer`
rozdziela trzy przypadki: `Complete` → `received`, `Idle`/`Blank`/`Suspended`
→ `skipped_idle`, błąd dekodowania → `rejected`. Bezczynny pulpit **nie jest
cichy** — podbija `skipped_idle` (w tym przebiegu uzbierało się 1200). Awaria
z tego paragrafu zatrzymuje wszystkie trzy. Komentarz w kodzie („an idle
desktop looks exactly like this") jest więc prawdziwy tylko dla `rejected`,
a nie dla sumy wszystkich wywołań delegata.

**Co to wywołało — hipoteza z twardą osią czasu, do potwierdzenia sondą.**
Zamarznięcie spowodował asystent, nie właściciel. Weryfikacja F-007.26
uruchomiła **drugi** proces `mac-rdp-server` na tym samym ekranie 5:

```
14:25:10.336Z  (mój proces) capture started display=Display 5
14:25:16.619Z  (mój proces) capture stopped
14:25:24Z      (proces właściciela) ostatnia klatka
```

Osiem sekund po tym, jak drugi strumień na tym samym ekranie został zamknięty,
pierwszy zamilkł na zawsze. To pasuje do tego, co już raz zmierzyliśmy przy
§8c: ScreenCaptureKit traktuje przechwytywanie ekranu jako zasób
międzyprocesowy. Jeśli to się potwierdzi, awaria jest **odtwarzalna na
żądanie**, czyli pierwsza z tej rodziny, której nie trzeba wyczekiwać.

### Zmiana speca — iteracja 1f (**Approved** 2026-10-20)

- **F-007.30** (MUST) Watchdog MUSI uznać strumień za martwy także wtedy, gdy
  **żaden** licznik nie rośnie — ani `received`, ani `skipped_idle`, ani
  `rejected` — dłużej niż próg, i MUSI wtedy przebudować strumień. Próg MUSI
  być liczony od skonfigurowanego `max_fps`, nie zapisany na stałe, żeby
  `--max-fps 1` nie dawał fałszywych alarmów.
- **F-007.31** (MUST) Linia `capture stats` MUSI zawierać czas od ostatniego
  wywołania delegata (`silent_for_ms`). Dziś „zamarznięte" rozstrzyga się
  dopiero przez porównanie dwóch linii loga z odstępu 10 sekund.
- **F-007.32** (MUST) Odzyskiwanie z ciszy MUSI być w logu odróżnialne od obu
  istniejących wyzwalaczy (`Reported`, `Stalled`) — z tego samego powodu, dla
  którego powstało F-007.16.
- **F-007.33** (SHOULD) Cisza NIE POWINNA wyzwalać odzyskiwania w czasie
  trwającej już przebudowy ani w jej backoffie, żeby jedna awaria nie
  generowała dwóch równoległych odzyskiwań.
- **F-007.34** (SHOULD) Sonda w `specs/probes/` POWINNA odtwarzać tę awarię bez
  klienta RDP: uruchom serwer, odczekaj na klatki, uruchom i zabij drugi proces
  przechwytujący ten sam ekran, sprawdź, czy pierwszy odzyskał.

**Kryterium akceptacji dla tej iteracji: AC-10** — po zamilknięciu strumienia
(wymuszonym sondą z F-007.34) log w ciągu kilku sekund pokazuje odzyskiwanie
z powodu ciszy, `received` znowu rośnie, a obraz u podłączonego klienta wraca
**bez rozłączania**. Testy jednostkowe watchdoga pokrywają przypadek
„wszystkie liczniki stoją" — to jest logika czysta, więc należy do testów
domyślnych, nie do checklisty ręcznej.

**Uwaga do F-007.27 (iteracja 1e).** Placeholder „przechwytywanie
niedostępne" nie ratuje tej awarii: publikuje go ścieżka odzyskiwania, a ta
w ogóle nie startuje. Dopóki F-007.30 nie jest zrobione, klient widzi
zamrożoną klatkę bez żadnego wyjaśnienia — czyli dokładnie to, przed czym
F-007.27 miało chronić.

## 8h. Pomiar w boju (2026-10-20) — pierwszy pełny dzień bez zacięcia

Zgłoszenie właściciela: „działa lepiej, dziś cały dzień ani razu się nie
zacięło". Log `/tmp/stream5.log` (7,7 MB, 4662 linie statystyk) to potwierdza
i pozwala powiedzieć, o ile dokładnie lepiej.

**Przebieg:** 08:07:05Z → 21:04:05Z, czyli **12 godzin 57 minut** jednym
procesem, na buildzie z iteracją 1e.

| Co | Ile |
|---|---|
| Klatki przechwycone (`received`) | 942 593 |
| **Najdłuższy postój `received`** | **0 s** — rósł między każdą parą kolejnych próbek przez 13 godzin |
| Nieoczekiwane zatrzymania strumienia | 0 |
| Wykrycia zastoju, próby odzyskiwania | 0 |
| Odrzucone klatki (`rejected`) | 0 |
| Sesje klienta | 1 przyjęta, 0 rozłączeń — jedna sesja trzymana ~12 h |
| Cykle suppress output (minimalizacje okna) | 34, najdłuższy **3 h 52 min** (`paused_ms=13951208 skipped_frames=137526`) |

**F-007.26 zaliczone w boju.** `holding the display awake caffeinate_pid=76540`
o 08:55:11Z i **ani jednej linii zwolnienia** do końca przebiegu — blokada
trzymana ciągle przez 12 godzin 9 minut, przy jednym podłączonym kliencie.
Wcześniejsza para linii o 08:55:08/08:55:09 to nieudane `accept` (`Connection
reset by peer` w finalize) — czyli poprawne zachowanie: klient się podłączył,
odpadł, blokada poszła za nim i wróciła przy udanej próbie 2 sekundy później.

**F-007.28 zaliczone w boju, i to w gorszym wariancie, niż zakładał spec.**
Przebieg nie skończył się czysto: nie ma `interrupted, shutting down` ani
`released the display`, więc `Drop` się nie wykonał (prawdopodobnie zamknięte
okno terminala, czyli SIGHUP). Mimo tego po śmierci procesu w systemie **nie
został żaden `caffeinate`** — `-w <pid serwera>` zrobił dokładnie to, po co
został wybrany przy approve.

**Czego ten dzień NIE dowodzi — uczciwie.** Przez całą dobę
`PreventUserIdleDisplaySleep` trzymał też inny proces właściciela
(`Vorssaint`, asercja licząca 24 h 20 min w momencie pomiaru). Nasza blokada
jest w logu udokumentowana i nieprzerwana, ale dzisiejszy brak uśpienia ekranu
**nie izoluje jej jako przyczyny** — ekran i bez nas miał powód nie zasnąć.
Pełna izolacja wymaga dnia bez tamtej asercji; do tego czasu dowodem na
F-007.26 pozostaje `tests/display_wake_lock.rs` plus obecność naszej asercji
w `pmset`.

**Co nadal jest niezweryfikowane, bo nie było na czym.** Przez 13 godzin nie
zdarzyła się **ani jedna** awaria przechwytywania. To dobra wiadomość i zła
naraz: F-007.25 („obraz wraca bez rozłączania klienta") i żywa publikacja
placeholdera z F-007.27 wymagają awarii, a tej nie było. Oba zostają
niezweryfikowane i czekają na sondę z F-007.34, która wywoła awarię na
żądanie, zamiast jej wyczekiwać.

**Co to mówi o iteracji 1f (§8g).** Cisza strumienia nie wróciła ani razu, co
zgadza się z diagnozą: wyzwalaczem był drugi proces przechwytujący ten sam
ekran, a nie samo upływanie czasu. Luka w watchdogu jest więc rzadka, ale
nadal otwarta — i nadal kończy się zamrożonym obrazem, którego nic nie
naprawia poza restartem.

## 8i. Wynik iteracji 1f (2026-10-20) — AC-10 zaliczone w części automatycznej

**F-007.34 najpierw**, bo bez sondy reszta nie dałaby się zmierzyć.
`specs/probes/f007-silent-stream/run.sh` odtwarza awarię z §8g przez to, co ją
wywołało w boju: drugi proces przechwytujący ten sam ekran, który się kończy.
**Dwa przebiegi, dwa razy odtworzona** — więc „odtwarzalna na żądanie" nie
jest twierdzeniem z jednej próby:

| Przebieg | Cisza wykryta po | Przebudowa | `received` |
|---|---|---|---|
| 1 | 5000 ms | 0,62 s | 294 → 2769 |
| 2 | 5499 ms | 0,59 s | 298 → 2769 |

**F-007.30 i F-007.32 zaliczone.** Log z przebiegu 1:

```
21:16:48.063  WARN capture stream stopped calling us entirely (no frames, no idle,
              no rejects); recovering display=5 silent_for_ms=5000 threshold_ms=5000
21:16:48.685  INFO capture recovered display=5
```

Komunikat nazywa dokładnie ten wyzwalacz i podaje próg, więc następna
diagnoza nie pójdzie ścieżką „stale content filter?", którą ta awaria
fałszywie sugerowała. Cała przerwa to **około sześciu sekund** — przedtem było
to zamrożenie do końca życia procesu.

**F-007.31 zaliczone.** `silent_for_ms` jest w linii statystyk:
`received=2769 … silent_for_ms=34`. „Zamarznięte" czyta się teraz z jednej
linii; w §8g trzeba było porównywać dwie z odstępu 10 sekund.

**F-007.33 zaliczone testami jednostkowymi.** Trzydzieści sekund pełnej ciszy
przy odpytywaniu co 500 ms daje **sześć** wyzwoleń (raz na okno), nie
sześćdziesiąt, a `reset()` po przebudowie zeruje zegar ciszy, więc nowy
strumień nie jest sądzony za ciszę starego.

**Jak próg jest liczony.** `silence_threshold(max_fps)` = większe z: 5 sekund
albo 30 interwałów klatkowych. Przy 30 fps wychodzi 5 s, przy `--max-fps 1`
— 30 s. Zdrowy strumień woła delegata z częstotliwością `max_fps` niezależnie
od tego, czy cokolwiek się zmienia na ekranie, więc cisza dłuższa niż
trzydzieści klatek nie ma zdrowego wytłumaczenia. Dowód z boju: przez
12 godzin 57 minut zdrowej pracy (§8h) nie było ani jednej przerwy widocznej
w rozdzielczości 10 sekund.

**Czego sonda nie dowodzi.** Nie ma klienta RDP, więc druga połowa AC-10 —
„obraz wraca u podłączonego klienta bez rozłączania" — nadal jest
niezweryfikowana, razem z F-007.25 i żywą publikacją placeholdera z F-007.27.
Ale teraz wszystkie trzy da się zamknąć jednym testem ręcznym na dwie minuty,
bez czekania na przypadek: **C-3** w `specs/testy-reczne.md`.

## 9. Status

**Approved** (2026-07-27) — z zakresem pierwszej iteracji poniżej.

**Iteracja 1: zaimplementowana, weryfikacja NIEZALICZONA** (2026-08-07).
Zaliczyła AC-1, AC-7 i F-007.9; **nie zaliczyła AC-2** z powodu opisanego
w §8 — domknięte dopiero iteracjami 1b i 1c.

**Stan kryteriów po iteracji 1c:** AC-1, AC-2, AC-5, AC-7 zaliczone.
**AC-3 i AC-6 niesprawdzone** — przechodzą do weryfikacji zamykającej Falę 3.
AC-4 (skalowanie przy zmianie rozdzielczości) domyka się dopiero z F-006.

**Iteracja 1b: Approved, zaimplementowana, zweryfikowana częściowo**
(2026-08-07). F-007.14, F-007.14a, F-007.15, F-007.16 — watchdog zastoju jako
drugi wyzwalacz odtwarzania. Nowy moduł `src/capture/watchdog.rs` (czysty, bez
SCK) + drugie ramię `tokio::select!` w `supervise`. **Wyzwalacz potwierdzony
w boju** (dwa wyzwolenia, bez zalewania logu); **AC-2 nierozstrzygnięte** —
patrz §8a.

**Iteracja 1c: Approved, zaimplementowana, ZWERYFIKOWANA** (2026-08-07).
F-007.17, .17a, .17b (odświeżanie geometrii wejścia) i F-007.18 (zerowanie
rate-limitu logu). **AC-2 zaliczone**, F-007.17 potwierdzone logiem
i zachowaniem — patrz §8b.

Rozwiązanie: nowy `capture::geometry::probe` czyta pozycję i skalę ekranu
prosto z Quartza (`CGDisplayBounds` + tryb wyświetlania) — na tyle tanio, że
da się to odpytywać co 2 s, w przeciwieństwie do `SCShareableContent`.
Zadanie w `main` przelicza `DisplayInfo` na `ScreenGeometry` i wysyła je
**tym samym kanałem, co zdarzenia myszy i klawiatury**, jako nowy wariant
`InputEvent::Geometry`. Dzięki temu mapowanie zmienia się dokładnie *pomiędzy*
dwoma zdarzeniami, nigdy w środku serii ruchów, i nie powstaje żaden nowy
kanał ani blokada.

Dwie rzeczy, które łatwo tu przeoczyć, i dlatego mają własne testy:

- **Zapamiętana pozycja kursora też musi się przenieść.** Klik i scroll są
  wysyłane w miejscu ostatnio zapamiętanym, nie w miejscu ostatniego ruchu —
  bez przeliczenia pierwszy klik po dołożeniu ekranu wylądowałby na starym
  ekranie, mimo że ruchy byłyby już poprawne (`CgEventSink::rebase_cursor`).
- **Aktualizacja geometrii może zostać porzucona**, bo kanał wejścia jest
  bounded i pod zalewem ruchów myszy gubi zdarzenia. Wysłana raz, przepadłaby
  na zawsze. Dlatego watcher publikuje **przy każdym takcie**, nie tylko przy
  zmianie — koszt to jedno zdarzenie na 2 s, a błąd sam się koryguje.

CI zielone: **102** testy jednostkowe, clippy `-D warnings`, `check --locked`.
Sprawdzone na żywym Macu, że `probe` zwraca poprawne wartości
(`display=3 origin=(0,0) scale=1.0`, zgodnie z tym, co raportuje SCK).

CI zielone: **91** testów jednostkowych (81 + 10 na watchdogu), test 3
reconnectów (`repeated_reconnects_each_see_the_picture`), clippy `-D warnings`,
`cargo check --locked`.

### Czego szukać w logu przy ponownej weryfikacji AC-2

Po otwarciu klapy, w ciągu kilku sekund, powinna pojawić się linia:

```
WARN … capture stream is alive but delivering no usable frames (stale content filter?); recovering  display=3 stalled_for_ms=3000
```

a po niej `capture recovered`. Trzy możliwe wyniki i co każdy znaczy:

| Co widać | Wniosek |
|---|---|
| `…recovering` + `capture recovered` + **obraz wraca** | AC-2 zaliczone, hipoteza „stale filter" potwierdzona |
| `…recovering` + `capture recovered`, ale **obraz nadal zamrożony** | Wyzwalacz działa, `try_rebuild` nie leczy. Następny krok: `CGDisplayRegisterReconfigurationCallback` (§8, otwarta kwestia 1) |
| **brak** linii `…recovering` | Watchdog nie wyzwolił — do sprawdzenia progi `WATCHDOG_*` w `src/capture/screen.rs` |

### Zakres iteracji 1 (2026-07-27, decyzja właściciela)

Implementujemy **rdzeń odporności strumienia**, bo dokładnie leczy zgłoszony
incydent z klapą (reconfiguracja unieważnia `SCContentFilter`, strumień oddaje
próbki bez obrazu):

- **W tej iteracji:** F-007.1, F-007.2, F-007.3, F-007.5, F-007.8, F-007.9,
  F-007.10, F-007.11, F-007.12. Odtworzenie `SCStream` dla **tego samego**
  ekranu, backoff, rate-limit logów, sleep/wake, testy.
- **Odłożone do F-006 (osobna iteracja):** F-007.4, F-007.4a, F-007.6,
  F-007.6a, F-007.7 — przełączanie na inny ekran i skalowanie/kadrowanie do
  rozmiaru pulpitu wciągają dynamiczny resize sesji (F-006), którego nie ma
  jeszcze jako speca. Do tego czasu: jeśli łapany ekran **zniknie na trwałe**,
  serwujemy placeholder i czekamy na jego powrót (spójne z §6 „Poza zakresem").

Kryteria akceptacji odnoszące się do przełączania ekranów (AC-4 w części
skalowania) domykają się dopiero z F-006; iteracja 1 domyka AC-1, AC-2, AC-3,
AC-5, AC-6, AC-7.

**Iteracja 1d: Approved, zaimplementowana, ZWERYFIKOWANA automatycznie**
(2026-10-19) — AC-3 wykonane po raz pierwszy, **niezaliczone**, naprawione
i ponownie zmierzone tego samego dnia; patrz §8c. Przyczyna nazwana w stosie i w źródle crate'a:
`start_capture`/`stop_capture` w `screencapturekit 8.0.1` czekają na
`Condvar::wait_while` **bez timeoutu**, więc odzyskiwanie po uśpieniu ekranu
wisi na zawsze. Nowe wymagania F-007.19 … F-007.25 i nowe AC-8 mają
**approve właściciela 2026-10-19**, razem z **ADR-0016** (bump
`screencapturekit` 8.0.1 → 11.0.0). Właściciel wskazał tę iterację jako
**pierwszą rzecz do zrobienia**, przed F-112 i F-111.

**Stan kryteriów po iteracji 1d:** AC-1, AC-2, AC-5, AC-7 zaliczone.
**AC-3 zaliczone w części automatycznej** (dwa przebiegi sondy, 295 → 1234
i 295 → 1235) — pełne domknięcie wymaga ręcznego C-1 z podłączonym klientem,
bo F-007.25 nie jest zmierzone. **AC-8 zaliczone.** AC-6 niesprawdzone.
AC-4 domyka się dopiero z F-006.

**Iteracja 1e: Implemented** (2026-10-19) — F-007.26 … F-007.29, z pomiaru w boju
(§8e): **ekran Maca zasypia pod żywą sesją RDP** i przechwytywanie umiera na
10 minut 50 sekund, bo nie da się przechwycić ekranu, którego nie ma. Serwer
nie trzyma żadnej asercji. Przy okazji odsłonięta druga luka: w czasie awarii
klient widzi ostatnią zamrożoną klatkę, bo `Placeholder` nie ma wariantu
„przechwytywanie niedostępne”. **Approve właściciela 2026-10-19**, razem
z decyzją o mechanizmie: `caffeinate -d -w <pid>` jako proces potomny.
Zrobione tego samego dnia — wynik w §8f. **AC-9 zaliczone automatycznie**,
część ręczna (sesja dłuższa niż `displaysleep`) czeka na spokojny moment,
tak samo jak F-007.25 i żywe potwierdzenie F-007.27.

**Pomiar w boju 2026-10-20 (§8h):** pierwszy pełny dzień bez zacięcia —
12 h 57 min jednym procesem, 942 593 klatki, **najdłuższy postój `received`
0 s**, zero zatrzymań strumienia. **F-007.26 i F-007.28 zaliczone w boju**
(blokada trzymana nieprzerwanie 12 h 9 min; po nieczystej śmierci procesu nie
został żaden `caffeinate`). **AC-9 zaliczone z zastrzeżeniem**: tę dobę
`PreventUserIdleDisplaySleep` trzymał też inny proces właściciela, więc brak
uśpienia nie izoluje naszej asercji jako przyczyny. F-007.25 i żywy placeholder
z F-007.27 **nadal niezweryfikowane** — przez 13 godzin nie było ani jednej
awarii, na której dałoby się je zobaczyć.

**Iteracja 1f: Implemented** (2026-10-20, approve i wykonanie tego samego dnia
— właściciel wskazał ją jako następną rzecz do zrobienia, przed F-112).
**AC-10 zaliczone w części automatycznej**, dwa przebiegi sondy, przerwa
skrócona z „na zawsze" do ~6 sekund; wynik w §8i. Część kliencka AC-10,
F-007.25 i żywy placeholder z F-007.27 czekają na test ręczny **C-3**, który
od teraz zajmuje dwie minuty i nie wymaga czekania na awarię. — F-007.30 … F-007.34 i nowe AC-10,
z pomiaru w boju (§8g): strumień ScreenCaptureKit **zamilkł bez błędu**
i watchdog tego nie widzi, bo jego sygnałem awarii są odrzuty, a w tej awarii
nie przychodzi nic. 14 minut zamrożonego obrazu przy podłączonym kliencie,
reconnect nie pomaga.

**Co §8e potwierdziło o iteracji 1d:** odzyskiwanie zadziałało w boju —
130 prób, powrót sam z siebie, log nie zalany (głośno raz na 30 s). Przy tym
F-007.25 **wciąż** nie jest zweryfikowane: klient był podłączony, ale przez
całą awarię miał zminimalizowane okno i suppress output, więc „obraz wraca bez
rozłączania klienta” nadal nie zostało zobaczone na żywym obrazie.
