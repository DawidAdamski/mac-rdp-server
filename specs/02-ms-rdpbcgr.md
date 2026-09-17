# 02 — MS-RDPBCGR: przegląd zgodności i plan własnej implementacji

**Status:** Draft (2026-09-17) — kierunek zaakceptowany przez właściciela 2026-09-17 (§8); treść czeka na approve
**Data:** 2026-09-17
**Źródło:** `specs/[MS-RDPBCGR].pdf` — *[MS-RDPBCGR] v20260309, Release: March 9, 2026*, 444 str.
**Dotyczy:** wszystkie fale; w szczególności F-003, F-004, F-005, F-006, F-007, F-101, F-105, F-108, **F-200**
**Powiązane ADR:** ADR-0001 (silnik), ADR-0004 (TLS), ADR-0006 (kodeki), ADR-0007 (przyciski myszy), **ADR-0011 (strategia F-200: fork IronRDP, Proposed)**
**Tekst źródłowy do grep:** `specs/ref/MS-RDPBCGR-v20260309.txt` (PDF jest w `.gitignore`)

## 0. Czym ten dokument jest, a czym nie

To **nie jest spec funkcjonalności** i niczego nie zatwierdza. Ma dwa zadania:

1. **Przegląd (§3–§4):** co mówi MS-RDPBCGR o serwerze i gdzie dzisiejsza
   konfiguracja — IronRDP 0.13.0 + nasz kod — od tego odstaje.
2. **Plan (§5–§7):** jak rozłożyć F-200 (własna implementacja MS-RDPBCGR,
   `waves.md` §Backlog) na małe, osobno testowalne kroki, które da się potem
   zamienić w cele implementacyjne.

Każde ustalenie z §4 trafia do kodu **wyłącznie** przez aktualizację
odpowiedniego speca albo nowy ADR (zasada zero z `CLAUDE.md`).

### Konwencje

- **Strony** to numery ze stopki PDF („N / 444”) — te same, co znaczniki
  `==== page N ====` w `specs/ref/`. Zakresy w §1 wzięte ze spisu treści,
  który od okolic sekcji 3 podaje numery o 1 większe; w §3 — z odczytu treści,
  dokładność ±1.
- Sekcje normatywne MS-RDPBCGR: **1.5, 1.8, 1.9, 2 i 3** (str. 17).
  Sekcja 4 (przykłady) i Appendix A są informacyjne, ale Appendix A mówi,
  **jak naprawdę zachowują się klienci Microsoftu**, więc traktujemy go poważnie.
- Pewność ustaleń:
  - **[Z]** — zweryfikowane w źródle pinowanych cratów albo w naszym kodzie
    przy pisaniu tego dokumentu,
  - **[R]** — odczytane z kodu podczas przeglądu, bez osobnej weryfikacji,
  - **[I]** — inferencja; wymaga pomiaru albo testu, zanim zostanie użyta
    jako argument.
- Ścieżki IronRDP są względne wobec `~/.cargo/registry/src/*/`.

---

## 1. Mapa dokumentu z perspektywy serwera

| Obszar | Sekcje | Strony | Dla nas |
|---|---|---|---|
| Przegląd przepływów | 1.3.1–1.3.10 | 23–36 | Czytać raz; kolejność sekwencji połączenia (1.3.1.1) jest kluczowa |
| Transport | 2.1 | 38 | TCP 3389, little-endian, MCS SDrq/SDin ≤ 16 383 B |
| Sekwencja połączenia (PDU) | 2.2.1 | 38–112 | **Rdzeń F-200** |
| Rozłączenie | 2.2.2, 3.3.5.4 | 112–116, 325 | Shutdown Denied, DPUM |
| Deactivation–Reactivation | 2.2.3, 3.3.5.5 | 116–117, 325 | **Resize (F-006)** |
| Auto-reconnect | 2.2.4, 5.5 | 117–120, 433 | Odłożone do po F-101 |
| Error Info / Status Info | 2.2.5, 3.3.5.7 | 120–132, 326 | Informowanie klienta, czemu rozłączamy |
| Static Virtual Channels | 2.2.6, 3.1.5.2 | 132–135, 257 | Warunek F-102 (clipboard) i F-103 (audio) |
| Capability sets | 2.2.7 | 135–161 | **Rdzeń F-200** |
| Input | 2.2.8, 3.3.5.8 | 161–187, 326 | F-003 / F-003a — dziś i w F-200 |
| Wyjście graficzne | 2.2.9, 3.3.5.9 | 187–217, 328 | F-004 / F-005 / F-104 |
| Logon notifications | 2.2.10 | 217–224 | Early User Authorization Result (F-101) |
| Refresh Rect / Suppress Output | 2.2.11, 3.3.5.11 | 224–228, 331 | **Luka dziś** (§4, P-06) |
| Monitor Layout | 2.2.12, 3.3.5.12 | 228–230, 331 | F-105 |
| Server Redirection | 2.2.13 | 230–238 | Poza zakresem |
| Auto-detect sieci | 2.2.14, 3.3.5.14 | 238–246, 332 | Później (RTT) |
| Multitransport | 2.2.15 | 246–248 | Poza zakresem |
| Heartbeat | 2.2.16 | 249–250 | F-007 / F-108 |
| RDSTLS, RDS AAD | 2.2.17–2.2.18 | 250–256 | Poza zakresem |
| Kompresja MPPC | 3.1.8 | 261–270 | Później (SHOULD) |
| Interleaved RLE | 3.1.9 | 270–284 | Pseudokod **dekodera**; serwer 32 bpp go nie potrzebuje |
| Model danych i reguły serwera | 3.3 | 309–333 | **Rdzeń F-200** — główne źródło wymagań MUST |
| Przykłady z adnotacjami | 4.1–4.10 | 334–409 | Wektory testowe (z zastrzeżeniem w §7.3) |
| Bezpieczeństwo | 5.3, 5.4 | 410–433 | 5.4 (TLS, CredSSP) — tak; 5.3 (RC4) — nie |
| Zachowanie produktów | 6 | 435–440 | Odchylenia klientów MS od litery specyfikacji |

---

## 2. Zakres: co serwer tego projektu musi umieć, a co świadomie pomija

### 2.1 W zakresie

- **Enhanced RDP Security** (5.4): TLS 1.2 **i** 1.3 — Windows 10 i Server 2019
  nie mają TLS 1.3 (Appendix A <55>).
- **HYBRID i HYBRID_EX** (CredSSP/NLA) z **Early User Authorization Result
  PDU** (2.2.10.2, str. 223). Ścieżka F-101.
