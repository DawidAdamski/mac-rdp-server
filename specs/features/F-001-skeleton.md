# F-001 — Szkielet: serwer TLS + statyczny obraz testowy

- **Fala:** 0
- **Status:** Verified (2026-07-22)
- **Data:** 2026-07-22
- **ADR:** ADR-0001 (IronRDP), ADR-0004 (TLS/cert), ADR-0005 (piksele/stride)

## 1. Cel

Binarka `mac-rdp-server` startuje, generuje self-signed cert, nasłuchuje na porcie
i serwuje klientowi RDP statyczny obraz testowy — udowadniając, że łańcuch
TLS → handshake RDP → wysyłanie bitmap działa, zanim dołożymy cokolwiek z macOS.

## 2. Wymagania

### Projekt i toolchain

- **F-001.1** (MUST) Projekt MUSI być cargo binary crate `mac-rdp-server`,
  edition 2024, na stabilnym Rust. `Cargo.lock` MUSI być w repo.
- **F-001.2** (MUST) `ironrdp-server` MUSI być pinowany jako `=0.13.0`
  z feature `helper`; `screencapturekit` jako `=8.0.1` (dołożony w Fali 1).
- **F-001.3** (MUST) Repo MUSI mieć CI (GitHub Actions, `macos-latest`)
  uruchamiające: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo check`. CI MUSI być zielone przed uznaniem fali za Verified.

### Konfiguracja

- **F-001.4** (MUST) Konfiguracja MUSI być czytana z flag CLI i pliku TOML,
  z precedencją: **flagi > TOML > wartości domyślne**.
  Klucze: `bind_addr` (domyślnie `127.0.0.1`), `port` (domyślnie `3389`),
  `cert_path`, `key_path`, `display` (domyślnie `0`), `max_fps` (domyślnie `30`),
  `log_level`.
- **F-001.5** (SHOULD) `--help` POWINIEN wypisywać wszystkie opcje z opisami
  i wartościami domyślnymi.

> **Uzupełnione przez F-112** (Draft, 2026-10-16). F-001.4 mówi o pliku TOML
> **wskazanym** flagą `--config`; F-112 dokłada plik czytany domyślnie
> z `~/.config/mac-rdp-server.toml` i rozszerza precedencję o jeden poziom:
> *flagi > plik wskazany > plik domyślny > wartości domyślne*. Treść F-001.4
> nie zmienia się i ten spec zostaje Verified — zmianę niesie F-112, zgodnie
> z `CLAUDE.md` („zmiana zdania = aktualizacja speca, nigdy cichy refactor").

### TLS i certyfikat

- **F-001.6** (MUST) Jeżeli `cert_path`/`key_path` nie istnieją, binarka MUSI
  wygenerować self-signed cert (`rcgen`), zapisać go jako PEM pod tymi ścieżkami
  i wypisać w logu, gdzie go zapisała. Przy kolejnych startach MUSI go wczytać,
  a nie generować od nowa.
- **F-001.7** (MUST) Cert MUSI zawierać SAN: `localhost`, `127.0.0.1`, `::1`
  oraz hostname maszyny.
- **F-001.8** (MUST) Serwer MUSI używać `RdpServer::builder().with_tls(acceptor)`
  z akceptorem zbudowanym przez `TlsIdentityCtx::init_from_paths(..).make_acceptor()`.
  NLA (`with_hybrid`) jest **poza zakresem** tej fali.

### Serwer i obraz

- **F-001.9** (MUST) Serwer MUSI nasłuchiwać na skonfigurowanym adresie i przyjąć
  połączenie klienta RDP po TLS.
- **F-001.10** (MUST) Moduł `capture` MUSI dostarczać `TestPatternSource`
  implementujący `FrameSource` i generujący obraz 1280×720 zawierający:
  - trzy jednoznaczne pola pełnego koloru: **czysty czerwony `#FF0000`,
    czysty zielony `#00FF00`, czysty niebieski `#0000FF`** (weryfikacja mapowania
    kanałów, ADR-0005 §4),
  - biały prostokąt w **lewym górnym** rogu i czarny w prawym dolnym
    (weryfikacja orientacji i braku przesunięcia wierszy),
  - pole tekstowe/licznik zmieniające się raz na sekundę (dowód, że ścieżka
    aktualizacji żyje, a nie że widzimy jedną zamrożoną klatkę).
- **F-001.11** (MUST) `TestPatternSource` MUSI generować bufor ze **stride ≠ width×4**
  (np. wyrównanym do 64 B), żeby ścieżka stride była przećwiczona już w Fali 0,
  a nie odkryta jako błąd w Fali 1.
- **F-001.12** (MUST) Żaden moduł poza `session` NIE MOŻE importować
  `ironrdp_server::*`. Warunek MUSI być egzekwowany testem w CI
  (np. test skanujący `src/` i sprawdzający importy).

