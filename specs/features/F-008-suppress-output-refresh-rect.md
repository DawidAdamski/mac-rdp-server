# F-008 — Suppress Output i Refresh Rect: przerwa w wysyłaniu i pełne odświeżenie

- **Fala:** 3 (Jakość MVP) — poprawka zgodności z MS-RDPBCGR, przed F-005
- **Status:** Implemented (2026-09-18) (Approved 2026-09-17, właściciel)
- **Data:** 2026-09-17
- **ADR:** ADR-0013 (rzeczywista ścieżka obrazu, Proposed), ADR-0009 (damage),
  ADR-0011 (fork IronRDP, Proposed — docelowe domknięcie znanej luki, §5)
- **Źródło:** `specs/02-ms-rdpbcgr.md` §4 P-06, §3.3 G-2/G-3, §4.1 (Cmd+Tab)
- **Zależności:** F-004 (Implemented). **Cel pośredni:** zamknięcie F-004 AC-5
  (F-004 §8 pkt 6)

## 1. Cel

Serwer przestaje wysyłać obraz, gdy klient o to prosi (okno zminimalizowane
albo zasłonięte), a po wznowieniu wysyła **pełny, aktualny** obraz. Dzięki temu
klient nigdy nie zostaje z fragmentem, o którym serwer „myśli”, że klient go ma.

## 2. Wymagania

### 2.0 Stan faktyczny (2026-09-17), z którego wynikają wymagania

**Specyfikacja.**
- **G-3.** Po Suppress Output serwer **MUST** przestać albo wznowić wysyłanie
  aktualizacji, zależnie od `allowDisplayUpdates` (MS-RDPBCGR 3.3.5.11.2,
  str. 330; PDU: 2.2.11.3, str. 226–227).
- **G-2.** Po Refresh Rect serwer **MUST** wysłać grafikę dla wskazanego
  obszaru (3.3.5.11.1, str. 330; PDU: 2.2.11.2, str. 224–225).
- Obsługę obu PDU serwer ogłasza w General Capability Set (`refreshRectSupport`,
  `suppressOutputSupport` — 2.2.7.1.1, str. 137).
- Appendix A <38> (str. 437–438): serwery RDP 8.0/8.1 źle obsługiwały Refresh
  Rect, a obejściem jest **para Suppress Output (wyłącz → włącz), wymuszająca
  odświeżenie całego pulpitu**. Wznowienie po Suppress Output jest więc
  w praktyce traktowane jako prośba o pełne przemalowanie.

**IronRDP 0.13.0.**
- Ogłasza obsługę obu PDU (`ironrdp-server-0.13.0/src/capabilities.rs:24-30`).
- Suppress Output tylko ustawia `display_suppressed: Arc<AtomicBool>`
  (`server.rs:1589-1593`). Samo wysyłanie **nie** jest wstrzymywane — to zadanie
  backendu, który ma czytać flagę przez `RdpServer::display_suppressed_handle()`
  (`server.rs:618`) albo `RdpServerBuilder::with_display_suppressed_handle`
  (`builder.rs:230`).
- Refresh Rect **tylko zeruje tę flagę**, jeśli była ustawiona (`swap(false)`,
  `server.rs:1602-1606`). Niczego nie odmalowuje, a prostokątów nie przekazuje
  dalej. **Refresh Rect przy nieustawionej fladze jest dla nas niewidoczny.**
- Na początku każdego połączenia flaga jest zerowana (`server.rs:800-809`).
- IronRDP trzyma własną kopię obrazu (`UpdateEncoder::framebuffer`) i każdą
  bitmapę porównuje z nią kafelkami (`encoder/mod.rs:240-271`). Pełnoekranowa
  bitmapa **najpierw** jest porównywana, a dopiero **potem** zastępuje kopię
  (`:318-330`). Wysłanie tej samej pełnej klatki drugi raz nie wysyła więc nic.
  **Publicznego API do unieważnienia tej kopii nie ma.**
- **Jedyny reset tej kopii:** `DisplayUpdate::Resize` wywołuje Deactivate All
  i ponowną aktywację (`server.rs:1047-1051`, `:1739-1750`).
  - Aktywacja tworzy nowy `UpdateEncoder` z `framebuffer: None`
    (`server.rs:1482-1484`, `encoder/mod.rs:163`).
  - Pętla klienta ponownie woła `RdpServerDisplay::updates()`
    (`server.rs:1236`).
- `desktopResizeFlag` klienta jest tylko logowany i nie blokuje Deactivate All
  (`server.rs:1408-1410`) — ograniczenie opisane w §5.

**Nasz kod.**
- `grep display_suppressed src/` jest pusty: wysyłamy dalej, gdy klient prosi
  o przerwę. Narusza to G-3.
- `DamageEncoder` porównuje z ostatnią **wysłaną** klatką (ADR-0009 §1), więc
  po wznowieniu też nie wyśle niczego, co się nie zmieniło.