- Pełna sekwencja połączenia z 1.3.1.1 w wariancie „negotiation-based”
  (5.4.2.1).
- Wyłącznie **fast-path** dla wyjścia graficznego. Klienci MS od 7.1 i tak nie
  dostają grafiki slow-path od serwerów MS (<36>, str. 437).
- Input slow-path **i** fast-path. Serwer nie wybiera, którą ścieżką klient
  wyśle zdarzenia.
- Refresh Rect, Suppress Output, Monitor Layout, Deactivation–Reactivation,
  Set Error Info, Shutdown Denied.
- Static Virtual Channels — dopiero gdy pojawi się pierwszy kanał (F-102).

### 2.2 Poza zakresem — z uzasadnieniem, żeby nie wracało

| Element | Dlaczego nie |
|---|---|
| **Standard RDP Security** (5.3: RC4, proprietary certificate) | Sama specyfikacja mówi, że nie chroni przed MITM (1.3.1.2, str. 28). Odpowiedź: `SSL_REQUIRED_BY_SERVER` / `HYBRID_REQUIRED_BY_SERVER`. |
| FIPS encryption level | To poziom szyfrowania Standard RDP Security, nie osobny protokół. |
| **Direct Approach** (5.4.2.2) | CredSSP przed X.224. Nie obsługuje Early User Authorization Result; główni klienci go nie potrzebują [I]. |
| RDSTLS (5.4.5.3), RDS AAD Auth (5.4.5.4) | RDSTLS służy głównie przekierowaniu serwera; AAD wymaga Entra ID. |
| Server Redirection (2.2.13) | Nie mamy brokera. Non-goal z `00-vision.md` §5 (RDS). |
| Licencjonowanie poza „Valid Client” (MS-RDPELE) | Serwer konsoli nie wydaje licencji. |
| Multitransport / UDP (2.2.15, MS-RDPEUDP) | Nie w tym horyzoncie. `SC_MULTITRANSPORT` nie jest wysyłany. |
| Orders / MS-RDPEGDI, palety, glyph/brush/bitmap cache | Źródłem jest bitmapa z ScreenCaptureKit, a nie wywołania GDI. Palety dotyczą tylko 8 bpp. |
| Grafika slow-path | Patrz <36>. |
| Auto-reconnect (2.2.4, 5.5) | Bez uwierzytelnienia ciasteczko niczego nie chroni, a konsola i tak istnieje. **Wraca po F-101.** |
| IME Status PDU | Wysyłały go tylko serwery 5.0–7.1 (<35>). |
| Save Session Info (poza PLAINNOTIFY) | Przepływy logowania RDS. Najwyżej tani PLAINNOTIFY (3.3.5.10.1). |

---

## 3. Reguły, na których najłatwiej się pomylić

Wybór z sekcji 2 i 3 — nie całość. To są miejsca, w których IronRDP albo my
już odstajemy (§4), albo które F-200 musi trafić za pierwszym razem.

### 3.1 Połączenie i bezpieczeństwo

| # | Reguła | § / str. |
|---|---|---|
| C-1 | X.224 CR krótszy niż 11 B, niespójny TPKT albo klasa ≠ 0 ⇒ rozłącz. `routingToken` i cookie MUSZĄ być ignorowane. | 3.3.5.3.1, 312 |
| C-2 | CR bez `rdpNegData` oznacza Standard RDP Security, a więc u nas odmowę. Po `RDP_NEG_FAILURE` serwer MUSI zamknąć połączenie. | 3.3.5.3.1, 312; 2.2.1.2.2, 43 |
| C-3 | `RDP_NEG_RSP` z `EXTENDED_CLIENT_DATA_SUPPORTED` — bez tej flagi klient nie wyśle bloków monitorów ani kanału komunikatów. | 2.2.1.2.1, 41 |
| C-4 | Parametry domeny MCS scalane pseudokodem `MergeDomainParameters`. Wynik z przykładu: `maxUserIds = 3`, `maxMCSPDUsize = 65528`. | 3.3.5.3.3, 313–317; 4.1.4 |
| C-5 | **Anti-downgrade:** `serverSelectedProtocol` od klienta ≠ protokół z `RDP_NEG_RSP` ⇒ SHOULD rozłączyć. `SC_CORE.clientRequestedProtocols` MUSI powtórzyć wartość klienta. | 5.4.2.1, 426; 2.2.1.4.2, 63 |
| C-6 | GCC Conference Create Request ≤ 1024 B, a przy rozszerzonych blokach ≤ 4096 B. Klucz H.221 `"Duca"`. | 3.3.5.3.3, 314 |
| C-7 | Monitory: najwyżej 16; współrzędne **włączne**; monitor główny w (0,0). | 2.2.1.3.6, 57–59 |
| C-8 | `SC_MCS_MSGCHANNEL` i `SC_MULTITRANSPORT` MUST NOT, jeśli klient nie przysłał odpowiedniego bloku. | 3.3.5.3.4, 317 |
| C-9 | Duplikat Channel Join SHOULD zostać zignorowany, bez potwierdzenia. Nieznany kanał dostaje Confirm z `result = 14`, a nie zerwanie połączenia. | 3.3.5.3.9, 319 |
| C-10 | Client Info PDU **zawsze** ma Basic Security Header z `SEC_INFO_PKT`. | 3.3.5.3.11, 320 |
| C-11 | Demand Active: `pduSource = 1002` (Server Channel ID). Monitor Layout: `pduSource = 0`. | 3.3.5.1, 310; 2.2.12.1, 229 |
| C-12 | Kolejność finalizacji: serwer wysyła Synchronize i Control(Cooperate) po Confirm Active, Granted Control po Request Control, a Font Map po Font List. Grafika dopiero po Font Map. | 1.3.1.1, 27–28; 3.3.5.3.14–22 |
| C-13 | Timer sekwencji połączenia: **60 s** (MAY rozłączyć). | 3.3.2.1, 3.3.6.1, 332 |
| C-14 | HYBRID_EX: Early User Authorization Result wysyłany **natychmiast** po CredSSP. Przy HYBRID bez EX odmowa idzie przez Set Error Info 9 **po** Channel Join. | 2.2.10.2, 223; 3.3.5.7.1.1, 325 |

### 3.2 Input

