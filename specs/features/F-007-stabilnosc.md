# F-007 — Stabilność: przeżyć rekonfigurację ekranu, sen/budzenie i reconnect

- **Fala:** 3 (wyciągnięta na początek Fali 3, bo blokuje realne używanie)
- **Status:** Draft
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

## 8. Status

**Approved** (2026-07-27) — z zakresem pierwszej iteracji poniżej.
**Iteracja 1 zaimplementowana** (2026-07-27, commit „F-007.1/.2/.3…") —
czeka na ręczną weryfikację właściciela (AC-2, AC-3, AC-5, AC-6, AC-7).
CI zielone: 81 testów jednostkowych (w tym backoff), test 3 reconnectów
(`repeated_reconnects_each_see_the_picture`), clippy `-D warnings`.

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