- `FrameStream::clone` zaczyna od nowa i oddaje **bieżącą** klatkę od razu
  (`src/capture/mod.rs:275-287`).
- `MacRdpDisplay::updates()` tworzy nowy enkoder na połączenie
  (`src/session/display.rs:38-46`).

**Wniosek.** Ponowna aktywacja przez `Resize` z **niezmienionym** rozmiarem
resetuje oba porównania naraz: IronRDP dostaje nowy `UpdateEncoder`,
a my — nowe `updates()`, czyli świeży enkoder i klon strumienia z bieżącą
klatką. Pierwsza klatka po aktywacji idzie więc w całości. Wynika to z kodu
[Z]. Zachowanie prawdziwych klientów przy ponownej aktywacji tego samego
rozmiaru trzeba zmierzyć (AC-5, AC-7).

### 2.1 Diagnostyka (pomiar D-2)

- **F-008.1** (MUST) Procedura pomiaru D-2 (`02-ms-rdpbcgr.md` §5) MUSI być
  zapisana w §4 z dokładnym filtrem logów.
  - Filtr: `MAC_RDP_LOG=info,mac_rdp_server=debug,ironrdp_server=debug`.
    `EnvFilter` w `src/main.rs:237-252` już to obsługuje, więc wymaganie nie
    zmienia kodu.
  - Szukane komunikaty IronRDP: `client suppress-output state changed`,
    `client RefreshRectangle cleared suppress-output state`,
    `Dropping bitmap update that exceeds desktop size`,
    `Failed to extract bitmap subregion`.
  - D-2 MUSI zostać wykonany i opisany **przed** implementacją F-008.3+ — jego
    wynik może zmienić priorytety (§5).
- **F-008.2** (MUST) Serwer MUSI logować na poziomie `info` każde **przejście**
  stanu przerwy obserwowane po naszej stronie (`suppressed` ↔ `resumed`).
  - Przy wznowieniu log zawiera czas trwania przerwy i liczbę klatek
    pominiętych w jej trakcie.
  - Dzięki temu D-2 i AC-3…AC-6 da się ocenić z naszego logu, bez logów
    IronRDP.

### 2.2 Przerwa w wysyłaniu (G-3)

- **F-008.3** (MUST) `session` MUSI odczytywać stan przerwy z uchwytu IronRDP
  (`display_suppressed_handle()` albo `with_display_suppressed_handle`).
  Typ `Arc<AtomicBool>` z IronRDP NIE MOŻE wyjść poza `session`, a logika
  decyzji jest czysta i ma nasz własny typ (F-008.8).
- **F-008.4** (MUST) Gdy klient poprosił o przerwę, `next_update` NIE MOŻE
  zwracać aktualizacji obrazu (`DisplayUpdate::Bitmap`).
  - Klatki z capture są w tym czasie odbierane i **porzucane bez kodowania** —
    koalescencja `watch` i tak trzyma tylko najnowszą (`01-architecture.md` §4b).
  - Baza porównania naszego enkodera się nie zmienia.
  - Capture działa dalej (§5).
- **F-008.5** (MUST) Zmiana stanu przerwy MUSI zostać zauważona w ciągu
  **≤ 100 ms**, także przy statycznym pulpicie, gdy capture nie dostarcza
  nowych klatek. `next_update` nie może więc czekać **wyłącznie** na następną
  klatkę. Mechanizm wybiera implementacja (np. `tokio::select!` z interwałem).
  Warunki:
  - zachowana cancel-safety z kontraktu `RdpServerDisplayUpdates`
    (komentarz w `src/session/display.rs:60-64`);
  - brak zajętego czekania (busy-wait).

### 2.3 Pełne odświeżenie po wznowieniu (G-2, G-3, <38>)

- **F-008.6** (MUST) Po przejściu `suppressed` → `resumed` serwer MUSI wysłać
  klientowi **cały pulpit** z najnowszej klatki, także jeśli od ostatniego
  wysłania nic się nie zmieniło.
  - Obejmuje to Refresh Rect przychodzący w trakcie przerwy: IronRDP zeruje
    wtedy flagę, więc widzimy wznowienie.
  - Pełny pulpit zawiera każdy prostokąt, o który mógł prosić klient, więc G-2
    jest w tym przypadku spełnione.
- **F-008.7** (MUST) ~~**wersja 1 — zastąpiona 2026-09-24, patrz F-008.7v2
  i §6.**~~ **Rozstrzygnięcie resetu porównań.** Na IronRDP 0.13.0
  pełne odświeżenie MUSI być wykonane przez `DisplayUpdate::Resize` z
  **bieżącym, niezmienionym** rozmiarem pulpitu (§2.0, „Wniosek”). Skutki:
  - IronRDP tworzy nowy `UpdateEncoder` bez kopii obrazu;
  - IronRDP ponownie woła `updates()`, a my zwracamy nowy enkoder i klon
    strumienia, który od razu oddaje bieżącą klatkę.

  Nie wysyłamy „tej samej klatki jeszcze raz” — zjadłyby ją oba diffy.
  Nie dokładamy obejść typu celowo zmieniona klatka. Jeśli AC-5 albo AC-7
  pokaże, że ponowna aktywacja jest u któregoś z klientów widocznie szkodliwa
  (miganie, rozłączenie, reset skalowania okna), implementacja MUSI się
  zatrzymać i zaproponować zmianę tego wymagania: poprawkę w forku IronRDP
  (ADR-0011) albo nowy ADR.