| # | Reguła | § / str. |
|---|---|---|
| I-1 | Synchronize ustawia locki **i zeruje stan klawiszy serwera do „wszystko puszczone”**. Klient SHOULD potem dosłać wciśnięcia tego, co trzyma. | 2.2.8.1.1.3.1.1.5, 173 |
| I-2 | Wartość kółka to **9-bitowe U2** (maska 0x01FF, bit znaku 0x0100). Przy WHEEL/HWHEEL pozycję ignorujemy. HWHEEL tylko przy ogłoszonym `TS_INPUT_FLAG_MOUSE_HWHEEL`. | 2.2.8.1.1.3.1.1.3, 172 |
| I-3 | Zdarzenie przycisku dzieje się **w xPos/yPos tego zdarzenia**. | 172 |
| I-4 | W `TS_POINTERX` `BUTTON1` (0x0001) to **przycisk 4**, a `BUTTON2` (0x0002) to **przycisk 5**. | 2.2.8.1.1.3.1.1.4, 173 |
| I-5 | Unicode to **jedna jednostka UTF-16** — para surogatów przychodzi jako dwa zdarzenia. | 2.2.8.1.1.3.1.1.2, 171 |
| I-6 | Pause = Ctrl z `EXTENDED1` + NumLock. | 170–171, 178–179 |
| I-7 | Nieznany typ zdarzenia ⇒ SHOULD rozłączyć. Każde przetworzone zdarzenie MUSI zostać wstrzyknięte. | 3.3.5.8.1, 325–327 |
| I-8 | Input Caps: `INPUT_FLAG_SCANCODES` MUST. Pola klawiatury wysyłane przez serwer SHOULD = 0. | 2.2.7.1.6, 147–148 |

### 3.3 Wyjście graficzne

| # | Reguła | § / str. |
|---|---|---|
| G-1 | **Trzy konwencje prostokątów.** Włączne: `TS_RECTANGLE16` (Refresh Rect, Suppress Output), `TS_BITMAP_DATA.destRight/Bottom`, `TS_MONITOR_DEF`. Wyłączne: `destRight/Bottom` w Set Surface Bits. | 224; 191; 58; 214 |
| G-2 | Po Refresh Rect serwer **MUST** wysłać grafikę dla tego obszaru. | 3.3.5.11.1, 330 |
| G-3 | Po Suppress Output serwer **MUST** przestać, a po ALLOW wznowić wysyłanie. Appendix A <38>: serwery RDP 8.0/8.1 źle obsługiwały Refresh Rect, a obejściem jest para Suppress(0) → Suppress(1), która wymusza odświeżenie całego pulpitu. | 3.3.5.11.2, 330; 437–438 |
| G-4 | `MaxRequestSize` klienta to twardy limit złożonego fast-path update. | 2.2.7.2.6, 156 |
| G-5 | Fast-path PDU SHOULD ≤ 16 383 B; fragmentacja SINGLE/FIRST/NEXT/LAST. | 2.2.9.1.2, 204–206 |
| G-6 | Wskaźnik ≤ 32×32 bez `LARGE_POINTER_FLAG_96x96`. `cacheIndex` < `pointerCacheSize` **klienta**. Przy cache 0 nie wysyłamy New Pointer. | 2.2.9.1.1.4.4, 200; 2.2.7.1.5, 146; 3.3.5.9.2, 328 |
| G-7 | Order Caps: `NEGOTIATEORDERSUPPORT` **MUST** być ustawione, nawet przy pustej tablicy orderów. | 2.2.7.1.3, 140 |
| G-8 | Surface Commands wymagają fast-path output; Frame Marker BEGIN/END z `frameId`. | 2.2.7.2.9, 158; 2.2.9.2.2, 216 |
| G-9 | `codecID` w Surface Bits nadaje **klient**; 0 oznacza „bez kodeka”. | 2.2.9.2.1, 215 |
| G-10 | Serwery MS uznają fast-path output tylko przy `FASTPATH_OUTPUT_SUPPORTED` **razem z** `LONG_CREDENTIALS_SUPPORTED` (<24>). | 437 |

### 3.4 Cykl życia i kanały

| # | Reguła | § / str. |
|---|---|---|
| L-1 | Shutdown Request przy zalogowanym użytkowniku ⇒ serwer **MUST** wysłać Shutdown Request Denied. | 3.3.5.4.1, 324 |
| L-2 | Serwer MUST ignorować powód w DPUM od klienta. Po własnym DPUM połączenie MUSI zostać zamknięte. | 3.1.5.1, 256 |
| L-3 | Resize przez Deactivate All wymaga `desktopResizeFlag` klienta. | 2.2.7.1.2, 138; 3.3.5.5.1, 324 |
| L-4 | Set Error Info tylko przy `RNS_UD_CS_SUPPORT_ERRINFO_PDU`; po nim serwer MUSI rozłączyć. | 3.3.5.7, 325 |
| L-5 | Kanały wirtualne: kawałki ≤ 1600 B, chyba że obie strony podały `VCChunkSize`. Bufor składania sięga `totalLength` z FIRST. | 3.1.5.2, 257–259 |
| L-6 | Heartbeat **wyłącznie** kanałem komunikatów, tylko przy `SUPPORT_HEARTBEAT_PDU`. | 2.2.16.1, 248 |

---

## 4. Przegląd stanu obecnego (IronRDP 0.13.0 + nasz kod, 2026-09-17)

Posortowane według wpływu na użytkownika dziś. Kolumna „Dokąd” mówi, który
dokument musi się zmienić, zanim powstanie kod.