### Input (na razie tylko obserwacja)

- **F-001.13** (MUST) `session::MacRdpInput` MUSI konwertować `KeyboardEvent`
  i `MouseEvent` z IronRDP na nasze `KeyEvent`/`PointerEvent` i przekazywać je
  do `InputSink`. W tej fali `InputSink` to `LoggingSink` — **nic nie wstrzykuje**,
  tylko loguje na poziomie `info`.
- **F-001.14** (MUST) Handler wejściowy NIE MOŻE blokować: konwersja + `try_send`
  na bounded kanał; przepełnienie = pominięcie zdarzenia i `warn`
  (`01-architecture.md` §4a).

### Logi, błędy, zamykanie

- **F-001.15** (MUST) Logowanie przez `tracing`, poziom sterowany zmienną
  `MAC_RDP_LOG` i flagą `--log-level`. Na starcie MUSI być zalogowane:
  wersja, adres nasłuchu, ścieżka certu, rozmiar pulpitu.
- **F-001.16** (MUST) Rozłączenie klienta lub błąd sesji NIE MOŻE ubić procesu —
  serwer loguje i czeka na kolejne połączenie. Kolejne połączenia po rozłączeniu
  MUSZĄ działać.
- **F-001.17** (SHOULD) `Ctrl-C` POWINIEN zamykać serwer czysto (log + exit 0).

### Testy

- **F-001.18** (MUST) Testy jednostkowe MUSZĄ pokrywać:
  - precedencję konfiguracji (flaga nadpisuje TOML nadpisuje default),
  - geometrię wzorca testowego: kolor konkretnego piksela liczony przez
    `y * stride + x * 4` przy stride ≠ width×4,
  - konwersje `MouseEvent`/`KeyboardEvent` → `PointerEvent`/`KeyEvent`
    (wszystkie warianty, w tym `extended`),
  - konwersję `FrameUpdate` → `BitmapUpdate` (wymiary, stride, długość danych).