- **F-008.7v2** (MUST) **Approved (2026-09-24, właściciel).** Pełne
  odświeżenie po wznowieniu MUSI być wykonane **bez** Deactivate All:
  - `Encoder` dostaje metodę unieważniającą bazę porównania (np.
    `invalidate_baseline()`), czystą i testowalną bez GUI: po jej wywołaniu
    następna klatka jest zakodowana **w całości**, niezależnie od tego, co
    enkoder wysłał wcześniej;
  - po przejściu `suppressed` → `resumed` `session` woła tę metodę i koduje
    najnowszą klatkę normalną ścieżką; `DisplayUpdate::Resize` NIE JEST
    wysyłany;
  - diff kafelkowy IronRDP (`encoder/mod.rs:240-271`) odetnie kafelki, których
    zawartość klient już ma. Jego kopia obrazu to z definicji to, co klientowi
    wysłano, więc wynik jest poprawny **dopóki klient nie porzuca swojego
    obrazu w trakcie przerwy**.

  Powód zmiany: ponowna aktywacja rozłącza klienta — `ironrdp-acceptor`
  0.10.0 `connection.rs:833-848` kończy połączenie błędem, gdy w okno
  reaktywacji trafi jakikolwiek Share Data PDU, a klienci wysyłają Refresh
  Rect i Suppress Output parami (2/2 wznowień 2026-09-24, §6).

  **Jawne odstępstwo:** jeśli klient porzuci obraz w trakcie przerwy, jego
  niezmienione fragmenty zostaną nieodświeżone — wbrew duchowi Appendix A
  <38>. Domknięcie: poprawka tolerancji PDU w forku IronRDP (ADR-0011,
  Etap 1), po której można wrócić do wariantu z ponowną aktywacją albo dodać
  jawne unieważnienie kopii obrazu w IronRDP.
- **F-008.8** (MUST) Decyzja „kiedy odświeżyć” MUSI być czystą maszyną stanów
  (np. `SuppressTracker`) w warstwie bez typów IronRDP, testowalną jednostkowo.
  - Wejście: kolejne odczyty flagi i znaczniki czasu.
  - Wyjście: `Idle` / `Suppressed` / `RefreshDue`.
- **F-008.9** (MUST) Odświeżenia MUSZĄ się **koalescować**.
  - Jeśli klient wznowi i ponownie wstrzyma, zanim odświeżenie zostało
    wysłane, odświeżenie jest anulowane.
  - Najwyżej jedno odświeżenie jest w toku naraz.
  - Kolejne może nastąpić najwcześniej **1 s** po poprzednim; wznowienie
    w tym oknie odkłada odświeżenie na jego koniec, zamiast je gubić.
  - Szybkie minimalizuj/przywróć nie może więc wywołać lawiny ponownych
    aktywacji.
- **F-008.10** (MUST) Przerwa i odświeżenie MUSZĄ działać tak samo dla
  `--encoder damage` i `--encoder full` — mechanizm leży w `session`,
  a nie w enkoderze.

### 2.4 Testy

- **F-008.11** (MUST) Testy jednostkowe `SuppressTracker` pokrywają:
  - wstrzymanie → wznowienie ⇒ jedno `RefreshDue`;
  - wstrzymanie → wznowienie → wstrzymanie przed obsłużeniem ⇒ brak
    odświeżenia (F-008.9);
  - dwa wznowienia w odstępie < 1 s ⇒ drugie odroczone, nie zgubione
    (F-008.9);
  - brak przerwy ⇒ nigdy `RefreshDue`;
  - stan początkowy połączenia = brak przerwy.

  Każdy test ma w nazwie albo komentarzu ID wymagania.
- **F-008.12** (SHOULD) Test integracyjny w `tests/` (wzorem
  `tests/rdp_pixels.rs`, klient `ironrdp-connector`/`ironrdp-session`
  z `dev-dependencies`) POWINIEN sprawdzić:
  - po Suppress Output (`allowDisplayUpdates = 0`) i zmianie wzorca testowego
    serwer przez ≥ 500 ms nie wysyła aktualizacji obrazu;
  - po Suppress Output (`allowDisplayUpdates = 1`) klient przechodzi ponowną
    aktywację i dostaje pełny, poprawny obraz (asercja na pikselach —
    ADR-0006 §„Czego ten incydent nauczył”).

  Jeśli `ironrdp-session` 0.11.0 nie pozwala wysłać tych PDU albo obsłużyć
  ponownej aktywacji, implementacja opisuje to w §6 i zostaje przy AC ręcznych.

  **Uzupełnienie 2026-09-24 (Approved, właściciel), po awarii AC-5:** test
  MUSI odtwarzać wyścig, który tę awarię wywołał — klient wysyła Refresh Rect
  i **natychmiast** Suppress Output `allowDisplayUpdates = 1`, bez czekania na
  odpowiedź serwera, a sesja MUSI przeżyć wznowienie i dostać pełny obraz.