| ID | Ustalenie | Pewność | Wpływ | Dokąd |
|---|---|---|---|---|
| **P-01** | **ADR-0006 opisuje inną ścieżkę niż ta, która idzie po drucie.** `UpdateEncoder::new` (`ironrdp-server-0.13.0/src/encoder/mod.rs:132-159`) wybiera `BitmapHandler` (RDP 6.0) **tylko wtedy, gdy klient nie ogłasza `SET_SURFACE_BITS`**. Przy ogłoszonym `SET_SURFACE_BITS` i pustej liście kodeków wybiera `NoneHandler` (`:491-499`), czyli **nieskompresowane Set Surface Bits z `codecID = 0`**. ADR-0006 nazywa ten wariant „gołe bitmapy RDP 6.0”. Poprawka z ADR-0006 (czarny ekran) pozostaje słuszna — błędny jest opis ścieżki. | [Z] kod; [I] który wariant dostaje mstsc/Windows App — do potwierdzenia pomiarem (D-1) | Wnioski wydajnościowe F-004 §7 (mowa o „kompresji RDP 6.0”) i F-004 §8 pkt 4 (Frame Marker „poza ścieżką”) mogą być nieaktualne | Nowy ADR częściowo zastępujący ADR-0006; aktualizacja F-004 §7/§8 |
| **P-02** | **Brak timeoutu i jedno połączenie naraz.** `RdpServer::run` czeka na `run_connection` bezpośrednio w pętli akceptowania (`server.rs:957-970`); w `server.rs` ani w `ironrdp-acceptor` nie ma żadnego `timeout`. Klient, który otworzy TCP i zamilknie, albo zniknie w trakcie sesji (Wi-Fi, uśpienie laptopa), blokuje serwer dla wszystkich. | [Z] | **DoS w LAN**; w praktyce „serwer przestał przyjmować połączenia” (F-007) | F-108 (model zagrożeń), F-007.10 |
| **P-03** | **Przyciski wstecz/dalej działają jak lewy/prawy.** IronRDP ogłasza `MOUSEX` (`capabilities.rs:72`), po czym `From<MouseXPdu>` mapuje `PointerXFlags::BUTTON1/2` na `LeftPressed/RightPressed` (`handler.rs:176-196`). Wg I-4 to przyciski 4 i 5. Klient zgodny ze specyfikacją wysyła więc „wstecz”, a Mac dostaje lewy klik. ADR-0007 tego nie obejmuje; nasze testy `Button4/5` w `src/session/convert.rs` dotyczą ścieżki, którą te zdarzenia nie przychodzą. | [Z] kod; [I] niezreprodukowane klientem | Przypadkowe kliknięcia | Aktualizacja ADR-0007 (nowy ADR), F-003; PR upstream |
| **P-04** | **Synchronize nie puszcza trzymanych klawiszy ani przycisków.** `src/input/cgevent.rs:175-177` ustawia tylko Caps Lock; `Modifiers` i `Buttons` zostają. Sink żyje przez cały proces, więc stan **przecieka między połączeniami** (rozłączenie z wciśniętym Cmd albo w trakcie przeciągania). Narusza I-1. | [Z] | Zablokowany Cmd po Cmd+Tab po stronie klienta | F-003 (nowe wymaganie) |
| **P-05** | **Surogat UTF-16 staje się literą „a”.** `cgevent.rs:166-173`: gdy `char::from_u32` zwraca `None`, zdarzenie i tak jest wysyłane z keycode 0 (`kVK_ANSI_A`) i bez tekstu. Emoji daje więc „aa” zamiast niczego. Zdarzenie niesie też bieżące modyfikatory, więc przy wciśniętym Cmd może odpalić skrót [R]. | [Z] | Śmieci w tekście | F-003a (asembler surogatów) |
| **P-06** | **Suppress Output i Refresh Rect nie są honorowane.** IronRDP tylko ustawia flagę `display_suppressed` (`server.rs:1589-1593`), a Refresh Rect jedynie ją zeruje i **niczego nie odmalowuje** (`:1602-1606`) — narusza G-2. Jednocześnie ogłasza obsługę obu PDU (`capabilities.rs:24-30`). U nas `grep display_suppressed src/` jest pusty, więc wysyłamy dalej, gdy klient prosi o przerwę (narusza G-3). Pełnego odświeżenia nie da się dziś wymusić, bo zjadają je dwa diffy: nasz (`damage.rs`) i wewnętrzny IronRDP (`encoder/mod.rs:240-271`). Jedyny reset to `DisplayUpdate::Resize`. | [Z] IronRDP, grep; [R] diffy | **Kandydat na przyczynę F-004 §8 pkt 6 (Cmd+Tab)** — patrz §4.1 | Nowy spec albo iteracja F-004 + ADR (reset bazy) |
| **P-07** | **Monitor Layout PDU jest błędny w trzech punktach:** jeden monitor na sztywno, `right = width` i `bottom = height` (a pola są włączne, więc o 1 px za dużo) oraz `pduSource` = kanał I/O zamiast 0 (`ironrdp-acceptor-0.10.0/src/connection.rs:786-800`). Narusza C-7 i C-11. | [Z] | Blokuje F-105; klienci tolerują [I] | F-105; PR upstream |
| **P-08** | **Brak kontroli anti-downgrade** (C-5): `serverSelectedProtocol` nie jest nigdzie czytany w acceptorze. | [Z] grep | Bezpieczeństwo — istotne od F-101 | F-101 / F-108 |
| **P-09** | **Każdy klient wchodzi.** W trybie TLS poświadczenia są sprawdzane tylko przy ustawionym `creds` (`connection.rs:696`), a `session::run` ich nie ustawia. | [R] (znane, ADR-0004) | Znane; stąd reguła „tylko loopback” | F-101 |
| **P-10** | **Kółko obcina ułamki notcha.** `cgevent.rs:136-137`: `dy / 120` — delty 1–119 (wysoka rozdzielczość, touchpady) znikają. | [Z] | Przewijanie touchpadem po stronie klienta nie działa albo działa skokami | F-003 |
| **P-11** | **Resize bez sprawdzenia `desktopResizeFlag`** klienta — flaga jest tylko logowana (`server.rs:1408-1410`), a Deactivate All i tak idzie. Narusza L-3. | [R] | Ryzyko przy F-006 | F-006 |
| **P-12** | **Limity klienta nie są czytane:** `MaxRequestSize` (serwer używa własnych 8 MiB) i `pointerCacheSize`. F-005.14 uzasadnia rozmiar cache wartością 2048, ale to jest capability **serwera**, a nie klienta. | [R] | Indeks kursora ≥ limitu klienta może zerwać sesję | F-005 (przed approve) |
| **P-13** | Order Caps bez `NEGOTIATEORDERSUPPORT` (`capabilities.rs:46-52`: `OrderFlags::empty()`) — narusza G-7. `keyboard_function_key = 128` łamie SHOULD z I-8. | [Z] | Brak objawów u znanych klientów | PR upstream; F-200 |
| **P-14** | **Cykl życia:** Shutdown Request zamyka połączenie bez Shutdown Denied (L-1); Set Error Info wysyłany tylko przy odmowie dostępu i bez sprawdzenia flagi klienta (L-4); przy zamykaniu serwera nie wysyłamy nic. Heartbeatu (L-6) nie ma w ogóle. | [R] | Klient pokazuje mylący komunikat przy rozłączeniu | F-007, F-108 |
| **P-15** | **Składanie kanałów wirtualnych bez limitu:** ignoruje FIRST i `totalLength` (`ironrdp-svc-0.8.0/src/lib.rs:328-350`). | [I] | Wzrost pamięci sterowany przez klienta, gdy powstanie pierwszy kanał | F-102 (warunek wstępny), F-108 |
| **P-16** | Ruch względny ignoruje stan przycisków (`cgevent.rs:111-125`), więc nie działa przeciąganie; brakuje też przycinania do ekranu. Luki w keymapie: 0x45, 0x46, 0x56 (ISO), E0 37, E0 5D, F13–F24; Insert → Home. Num Lock z Synchronize jest ignorowany. | [R]/[I] | Mniejszy | F-003a |
| **P-17** | Funkcja `MergeDomainParameters` nie jest używana (zawsze `target()`), `calledConnectId = 1`, a `USER_CHANNEL_ID` = 1002 = Server Channel ID (`connection.rs:25, :616-617`). Nieznany albo powtórzony Channel Join zrywa połączenie (C-9). | [Z]/[R] | Zgodność z egzotycznymi klientami | Tylko F-200 |
| **P-18** | IronRDP **ignoruje błędy dekodowania** Synchronize/Cooperate w finalizacji i wkłada Persistent Key List do zdarzeń wejściowych (`ironrdp-acceptor-0.10.0/src/finalization.rs:84-115`). | [R]/[I] | Nieznany | Tylko F-200 |
| **P-19** | Ścieżka RDP 6.0 (`encoder/bitmap.rs:44-50`) liczy wysokość fragmentu jako `chunk.len() / stride`. Dla wycinka z krótszym ostatnim wierszem wynik jest o 1 za mały. | [I] | Tylko jeśli klient nie ma Surface Commands (P-01) | Test regresyjny przed jakimkolwiek poleganiem na tej ścieżce |