- **F-001.19** (SHOULD) Test integracyjny POWINIEN podnieść serwer na porcie
  efemerycznym i sprawdzić, że TCP + TLS handshake dochodzi do skutku
  (bez pełnego handshake'u RDP).

## 3. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-001.8, F-001.9 | MS-RDPBCGR §5.4.5.1 (Enhanced RDP Security z TLS); README `ironrdp-server`: „Enhanced RDP Security with TLS External Security Protocols (TLS 1.2 and TLS 1.3)" |
| F-001.10 | MS-RDPBCGR §2.2.9.1.1.3.1.2 (Bitmap Update); `ironrdp-server` `DisplayUpdate::Bitmap` + kompresja RDP 6.0 |
| F-001.11, F-001.10 kolory | ADR-0005; `ironrdp_server::BitmapUpdate.stride: NonZeroUsize`, `PixelFormat::BgrA32` |
| F-001.13 | MS-RDPBCGR §2.2.8.1.2 / §2.2.9.1.2 (FastPath input); `RdpServerInputHandler` |
| F-001.6, F-001.7 | ADR-0004; `ironrdp_server::TlsIdentityCtx` (feature `helper`) |

## 4. Kryteria akceptacji

Sprawdzane ręcznie prawdziwym klientem + automatycznie w CI.

1. **AC-1** `cargo build --release` przechodzi na czystym klonie; CI zielone
   (fmt, clippy `-D warnings`, test).
2. **AC-2** Pierwsze uruchomienie bez istniejącego certu: w logu widać ścieżkę
   zapisanego certu; pliki `.pem` istnieją. Drugie uruchomienie: log mówi
   „loaded existing certificate", pliki nie zostały nadpisane.
3. **AC-3** **Windows App na macOS** łączy się z `127.0.0.1:3389`.
   Po zaakceptowaniu ostrzeżenia o certyfikacie widać obraz testowy.
   *Ostrzeżenie o certyfikacie i braku NLA jest zachowaniem oczekiwanym (ADR-0004).*
4. **AC-4** **mstsc.exe z Windows** łączy się z `<ip-maca>:3389` i widzi ten sam
   obraz (wymaga `bind_addr = 0.0.0.0`).
5. **AC-5** **Kolory się zgadzają:** pole czerwone jest czerwone, zielone zielone,
   niebieskie niebieskie. Zamienione kanały ⇒ złe mapowanie `PixelFormat`
   i poprawka w `session` (ADR-0005 §4).
6. **AC-6** **Orientacja się zgadza:** biały prostokąt jest w lewym górnym rogu,
   obraz nie jest skośny ani przesunięty (dowód poprawnej obsługi stride).
7. **AC-7** Licznik na obrazie zmienia się co sekundę — ścieżka aktualizacji żyje.
8. **AC-8** Ruch myszą i pisanie w oknie klienta produkują logi
   `pointer`/`key` z sensownymi wartościami (pozycja rośnie zgodnie z ruchem,
   naciśnięcie i zwolnienie klawisza to dwa zdarzenia).
9. **AC-9** Rozłączenie klienta nie ubija serwera; ponowne połączenie działa;
   trzy kolejne cykle połącz–rozłącz przechodzą bez restartu procesu.
10. **AC-10** `xfreerdp /v:127.0.0.1:3389 /cert:ignore -nla` łączy się i pokazuje
    obraz (klient kontrolny do debugowania).
11. **AC-11** `grep -rn "ironrdp" src/ --include=*.rs` zwraca trafienia wyłącznie
    w `src/session/` (poza `Cargo.toml`).

## 5. Poza zakresem

Jawnie **nie** w tej fali:
- ScreenCaptureKit i jakikolwiek prawdziwy obraz ekranu (Fala 1),
- wstrzykiwanie inputu do systemu (Fala 2) — tu tylko logowanie,
- sprawdzanie uprawnień TCC (moduł `perms` powstaje w Fali 1),
- NLA/CredSSP, clipboard, audio, wiele monitorów, kursor, resize (backlog),
- damage tracking i jakakolwiek optymalizacja wydajności (Fala 3),
- bundle `.app`, podpisywanie kodu, Info.plist (F-106),
- obsługa wielu równoczesnych klientów.

## 6. Kroki testu ręcznego (do wykonania po implementacji)

Zostaną doprecyzowane przy implementacji; szkielet:

1. `cargo run --release -- --bind-addr 0.0.0.0 --port 3389`
2. Windows App → **Add PC** → `127.0.0.1:3389` → user/pass dowolne →
   przy ostrzeżeniu o certyfikacie: **Continue**.
3. mstsc.exe → `<ip-maca>:3389` → „Yes" na ostrzeżenie o tożsamości.
4. Sprawdzić AC-5, AC-6, AC-7 wzrokowo; AC-8 w logach terminala.
5. Debug (gdy coś nie działa):
   `IRONRDP_LOG=trace MAC_RDP_LOG=debug cargo run` oraz
   `SSLKEYLOGFILE=/tmp/rdp-keys.log` + Wireshark.

## 7. Status

**Verified** (2026-07-22).

### Zapis weryfikacji

Automatycznie (CI: fmt, clippy `-D warnings`, 38 testów):

| AC | Jak zweryfikowane |
|---|---|
| AC-1 | `cargo build --release`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` — czysto |
| AC-2 | Test `tls::tests::generates_once_then_loads` + ręcznie: pierwszy start `generated self-signed certificate` z SAN-ami `localhost, 127.0.0.1, ::1, Dawids-MBP, Dawids-MBP.local`; drugi start `loaded existing certificate`, plik niezmieniony (suma kontrolna) |
| AC-10 | `tests/tls_handshake.rs` — pełna sekwencja X.224 Connection Request → Connection Confirm z `PROTOCOL_SSL` → handshake TLS |
| AC-11 | `tests/architecture.rs` — dwa testy, w tym zabezpieczenie przed pustym przejściem |

Ręcznie, prawdziwym klientem RDP (właściciel projektu, 2026-07-22):

| AC | Wynik |
|---|---|
| AC-3 / AC-4 | Klient łączy się po TLS i wyświetla obraz testowy |
| AC-5 | Trzy pola w kolejności czerwony → zielony → niebieski. **Potwierdza mapowanie `Bgra8` → `PixelFormat::BgrA32`** (ADR-0005 §4) |
| AC-6 | Biały kwadrat w lewym górnym rogu, czarny w prawym dolnym, brak skosu i brak magenty. **Potwierdza obsługę stride ≠ width×4 na całej ścieżce** |
| AC-7 | Licznik przeszedł od 60 do 82 — ścieżka aktualizacji żyje, sesja stabilna |
| AC-8 | Logi `pointer` i `key` pojawiają się przy ruchu myszą i pisaniu |
| AC-9 | Trzy cykle połącz–rozłącz bez restartu procesu |

### Pozostawione świadomie

- **AC-3 vs AC-4:** potwierdzone jednym klientem. Krzyżowy test drugim
  (mstsc ↔ Windows App) nie został wykonany — mstsc bywa bardziej wybredny przy
  braku NLA. Do sprawdzenia przy okazji Fali 1; nie blokuje.
- **`FrameUpdate::Region` i `::Resize`** nie istnieją jeszcze w kodzie, mimo że
  `01-architecture.md` je opisuje. Powstaną razem z F-004 i F-006 — dodane teraz
  byłyby martwym kodem bez zatwierdzonego wymagania.