- **F-008.13** (MUST) Istniejące testy przechodzą bez zmian, w tym
  `tests/architecture.rs` i `tests/rdp_pixels.rs`.

## 3. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-008.3–F-008.5 | MS-RDPBCGR 2.2.11.3 Client Suppress Output PDU (str. 226–227), 3.3.5.11.2 (str. 330); `ironrdp-server-0.13.0/src/server.rs:618`, `:1589-1593`; `builder.rs:230` |
| F-008.6 | MS-RDPBCGR 2.2.11.2 Client Refresh Rect PDU (str. 224–225), 3.3.5.11.1 (str. 330), Appendix A <38> (str. 437–438), 2.2.11.1 Inclusive Rectangle (str. 224) |
| F-008.7 | MS-RDPBCGR 2.2.3 Deactivation–Reactivation (str. 116–117), 3.3.5.5 (str. 325); `ironrdp-server-0.13.0/src/server.rs:1047-1051`, `:1236`, `:1482-1484`, `:1739-1750`; `encoder/mod.rs:163`, `:240-271`, `:318-330`; `src/capture/mod.rs:275-287` |
| F-008.1 | `src/main.rs:237-252` (`MAC_RDP_LOG`, `EnvFilter`); komunikaty: `ironrdp-server-0.13.0/src/server.rs:1592`, `:1604`; `encoder/mod.rs:380`, `:431` |
| całość | General Capability Set 2.2.7.1.1 (str. 134–137); `specs/ref/MS-RDPBCGR-v20260309.txt`; `specs/02-ms-rdpbcgr.md` §3.3, §4 P-06, §4.1 |

## 4. Kryteria akceptacji

Część czysta — testy w CI. Część z klientem — **checklista ręczna**, bo
zachowanie okna klienta i macOS-a nie daje się sprawdzić bez GUI.

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, `cargo test --all-targets`.
   Testy z F-008.11 (i F-008.12, jeśli powstał) przechodzą.
2. **AC-2 — pomiar D-2 (przed kodem, F-008.1).**
   - Uruchom serwer z
     `MAC_RDP_LOG=info,mac_rdp_server=debug,ironrdp_server=debug`.
   - Wykonaj Cmd+Tab **na Macu** (przez klienta), a potem Alt+Tab/Cmd+Tab
     **na maszynie klienta**, po 10 razy.
   - Zapisz w §6, które z komunikatów z F-008.1 się pojawiły i przy którym
     wariancie.
   - Przy każdym zamrożeniu odpowiedz na pytanie z F-004 §8 pkt 6: czy ruch
     myszy po zamrożonym obszarze go naprawia?
3. **AC-3 — minimalizacja przy ruchu na Macu.**
   - Na Macu odtwarzaj wideo, a okno klienta zminimalizuj na 30 s.
   - Log pokazuje przejście `suppressed` (F-008.2), a w tym czasie nie
     rośnie licznik `bytes_sent` w `damage stats`.
   - Po przywróceniu okna w ciągu 1 s widać pełny, aktualny obraz, bez
     fragmentów sprzed minimalizacji.
4. **AC-4 — statyczny pulpit.**
   - Zminimalizuj okno klienta.
   - Na Macu zmień coś jednym ruchem (np. otwórz okno skrótem z lokalnej
     klawiatury) i zostaw pulpit bez ruchu.
   - Przywróć okno klienta: zmiana jest widoczna od razu, mimo że capture
     nie dostarcza nowych klatek (F-008.5).
5. **AC-5 — ponowna aktywacja jest nieszkodliwa.** Przy przywróceniu okna
   (AC-3/AC-4) okno klienta nie zmienia rozmiaru ani skalowania, sesja się nie
   rozłącza, a miganie jest najwyżej jednokrotne. Sprawdzić **Windows App
   (macOS)** i **mstsc (Windows)**; xfreerdp, jeśli dostępny. Porażka ⇒
   F-008.7, ścieżka zatrzymania.
6. **AC-6 — koalescencja.** 10× minimalizuj/przywróć w ciągu ~5 s. Sesja
   działa dalej, a log pokazuje co najwyżej tyle ponownych aktywacji, ile
   pozwala F-008.9 (≤ ~6), bez błędów.