**Stan upstreamu (2026-09-17):** niewydany `master` IronRDP ma już poprawione
P-03 i prawy/dolny brzeg z P-07 oraz częściowo P-02 (timeouty
finalizacji i negocjacji). P-06, P-13 i P-17 są bez zmian. Szczegóły
i numery linii: ADR-0011 §„Co upstream zmienił”.

### 4.1 Werdykt w sprawie Cmd+Tab (F-004 §8 pkt 6)

Hipoteza z F-004 („IronRDP odrzuca aktualizację spoza pulpitu”,
`encoder/mod.rs:379-386`) jest **mało prawdopodobna**. Nasze regiony są
przycinane do klatki, a pulpit ma rozmiar klatki. Odrzucenie mogłoby zajść tylko
przy zmianie geometrii, a tę nasz enkoder sam loguje
(`FullReason::GeometryChanged`). Poza tym takie odrzucenie zamroziłoby cały
ekran, a nie fragmenty.

**Mocniejszy kandydat — P-06** [I]. Cmd+Tab może zasłonić okno klienta, a klient
reaguje wtedy Suppress Output i/lub Refresh Rect (2.2.11.2: Refresh Rect służy
do odmalowania fragmentów okna zasłoniętych przez lokalne aplikacje; <38>:
para Suppress(0) → Suppress(1) jako sposób na pełne odświeżenie).
Serwer nie odmalowuje niczego, a oba diffy „wiedzą”, że klient ma aktualny
obraz. Jeśli klient w stanie wstrzymania nie zaprezentował aktualizacji, obszar
stoi, dopóki piksele pod nim się nie zmienią. To pasuje do „czasami” i do
„ruch myszy naprawia tylko tam, gdzie coś się przemalowało”.

Rozstrzygnięcie — patrz D-2 w §5. Trzeba też ustalić, **na której maszynie**
wciśnięto Cmd+Tab: na Macu (serwer) czy na kliencie.

---

## 5. Plan przeglądu — pomiary, które zamieniają [I] w [Z]

Bez kodu produkcyjnego: narzędzia już są (`examples/probe_client.rs`,
`SSLKEYLOGFILE` + Wireshark, `IRONRDP_LOG`).

| ID | Pytanie | Metoda | Rozstrzyga |
|---|---|---|---|
| D-1 | Którą ścieżką idzie obraz do mstsc, Windows App i xfreerdp? | Wireshark z `SSLKEYLOGFILE`: fast-path `updateCode` 1 (Bitmap) czy 4 (Surface Commands), a przy 4 — `codecID`. Do tego log `trace` IronRDP. | P-01, P-19 |
| D-2 | Co towarzyszy objawowi Cmd+Tab? | Filtr `mac_rdp_server=debug,ironrdp_server=debug`. Szukamy: `client suppress-output state changed`, `client RefreshRectangle cleared`, `Dropping bitmap update`, `Failed to extract bitmap subregion`. Pytanie dzielące: czy ruch myszy po zamrożonym obszarze go naprawia? | P-06, F-004 AC-5 |
| D-3 | Czy przycisk „wstecz” klika? | `probe_client` wysyła `TS_POINTERX` z `BUTTON1` ⇒ sprawdzamy, co dociera do `InputSink`. Ręcznie: mysz z przyciskami bocznymi w mstsc. | P-03 |
| D-4 | Czy milczący klient blokuje serwer? | `nc 127.0.0.1 3389` bez wysyłania, równolegle `xfreerdp`. Drugi wariant: odcięcie klienta w trakcie sesji (firewall). | P-02 |
| D-5 | Jakie `pointerCacheSize` i `MaxRequestSize` ogłaszają prawdziwi klienci? | Wireshark: Confirm Active od mstsc, Windows App i xfreerdp. | P-12, F-005.14 |
| D-6 | Czy Synchronize przychodzi po Cmd+Tab po stronie klienta? | Log `KeyEvent::SyncLocks` przy przełączaniu okien na kliencie. | P-04 |

Wyniki D-1…D-6 dopisujemy do tej sekcji z datą i zmieniamy oznaczenia w §4.

---

## 6. Co warto zrobić przed F-200 (w ramach istniejących fal)

**Decyzja właściciela (2026-09-17):** punkty 1–3 wchodzą do **Fali 3**, przed
F-005 i F-006 (`waves.md`). Specy: pkt 1 → **ADR-0013**, pkt 2 →
**F-003.22–F-003.26** i **F-003a.14–F-003a.18**, pkt 3 → **F-008** (wszystkie
Draft/Proposed, 2026-09-17). Kolejność według stosunku zysku do ryzyka. Każdy
punkt to zmiana speca albo ADR, potem kod.

1. **D-1 + nowy ADR** korygujący ADR-0006 (P-01). Tanie, a zmienia rozumienie
   wszystkich pomiarów z F-004.
2. **P-04 + P-05** — wymagania w F-003/F-003a: Synchronize puszcza wszystko
   (z prawdziwymi KeyUp/MouseUp); asembler surogatów; zdarzenia Unicode bez
   modyfikatorów. Czysta logika, testy jednostkowe.
3. **P-06** — honorowanie Suppress Output przez
   `RdpServer::display_suppressed_handle()`; przy wznowieniu reset bazy enkodera.
   Wymaga ADR, bo reset diffu IronRDP jest dostępny tylko przez `Resize`.
   Najpierw D-2.