7. **AC-7 — Cmd+Tab (F-004 AC-5).** Scenariusz z F-004 AC-5: przełączanie
   aplikacji Cmd+Tab, zamknięcie okna, przeciąganie okna.
   - Wszystko powtórzone przy `--max-fps 5`.
   - Nigdzie nie zostaje fragment poprzedniej treści.
   - Jeśli zostaje, notujemy, czy log F-008.2 pokazał przejście. Brak przejścia
     oznacza inną przyczynę niż P-06, a F-004 §8 pkt 6 pozostaje otwarte
     z tym ustaleniem.
8. **AC-8** `--encoder full`: AC-3 i AC-4 dają ten sam wynik (F-008.10).
9. **AC-9** Kilkunastominutowa sesja z kilkoma minimalizacjami: brak regresji
   F-004 (AC-2 statyczny pulpit ≈ 0 B/s) i F-007 (reconnect).

## 5. Poza zakresem

- **Refresh Rect bez wcześniejszej przerwy — znana luka wobec G-2 (MUST).**
  IronRDP 0.13.0 nie przekazuje go backendowi (`server.rs:1602-1606`), więc
  po naszej stronie nie da się go wykryć. Zostaje jako **jawne odstępstwo**
  do czasu:
  - poprawki w forku IronRDP (ADR-0011, Etap 1 — to jest pierwsza poprawka,
    która uzasadnia fork), albo
  - wydania upstream, które to naprawi (na `master` 2026-09-16 bez zmian).
- **Odświeżanie tylko prośnego prostokąta** zamiast całego pulpitu —
  optymalizacja możliwa dopiero, gdy prostokąty z Refresh Rect będą dostępne
  (jw.). Uwaga na włączne współrzędne `TS_RECTANGLE16` (2.2.11.1, str. 224).
- **Zatrzymanie capture na czas przerwy** (oszczędność CPU) — F-107. F-008
  tylko nie koduje i nie wysyła.
- **Sprawdzenie `desktopResizeFlag` klienta przed ponowną aktywacją** —
  IronRDP 0.13.0 go nie udostępnia (`server.rs:1408-1410`). Dotyczy też F-006;
  rozstrzygane tam albo w forku. Główni klienci go ogłaszają [I — do
  potwierdzenia przy D-5].
- **Frame Marker, EGFX, nowe kodeki** — ADR-0013, F-104.
- **Wiele monitorów i `desktopRect` w Suppress Output** — F-105.

## 6. Status

**Implemented (2026-09-18)** (Approved 2026-09-17, właściciel). D-2 (AC-2)
wykonany przed kodem, zgodnie z F-008.1 — wyniki w tabeli niżej.

Implementacja (commity `6154272`, `4358e96`, `afdc980`):
- `src/session/suppress.rs` — `SuppressTracker` (F-008.8, F-008.9), bez typów
  IronRDP;
- `src/session/display.rs` — odczyt flagi co ≤ 50 ms przez `tokio::select!`
  ze snem (F-008.5), porzucanie klatek bez kodowania (F-008.4), pełne
  odświeżenie przez `DisplayUpdate::Resize` z niezmienionym rozmiarem
  (F-008.6, F-008.7); tracker przechodzi przez ponowną aktywację, więc odstęp
  1 s obowiązuje także po niej; log `info` przejść (F-008.2);
- `src/session/mod.rs` — jedna flaga `Arc<AtomicBool>` przekazana do
  `with_display_suppressed_handle` i do `MacRdpDisplay` (F-008.3).

Testy:
- jednostkowe `session::suppress::tests::f008_*` (F-008.2, F-008.8, F-008.9,
  F-008.11);
- integracyjne `tests/suppress_output.rs` (F-008.12, powstał):
  - klient IronRDP wysyła Suppress Output; przez 2 s zero aktualizacji
    obrazu, mimo że wzorzec się zmienia;
  - po wznowieniu klient dostaje Deactivate All, przechodzi ponowną aktywację
    przy tym samym rozmiarze, a obraz odmalowany od czarnego zgadza się
    piksel w piksel z klatką wzorca;
  - oba enkodery (F-008.10);
  - sprawdzone, że test pada przy wyłączonej przerwie.

F-008.5 (≤ 100 ms) wynika z konstrukcji (sen 50 ms). Test integracyjny
pokrywa to pośrednio: cisza zaczyna się w oknie karencji 300 ms.

**Do weryfikacji ręcznej właściciela:** AC-3…AC-9.

### Wyniki pomiarów

| Pomiar | Data | Klient | Wynik |
|---|---|---|---|
| D-2 / AC-2 (częściowy) | 2026-09-17 | mstsc, Windows 11 build 26100 (log `target/measure-windowsapp.log` — nazwa pliku myląca, klientem był mstsc) | Wstrzykiwanie wejścia było wyłączone (brak Accessibility), więc wykonany tylko wariant „przełączenie **po stronie klienta**”. Sekwencja od klienta: **14:09:22.475** Suppress Output (`suppress=true`) → po 1,6 s **14:09:24.054** Refresh Rect, zaraz po nim **14:09:24.057** Suppress Output (`suppress=false`) → **14:09:24.152** Tab↑, **SyncLocks**, LShift↑, RShift↑, LCtrl↑. Druga przerwa od **14:09:31.028** (`suppress=true`, bez wznowienia do końca logu). **W trakcie tej przerwy serwer wysłał dalej ~170 MB** (`bytes_sent` 45 580 288 → 215 867 392 między 14:09:30 a 14:09:40, pełne klatki „too many separate regions”) — naruszenie G-3 potwierdzone na prawdziwym kliencie. Komunikatów `Dropping bitmap update…` ani `Failed to extract bitmap subregion` **brak**. Wariant „Cmd+Tab na Macu” i pytanie o ruch myszą — do powtórzenia z Accessibility. |
| D-2 / AC-2 (drugi przebieg, z działającym wejściem) | 2026-09-18 | mstsc, Windows 11 build 26100 (log `target/measure-mstsc-2.log`, 07:36–07:43 UTC) | Accessibility nadane, wejście wstrzykiwane (0 wpisów `not injected`). **Właściciel nie zaobserwował zamrożenia obrazu** („seems to be working”). Klient wysłał **jeden** Suppress Output (`suppress=true`, 07:38:02 UTC) i **nie wysłał wznowienia ani Refresh Rect** do końca sesji (07:43:46). Przez te 5,7 min serwer wysłał dalej **~10,3 GB** (`bytes_sent` 3 343 106 048 → 13 643 169 792, ok. 34 MB/s), w większości pełne klatki „too many separate regions” (109 w logu). Komunikatów `Dropping bitmap update…` / `Failed to extract bitmap subregion` brak. **Wyjaśnione przez właściciela (2026-09-18):** o 09:38:02 czasu lokalnego okno mstsc zostało **zminimalizowane** i pozostało tak do końca sesji — brak wznowienia jest więc poprawny, a stan „suppressed” w IronRDP wiarygodny. Założenie F-008.6/F-008.9 (wznowienie przychodzi przy przywróceniu okna) nie zostało obalone; samo przywrócenie sprawdza AC-3. Cały ruch w tym oknie (~10,3 GB) był zbędny — wprost to, co usuwa F-008.4. |
| Połączenie (obserwacja uboczna, oba przebiegi) | 2026-09-17/18 | mstsc | Pierwsza próba połączenia kończy się `Connection reset by peer` w finalizacji, druga (po ~1,2 s) się udaje. Klient żąda `SSL \| HYBRID \| HYBRID_EX`, serwer wybiera `SSL`. Hipoteza [I]: mstsc zrywa pierwsze połączenie przy ostrzeżeniu o certyfikacie self-signed. |

### Test ręczny — BRAMKA C, pierwszy przebieg (2026-09-18)

Klient: mstsc, Windows 11 build 26100. Log `target/measure-mstsc-6.log`
(lokalny), 12:39–12:45 UTC, `--encoder damage`, domyślne `--max-fps`.
Właściciel: wykonane wszystko poza punktami 5 i 6 checklisty (AC-7, AC-8).

- **AC-3 — spełnione, potwierdzone logiem.**
  - 12:40:51 `client paused display updates`.
  - Przez 145 s przerwy (wideo na Macu) zero wpisów `damage stats`: enkoder
    nie był wołany, nic nie wysłano.
  - 12:43:17 `client resumed display updates paused_ms=145360
    skipped_frames=4262`; 16 ms później `full refresh: reactivating`, a po
    150 ms `Client accepted` (ponowna aktywacja zakończona).
  - Kolejność PDU od mstsc przy przywróceniu: Refresh Rect (zeruje flagę),
    potem Suppress Output `suppress=false`.
- **AC-4** — spełnione według właściciela. Log tego nie rozstrzyga.
- **AC-5** — sesja się nie rozłączyła: po 12:40:17 brak `connection ended`
  do końca logu, a obie ponowne aktywacje (12:43:17, 12:44:57) zakończyły się
  `Client accepted` w 150–290 ms. Rozmiaru, skalowania i migania log nie
  pokazuje.
- **AC-6** — w logu tylko dwa cykle przerwy (12:40:51, 12:44:55); seria
  10 minimalizacji w ~5 s się nie pojawia.
- **AC-7** — spełnione przy domyślnym `--max-fps` według właściciela
  (Cmd+Tab, zamknięcie okna, przeciąganie; bez fragmentów poprzedniej
  treści). Powtórzenie przy `--max-fps 5` niewykonane.
- **AC-8** — niewykonane.
- **AC-9** — sesja trwała ~5 min, a kryterium mówi o kilkunastu minutach.
  Właściciel uznał 5 min za wystarczające (decyzja 2026-09-18).
- Obserwacja poza F-008: przy odtwarzanym wideo `damage` wysyła do
  ~190 MB/s (5120×1440, pełne klatki „too many separate regions”). To temat
  ADR-0013 i F-104, nie F-008.