4. **P-03** — naprawione na `master` upstreamu (ADR-0011). Rozwiązanie
   przychodzi z bazą forka albo z następnym wydaniem; do tego czasu D-3
   potwierdza, czy objaw występuje u prawdziwych klientów.
5. **P-02** — w F-108 / F-007.10: własna pętla akceptowania z timeoutem
   handshake'u i keepalive TCP, na bazie `RdpServer::run_connection` (istnieje
   w API 0.13.0 — ADR-0001). Pamiętać o `!Send` (`LocalSet`).
6. **P-10, P-16** — akumulator kółka z resztą; ruch względny z przyciskami.
7. **P-12** — przed approve F-005: rozmiar cache kursora ≤ najmniejszy
   `pointerCacheSize` z D-5.
8. **P-07, P-08, P-13, P-14** — poprawki w forku (ADR-0011, Etap 1–2);
   o zgłoszeniu upstream właściciel decyduje dla każdej osobno.

---

## 7. Plan F-200 — własna implementacja MS-RDPBCGR

> **Kierunek (2026-09-17):** na dziś silnikiem jest IronRDP. Pierwsza
> propozycja realizacji F-200 to **własny fork IronRDP rozwijany przez AI
> według speców** — ADR-0011 (Proposed). W tym wariancie etapy E0–E12 poniżej
> nie są modułami do napisania od zera, tylko **listą audytów zgodności**
> crate'ów forka: test według MS-RDPBCGR, a tam, gdzie pada, poprawka w forku.
> Kryteria akceptacji zostają te same. Kształt „sans-IO od zera” (§7.2) jest
> odłożony, nie skreślony.

### 7.1 Warunki wejścia

- Fale 0–3 w statusie **Verified** (`waves.md`, F-200).
- ADR-0011 w statusie Accepted (wariant fork) — albo, jeśli zostanie
  odrzucony, nowy ADR „architektura własnego stosu” rozstrzygający §7.2.
- Wolne numery ADR na dziś: **0014+** (0008 zarezerwowany dla F-003a,
  0010 dla F-005, 0012 dla F-101, 0013 zajęty przez ścieżkę obrazu).

### 7.2 Kształt wariantu „od zera” (odłożony — ADR-0011 §Odrzucone alternatywy)

1. **Moduł `src/rdp/` w stylu *sans-IO*:** czyste kodeki PDU i maszyny stanów,
   bez tokio, bez sieci, bez macOS. Wejście to bajty, wyjście to bajty
   i zdarzenia. Dzięki temu cała sekwencja połączenia jest testowalna
   jednostkowo — zgodnie z `CLAUDE.md` §Testy.
2. **`src/session/` pozostaje jedynym adapterem.** Test architektury
   (F-001.12) rozszerzamy o regułę: `crate::rdp` importuje tylko `session`,
   a `rdp` nie importuje `capture`, `input` ani `encoder`.
3. **Dwa silniki równolegle przez okres przejściowy:** `--engine ironrdp|own`,
   domyślnie `ironrdp`, dopóki lista zgodności z §7.5 nie jest zielona.
   Żadnego „big bang”.
4. **IronRDP jako wyrocznia testowa:** `ironrdp-pdu` zostaje w
   `dev-dependencies` do testów różnicowych (nasz encoder ↔ ich decoder
   i odwrotnie), a `ironrdp-connector`/`ironrdp-session` jako klient
   w testach integracyjnych — jak dziś w `tests/rdp_pixels.rs`.
5. **Fuzzing** każdego dekodera, który czyta bajty od klienta. To sieć
   z napastnikiem (F-108).

### 7.3 Zastrzeżenie o wektorach testowych z §4

Przykłady w 4.1 pochodzą z przechwycenia RDP 5.1 ze **Standard RDP Security**
(str. 333). Bajt w bajt porównujemy tylko PDU niezaszyfrowane (4.1.1–4.1.8)
i **odszyfrowane** ładunki z adnotacji (4.1.10+). Nagłówki TPKT/SDin/security
od 4.1.10 wzwyż będą u nas inne, bo nie mamy nagłówka RC4. Drugim źródłem
wektorów są przechwycenia prawdziwych klientów (D-1, D-5), zapisane jako
fixture'y w `tests/`.

### 7.4 Etapy i kamienie milowe

Proponowany podział: **jeden etap = jeden spec F-2xx**, a jeden kamień milowy
to jeden cel implementacyjny z jednym kryterium akceptacji. Numery F-201…F-212
są propozycją do nadania przy approve.

#### E0 → F-201 · Fundament kodeków

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K0.1 | `ReadCursor`/`WriteCursor`, typ błędu, trait `Pdu { encode, decode, size }` | `size()` == liczba bajtów z `encode` dla każdego PDU (test własności) | jednostkowy |
| K0.2 | Harness fuzz (`cargo fuzz`) + katalog fixture'ów z §4.1 | Harness kompiluje się w CI; 1 min fuzzowania bez paniki | CI |
| K0.3 | Kodery BER i PER (podzbiór z T.125 potrzebny MCS) | Round-trip + wektory z 4.1.3/4.1.4 | bajtowy |

#### E1 → F-202 · Transport i negocjacja (2.1, 2.2.1.1–2.2.1.2, 3.3.5.3.1–2)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K1.1 | TPKT + X.224 CR/CC/DT, walidacja długości (C-1) | CR z 4.1.1 dekoduje się; CC zgodne bajtowo z 4.1.2 (poza wybranym protokołem); CR < 11 B lub klasa ≠ 0 ⇒ błąd | bajtowy + fuzz |
| K1.2 | Negocjacja jako czysta funkcja: `NEG_REQ` + konfiguracja → `NEG_RSP` / `NEG_FAILURE` (C-2, C-3) | Tabela przypadków: brak negData, tylko `PROTOCOL_RDP`, SSL, HYBRID, HYBRID_EX, konfiguracja „wymagaj NLA” | jednostkowy |

#### E2 → F-203 · TLS, czasy i współbieżność (5.4.2.1, 5.4.5.1, 3.3.2.1)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K2.1 | Upgrade do TLS (rustls, TLS 1.2 + 1.3) po CC | Klient rustls kończy handshake; `SSLKEYLOGFILE` działa (ADR-0004) | integracyjny |
| K2.2 | Termin 60 s na całą sekwencję (C-13) + keepalive TCP | Milczące gniazdo zamknięte w 60 s ± 5 s; w tym czasie łączy się drugi klient | integracyjny |
| K2.3 | Polityka wielu połączeń (jedna sesja aktywna, co z drugą — do decyzji w specu) | Zachowanie zgodne z wybraną polityką; log audytowy | integracyjny |