### AC-5 — NIE PRZESZŁO (2026-09-24). Ścieżka zatrzymania z F-008.7

Log właściciela, 2026-09-24 11:34:39 UTC, klient z kanałami DVC
`FreeRDP::Advanced::Input`, `Microsoft::Windows::RDS::DisplayControl`, `ECHO`
(czyli klient z rodziny FreeRDP — prawdopodobnie Windows App, nie mstsc),
sesja trwała ~3,4 h, pulpit 5120×1440.

Przebieg:

```
11:34:39.682914  client RefreshRectangle cleared suppress-output state
11:34:39.684292  client resumed display updates paused_ms=58613 skipped_frames=1719
11:34:39.684309  full refresh: reactivating at the same desktop size
11:34:39.684404  End of client loop: DeactivationReactivation
11:34:39.685043  ERROR Connection error: failed to accept client during finalize
                 Caused by: [expected client confirm active
                 @ ironrdp-acceptor-0.10.0/src/connection.rs:848]
```

Klient rozłączył się i wrócił po ~10 s (dwa podejścia, drugie udane).

**Przyczyna (ustalona z kodu).** W `ironrdp-acceptor-0.10.0`
`connection.rs:833-848`: w trakcie ponownej aktywacji akceptor **toleruje**
PDU, którego nie umie zdekodować jako `ShareControlHeader` („Dropping
unexpected PDU during reactivation”), ale PDU, które **zdekoduje się
poprawnie** i nie jest `ClientConfirmActive`, kończy połączenie błędem
`general("expected client confirm active")`. Każdy Share Data PDU w locie
w chwili Deactivate All zabija więc sesję.

To wyścig, nie determinizm — i dlatego 2026-09-18 się udało:
- 09-18: klient wysłał Refresh Rect, **2,1 ms później** Suppress Output
  `suppress=false`, a nasz `Resize` poszedł **15 ms po** obu; nic nie zostało
  w locie i ponowna aktywacja przeszła (dwa razy).
- 09-24: `Resize` poszedł **0,6 ms** po Refresh Rect, więc towarzyszące mu
  Suppress Output trafiło już w stan reaktywacji → błąd.

Nasz test integracyjny (F-008.12) tego nie łapie: klient IronRDP wysyła
tylko jedno PDU wznowienia i czeka.

**Zgodnie z F-008.7 implementacja zatrzymuje się tutaj.** Poniżej propozycja
zmiany, do decyzji właściciela.

### Propozycja zmiany F-008.7 — Draft (2026-09-24)

Warianty, od najmniej do najbardziej ryzykownego:

1. **(Rekomendowany) Pełne odświeżenie bez Deactivate All.** Po wznowieniu
   unieważniamy **tylko bazę naszego enkodera** (nowa metoda `Encoder`, np.
   `invalidate_baseline()`, czysta i testowalna) i kodujemy pełną klatkę.
   Diff kafelkowy IronRDP odetnie kafelki, które klient już ma — a jego kopia
   obrazu jest z definicji tym, co klientowi wysłano.
   - Koniec z ponowną aktywacją, więc wyścig z `connection.rs:848` znika
     w całości.
   - Cena: jeśli klient **wyrzucił** swój obraz w trakcie przerwy, jego
     nieruchome fragmenty zostaną nieodświeżone. Żaden z testowanych klientów
     się tak nie zachował (obraz po przywróceniu był poprawny), ale gwarancji
     z Appendix A <38> to nie daje.
   - Znana luka wobec G-2 zostaje i idzie do forku (ADR-0011).
2. **Ponowna aktywacja z opóźnieniem.** Zostawiamy `Resize`, ale dopiero
   ~250 ms po wznowieniu, żeby PDU towarzyszące wznowieniu zostały
   przetworzone. Zwęża wyścig, nie usuwa go; każde Share Data PDU od klienta
   (np. kolejne Suppress Output) nadal może trafić w okno reaktywacji.
3. **Poprawka w forku IronRDP (ADR-0011, Etap 1).** W akceptorze podczas
   reaktywacji odrzucać także PDU, które zdekodowały się poprawnie, ale nie są
   `ClientConfirmActive` — symetrycznie do gałęzi obok. Kandydat na PR
   upstream. To jest właściwa naprawa, ale nie odblokowuje Fali 3 dziś.

Do tego, niezależnie od wariantu: **test integracyjny musi odtwarzać wyścig**
— klient wysyła Refresh Rect i natychmiast Suppress Output, bez czekania
(rozszerzenie F-008.12).