#### E3 → F-204 · Basic Settings Exchange (2.2.1.3–2.2.1.4, 3.3.5.3.3–4)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K3.1 | MCS Connect Initial/Response + `MergeDomainParameters` (C-4) | Wynik scalenia z 4.1.3 == parametry z 4.1.4 (`34/3/0/1/0/1/65528/2`); przypadki negatywne kończą się błędem | bajtowy |
| K3.2 | Bloki klienta: core, security, network, cluster, monitor, monitorEx, msgchannel, multitransport + walidacja (C-6, C-7) | Bloki z 4.1.3 przechodzą round-trip; każda reguła walidacji ma test z wejściem odrzucanym | bajtowy + fuzz |
| K3.3 | Bloki serwera: `SC_CORE` (echo `clientRequestedProtocols`), `SC_SECURITY` (none), `SC_NET` z paddingiem, `SC_MCS_MSGCHANNEL` warunkowo (C-5, C-8) | Rozmiar NET jest wielokrotnością 4 dla parzystej i nieparzystej liczby kanałów; MSGCHANNEL tylko na żądanie | jednostkowy |

#### E4 → F-205 · Channel Connection (2.2.1.5–2.2.1.9, 3.3.5.3.5–9)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K4.1 | Erect Domain, Attach User, Channel Join, SDrq/SDin, DPUM | 4.1.5–4.1.8 bajt w bajt (Attach User Confirm `03 00 00 0b 02 f0 80 2e 00 00 06`) | bajtowy |
| K4.2 | Maszyna stanów łączenia kanałów (C-9) + skip-channel-join | Duplikat bez potwierdzenia; nieznany kanał ⇒ `result = 14`; żądania wysłane paczką; ścieżka `SKIP_CHANNELJOIN` | jednostkowy |

#### E5 → F-206 · Client Info i licencjonowanie (2.2.1.11–2.2.1.12, 3.3.5.3.11–12)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K5.1 | Client Info + `TS_EXTENDED_INFO`; kontrola anti-downgrade (C-5, C-10) | Odszyfrowany ładunek z 4.1.10 dekoduje się; brak `SEC_INFO_PKT` ⇒ rozłączenie; ANSI i Unicode; `serverSelectedProtocol` ≠ wybranego ⇒ rozłączenie | bajtowy |
| K5.2 | License Error — Valid Client | Ładunek == `ff 03 10 00 07 00 00 00 02 00 00 00 04 00 00 00` (4.1.11) | bajtowy |

#### E6 → F-207 · Capabilities i finalizacja (2.2.1.13–2.2.1.22, 2.2.7, 3.3.5.3.13–22)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K6.1 | Nagłówki Share Control/Data; Demand/Confirm Active z capability sets jako nieprzezroczystym TLV | Nasz Demand Active dla tych samych capabilities == odszyfrowany 4.1.12 (`pduSource = 1002`, shareId, `"RDP\0"`); 4.1.13 dekoduje się; zły `pduType` ⇒ rozłączenie | bajtowy |
| K6.2 | Kodery/dekodery capability sets: General, Bitmap, Order, Pointer, Input, VirtualChannel, MultifragmentUpdate, SurfaceCommands, LargePointer, BitmapCodecs (G-7, G-10, I-8) | Round-trip na 4.1.12/4.1.13 i na przechwyceniach z D-5 | bajtowy |
| K6.3 | Model `ClientCaps` z egzekwowaniem: fast-path wymagany; `MaxRequestSize`, `pointerCacheSize`, large pointer, `cmdFlags` i flagi rozszerzeń trafiają do warstwy wyjścia | Klient bez fast-path output ⇒ odmowa z komunikatem; limity widoczne w strukturze przekazanej enkoderowi | jednostkowy |
| K6.4 | Maszyna stanów finalizacji zgodna z C-12; Persistent Key List przyjmowany i przechowywany | 4.1.14–4.1.21 bajt w bajt (ładunki); Font Map dopiero po Font List; input przed Font List trafia do inputu | bajtowy + jednostkowy |

#### E7 → F-208 · Input (2.2.8, 3.3.5.8)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K7.1 | Dekoder input slow-path i fast-path → **nasze** `PointerEvent`/`KeyEvent` | Tabela: każda kombinacja flag z 2.2.8.1.x — BUTTON3, PTRX (I-4), HWHEEL, EXTENDED1 (I-6), 9-bitowe kółko ± (I-2), relative, unicode — dekoduje się zgodnie ze specyfikacją; nieznany typ ⇒ błąd (I-7). Wektor z 4.7 | bajtowy |
| K7.2 | Synchronize jako jawne zdarzenie „zresetuj stan” (I-1) | Zdarzenie niesie stan locków i znacznik resetu; sink go honoruje (zależność: P-04) | jednostkowy |

#### E8 → F-209 · Wyjście graficzne (2.2.9, 2.2.11, 3.3.5.9, 3.3.5.11)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K8.1 | Typy `InclusiveRect` ↔ `ExclusiveRect` z przycinaniem do pulpitu (G-1) | Test własności: `right + 1`, obcięcie do pulpitu, pusty prostokąt odrzucony | jednostkowy |
| K8.2 | Fragmenter fast-path z limitem `min(16 383, MaxRequestSize)` (G-4, G-5) | Suma fragmentów ≤ limit; flagi F/N/L; przypadek 1 B; przypadek dokładnie na granicy | jednostkowy + bajtowy |
| K8.3 | Set Surface Bits (`codecID = 0`) + Frame Marker (G-8, G-9); arytmetyka `y * stride + x * bpp` (ADR-0005) | Dekodowanie klientem `ironrdp-session` daje identyczne piksele; BEGIN/END sparowane; stride ≠ `width*4` | integracyjny (pikselowy) |
| K8.4 | Refresh Rect → odmalowanie przecięcia z pulpitem; Suppress Output → wstrzymanie i wznowienie z pełną klatką (G-2, G-3) | Po Refresh Rect wysłany dokładnie obszar przecięcia; po Suppress(0) `bytes_sent` nie rośnie; po Suppress(1) pełne odświeżenie | jednostkowy + klient |
| K8.5 | Wskaźniki: New, Cached, Hidden, Default, Position (G-6) — kontrakt z F-005 | `cacheIndex` < `pointerCacheSize` klienta; > 32 px tylko z flagą large pointer; brak New Pointer przy cache 0 | jednostkowy + bajtowy |

#### E9 → F-210 · Cykl życia sesji (2.2.2–2.2.3, 2.2.5, 2.2.16, 3.3.5.4–7)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K9.1 | Deactivate All → ponowny Demand Active (resize, F-006) warunkowo od `desktopResizeFlag` (L-3) | Test funkcji decyzji; ręcznie: przełączenie ekranu bez zerwania sesji | jednostkowy + klient |
| K9.2 | Shutdown Request → Shutdown Denied; DPUM w obie strony (L-1, L-2) | Zamknięcie w xfreerdp: w logu `SHUTDOWN_DENIED (37)`, klient kończy bez kodu błędu | integracyjny + klient |
| K9.3 | Set Error Info bramkowane flagą klienta (L-4): `SERVER_SHUTDOWN` przy SIGTERM, `INSUFFICIENT_PRIVILEGES` przy odmowie | Test bramki; log xfreerdp pokazuje `ERRINFO_SERVER_SHUTDOWN` | jednostkowy + klient |
| K9.4 | Heartbeat na kanale komunikatów (L-6) | Wysyłany tylko przy fladze klienta i tylko w ciszy; brak PDU bez flagi | jednostkowy |

#### E10 → F-211 · Monitory (2.2.1.3.6, 2.2.12)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K10.1 | Monitor Layout PDU z naszego modelu monitorów (C-7, C-11) | Układ z 2 monitorów daje oczekiwane bajty; 1920×1080 ⇒ `right = 1919`; `pduSource = 0`; brak PDU bez flagi `0x40` | bajtowy |

#### E11 → F-212 · NLA (5.4.5.2, 2.2.10.2; MS-CSSP)

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K11.1 | CredSSP/NTLM — własny albo przez `sspi` (**osobny ADR**; ograniczenie z F-101: hasło odwracalne) | `xfreerdp /sec:nla` łączy się | integracyjny |
| K11.2 | HYBRID_EX + Early User Authorization Result (C-14) | Złe hasło ⇒ `05 00 00 00`, potem alert TLS; HYBRID bez EX ⇒ Set Error Info 9 po Channel Join | integracyjny |

#### E12 · Przełączenie silnika

| Kamień | Zakres | Kryterium akceptacji | Test |
|---|---|---|---|
| K12.1 | `--engine own` przechodzi całą listę z §7.5 | Wszystkie punkty zielone | ręczny (właściciel) |
| K12.2 | Domyślny silnik = `own`; ADR zastępujący ADR-0001 | IronRDP tylko w `dev-dependencies` | CI |

#### Później, poza pierwszym przejściem F-200

- MPPC RDP5 (3.1.8) — `ironrdp-bulk` 0.1.1 jako wyrocznia.
- Fallback RDP 6.0 dla klientów bez Surface Commands (P-19).
- Auto-detect RTT (2.2.14), auto-reconnect (5.5) po F-101.
- **Poza MS-RDPBCGR, a dziś dostarczane przez IronRDP.** Przełączenie silnika
  wymaga planu także dla: SVC/DVC (MS-RDPEDYC — warunek MS-RDPEDISP, F-006),
  clipboardu (MS-RDPECLIP, F-102), dźwięku (MS-RDPEA, F-103), EGFX/RemoteFX
  (F-104). **Decyzja do ADR:** czy F-200 zastępuje tylko rdzeń BCGR i dalej
  korzysta z tych cratów IronRDP, czy zastępuje wszystko.

### 7.5 Lista zgodności przed przełączeniem silnika (K12.1)

Wykonuje właściciel, każdym z klientów: **mstsc (Windows), Windows App (macOS),
xfreerdp (Linux)**.

- [ ] połączenie TLS; odmowa klienta żądającego tylko Standard RDP Security,
- [ ] połączenie NLA; złe hasło daje czytelną odmowę,
- [ ] żywy obraz; kolory i geometria jak w F-002,
- [ ] damage: statyczny pulpit ≈ 0 B/s (F-004 AC-2),
- [ ] minimalizacja i przywrócenie okna klienta: obraz poprawny (K8.4),
- [ ] kursor lokalny (F-005),
- [ ] mysz: 3 przyciski, przyciski boczne, kółko pionowe i poziome, przeciąganie,
- [ ] klawiatura: modyfikatory, Cmd+Tab po stronie klienta bez zablokowanych klawiszy,
- [ ] zmiana rozmiaru okna klienta (F-006),
- [ ] zamknięcie przez klienta i przez serwer: poprawny komunikat u klienta,
- [ ] 3 cykle połącz–rozłącz; milczące gniazdo nie blokuje serwera,
- [ ] sesja ≥ 30 min bez wycieku pamięci.

### 7.6 Zależności między etapami

```
E0 ─┬─ E1 ── E2 ── E3 ── E4 ── E5 ── E6 ─┬─ E7
    │                                     ├─ E8
    │                                     ├─ E9 ── E10
    │                                     └─ E11 (równolegle z E7–E10)
    └─ fuzz/fixtures dla wszystkich
E7 + E8 + E9 + E10 + E11 ── E12
```

E0–E6 da się w całości zrobić i przetestować **bez żadnego klienta i bez macOS**.
Pierwszy obraz z własnego silnika pojawia się dopiero w E8 — dokładnie ten koszt,
który ADR-0001 opisał jako powód odłożenia F-200. Etapy E1–E6 warto więc zamykać
szybko, bo nie dają widocznego efektu.

---

## 8. Decyzje właściciela (2026-09-17)

| # | Pytanie | Decyzja |
|---|---|---|
| 1 | Osobny plik czy rozbicie na `waves.md` + F-200? | **Osobny plik** (ten dokument). |
| 2 | Czy §6 pkt 1–3 wchodzą do Fali 3 przed F-005/F-006? | **Tak** — `waves.md` zaktualizowany. |
| 3 | Zakres F-200? | **Na razie IronRDP**; przygotować pierwszą propozycję, ulepszanie później → ADR-0011 (Proposed). |
| 4 | PR-y do upstreamu? | Właściciel rozważa **fork IronRDP i własne podejście, w pełni prowadzone przez AI** → ADR-0011. O PR-ach upstream decyduje osobno dla każdej zmiany. |
| 5 | PDF w repo? | **PDF w `.gitignore`, wersja przetworzona w repo** → `specs/ref/`. |

### Otwarte

- Pytania z ADR-0011 §„Otwarte pytania” (konto forka, baza, moment startu).
- Wyniki pomiarów D-1…D-6 (§5).

## 9. Status

**Draft** (2026-09-17); decyzje kierunkowe w §8. Źródło: przegląd MS-RDPBCGR v20260309 wobec
`ironrdp-server` 0.13.0, `ironrdp-acceptor` 0.10.0, `ironrdp-pdu` 0.9.0
i kodu na commicie `f3beacb`.