**Drugie wystąpienie (2026-09-24 11:38:44, ten sam klient, sesja 235 s).**
Identyczny przebieg: `Display resize` → `DeactivationReactivation` → w ciągu
**1 ms** `expected client confirm active`, rozłączenie, nieudana próba
powrotu po 9 s i udana po 11 s. Razem 2026-09-24 padły **dwa z dwóch**
zaobserwowanych wznowień. Wyścig jest więc dla tego klienta regułą, nie
wyjątkiem, i wariant 2 (opóźnienie) tym bardziej nie wystarcza — rekomendacja
przesuwa się jednoznacznie na wariant 1 (odświeżenie bez Deactivate All).

### Implementacja F-008.7v2 (2026-09-24)

Commity `9496d57` (enkoder) i `41f6dac` (pętla obrazu i test).

- `Encoder::invalidate_baseline()` — domyślnie nic nie robi (enkoder pełnych
  klatek i tak wysyła wszystko); `DamageEncoder` kasuje `previous` i oznacza
  następną pełną klatkę powodem `baseline invalidated (client resumed)`.
  Testy: `encoder::damage::tests::f008_7v2_*`.
- `session::display` przy `RefreshDue` woła `invalidate_baseline()` i koduje
  **najnowszą klatkę zapamiętaną w trakcie przerwy**, dzięki czemu nieruchomy
  pulpit jest odmalowany od razu (AC-4), a nie przy najbliższej zmianie.
  `DisplayUpdate::Resize` nie jest już wysyłany, więc znika przekazywanie
  trackera przez ponowną aktywację.
- `tests/suppress_output.rs` wysyła wznowienie jako **jeden zapis**: Refresh
  Rect + Suppress Output, czyli dokładnie wyścig z §6. Asercje: sesja nie
  dostaje Deactivate All ani Terminate, a nieaktualny obraz klienta staje się
  pełną, **nowszą** klatką wzorca.

**Czego test nie pokrywa (ustalone empirycznie 2026-09-24).** Próba asercji
„odmaluj od czarnego obrazu” **pada**: IronRDP porównuje każdą bitmapę
z własną kopią i wycina kafelki, które się zgadzają. To jest dokładnie
odstępstwo zapisane w F-008.7v2 — klient, który porzuci obraz w trakcie
przerwy, nie dostanie ich z powrotem. Domknięcie: fork (ADR-0011, Etap 1).

**Czego test nie izoluje.** Po wyłączeniu `invalidate_baseline()` test nadal
przechodzi: skoro w trakcie przerwy nic nie wysyłamy, zwykły diff i tak
wyśle wszystko, co się zmieniło. Unieważnienie bazy jest zabezpieczeniem
wymaganym przez F-008.6 (pełny pulpit) i pokrywają je testy jednostkowe
enkodera, nie integracyjny.

**Do ponownego testu ręcznego:** AC-3…AC-9 (pełna checklista), bo zmienił się
mechanizm odświeżania.

### Wariant 3 zrealizowany w forku (2026-10-12, ADR-0014)

Odłożony wyżej wariant 3 („poprawka w forku IronRDP") **wszedł** razem z łatką
na środkowy przycisk — bo skoro fork i tak powstaje, drugi powód jest już
opłacony. `ironrdp-acceptor` w rewizji `ac4cd7db5e09a3293ffe2cede01b41e17a78bf2e`
w stanie `CapabilitiesWaitConfirm` przy `reactivation` porzuca poprawnie
zdekodowany PDU, który nie jest `ClientConfirmActive`, zamiast kończyć
połączenie — symetrycznie do dwóch gałęzi obok, które od zawsze porzucały PDU
niedekodowalne. Na pierwszym połączeniu błąd zostaje bez zmian. Dwa testy
jednostkowe w akceptorze; bez poprawki ten od reaktywacji pada.

**Co to zmienia, a czego nie.** Przyczyna rozłączeń z §6 znika: klient może
dosłać Refresh Rect i Suppress Output w okno reaktywacji i nic się nie stanie.
**F-008.7v2 zostaje jednak bez zmian** — odświeżanie nadal idzie bez
`Deactivate All`. Powrót do `DisplayUpdate::Resize` (i tym samym domknięcie
odstępstwa wobec Appendix A <38> dla klienta, który porzucił obraz) to osobna
zmiana wymagania, nie efekt uboczny podmiany zależności: wymaga nowego wpisu
w specu i zgody właściciela. Dziś mamy działające wznowienie **i** sprawny
akceptor; dopiero gdyby się okazało, że któryś klient czyści obraz w trakcie
przerwy, warto tę zmianę rozważyć.

Porzucony PDU jest tracony, nie odkładany: Refresh Rect i tak jest zbędny
(odmalowujemy po wznowieniu sami), a Suppress Output każdy obserwowany klient
wysyła ponownie. Odkładanie oznaczałoby kolejkę stanu wejścia w akceptorze,
czego on dziś nie robi.

**Do przetestowania ręcznie:** AC-5 z bramki C — ten sam klient z rodziny
FreeRDP/Windows App, kilka cykli zmiany rozmiaru okna i minimalizacji.
Oczekiwanie: w logu nie ma `expected client confirm active` ani rozłączenia.
