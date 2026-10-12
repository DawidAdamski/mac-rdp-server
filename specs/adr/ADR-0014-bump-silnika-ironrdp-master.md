# ADR-0014 — Jak dojść do działającego środkowego przycisku myszy

- **Status:** Accepted (2026-10-12, właściciel)
- **Dotyczy:** ADR-0007 (środkowy przycisk — ograniczenie IronRDP), ADR-0001
  (wybór silnika), ADR-0011 (fork IronRDP), F-003.7, F-008 §6
- **Decyzja właściciela:** **approve 2026-10-12** — fork (Etap 1 z ADR-0011).
  Właściciel zlecił też rozstrzygnięcie zagadki `split_diff` z sekcji niżej.

## Kontekst

ADR-0007 zapisał, że środkowy przycisk myszy nie dociera do nas, bo
`ironrdp-server` 0.13.0 gubi `PTRFLAGS_BUTTON3` w `From<MousePdu>`: traktuje go
jako ruch. Nasza strona jest gotowa od F-003.7 (`other_button_number` ustawia
`MOUSE_EVENT_BUTTON_NUMBER = 2`), więc brakuje wyłącznie zdarzenia z silnika.

### Zbadany stan faktyczny (2026-10-12)

**Wydania.** Najnowsze opublikowane `ironrdp-server` to nadal **0.13.0
(2026-07-10)** — tak samo `ironrdp-pdu` 0.9.0, `ironrdp-acceptor` 0.10.0,
`ironrdp-session` 0.11.0, `ironrdp-connector` 0.10.0, `ironrdp-rdpsnd` 0.9.0.
Przez trzy miesiące nie wyszło nic nowego, choć `master` żyje.

**Master (HEAD `e4143f2`, 2026-10-10) naprawia środkowy przycisk.**
`crates/ironrdp-server/src/handler.rs`:
- `PointerFlags::MIDDLE_BUTTON_OR_WHEEL` → `MouseEvent::Button { button: MouseButton::Middle, .. }`;
- API myszy przebudowane: zamiast `LeftPressed`/`MiddleReleased`/…
  jest `Button { x, y, button, pressed }`, `ButtonRel { x, y, button, pressed }`
  (ruch względny, MS-RDPBCGR 2.2.8.1.1.3.1.1.7), `VerticalScroll`,
  `HorizontalScroll`, `Scroll`, `RelMove`;
- `MouseButton` to `Left | Right | Middle | X1 | X2`, `#[non_exhaustive]`;
- zdarzenie przycisku **nosi pozycję**, czego 0.13.0 nie robiło.

**Master NIE naprawia błędu, który rozłączał nas przy wznowieniu.**
`ironrdp-acceptor/src/connection.rs:1437-1439` — nadal `expected client confirm
active` dla PDU, które zdekodowało się poprawnie, a nie jest Confirm Active
(F-008 §6). Powód forku z ADR-0011 zostaje.

**Co kosztuje przejście na master — zmierzone, nie oszacowane.** Próba
w worktree (`[patch.crates-io]` na wszystkie craty `ironrdp-*` z rewizji
`e4143f2`) doprowadzona do kompilacji i zielonego clippy:

| Miejsce | Zmiana |
|---|---|
| `src/session/convert.rs` | nowe warianty `MouseEvent`; mapowanie `MouseButton::{X1,X2}` → nasze `Back`/`Forward`; `_ => return None` dla `#[non_exhaustive]` |
| `src/session/display.rs` | `updates()` i `next_update()` zwracają `ServerResult`, nie `anyhow::Result` — potrzebny przelicznik naszych błędów (`ServerError::reason`, bo `anyhow::Error` nie implementuje `core::error::Error`) |
| `src/session/input.rs` | `ConnectionHandler::on_disconnected` dostaje `Option<&ServerError>`; dochodzi domyślna metoda `on_connection_info` |
| `src/session/mod.rs` | `RdpServer::run()` zwraca `ServerResult<()>` |
| `tests/tls_handshake.rs` | `ironrdp_server::tokio_rustls` nie jest już re-eksportowany — import z własnej dev-dependency |
| `tests/rdp_pixels.rs`, `tests/suppress_output.rs`, `examples/probe_client.rs` | `ironrdp_connector::Config` ma 7 nowych pól (`monitor_layout`, `enable_standard_rdp_security`, `connection_type`, `remote_application_mode`, `rail_support_level`, `enable_audio_capture`, `support_dyn_vc_gfx_protocol`); `KeyboardType` to newtype — `IbmEnhanced` → `IBM_ENHANCED` |

Granica architektoniczna się trzyma: **cała zmiana w `src/` dotyczy wyłącznie
`src/session/`**. `rust-version` mastera to 1.94, mamy 1.98.1.

**Wynik testów na masterze:** 153 testy jednostkowe, `architecture`,
`suppress_output` i `tls_handshake` — zielone. **`rdp_pixels`
`damage_updates_land_in_the_right_place_and_disturb_nothing_else` (F-004.20)
pada.** Obraz u klienta nigdy nie zgadza się dokładnie z klatką wzorca;
pierwsza aktualizacja przychodzi jako `(0, 0, 639, 478)`, czyli o **jeden
wiersz za mało** przy pulpicie 640×480, a potem pięć razy ten sam prostokąt
`(256, 256, 383, 383)`. Na 0.13.0 ten test jest zielony.

### Śledztwo w sprawie brakującego wiersza (2026-10-12, zlecone przez właściciela)

**Pomiar porównawczy.** Ten sam test, ta sama geometria (640×480, stride
2624 B = 2560 + 64 B wyrównania), dwa drzewa roboty:

| Wersja | Pierwsza aktualizacja (prostokąt inkluzywny) | Wiersze | Test |
|---|---|---|---|
| 0.13.0 (gałąź `probe/baseline-013`) | `(0, 0, 639, 479)` | 480 | zielony |
| master `e4143f2` (gałąź `probe/ironrdp-master`) | `(0, 0, 639, 478)` | **479** | **czerwony** |

Pozostałe sześć prostokątów (regiony licznika) jest w obu przebiegach
identycznych. Ginie więc **ostatni wiersz pierwszej pełnej klatki** i zostaje
nieodświeżony na zawsze, bo nasz enkoder uważa, że klient go ma.

**To nie jest `split_diff`.** Domyślny `max_request_size` to **8 MB**
(`server.rs:692`), a pełna klatka 640×480×4 to 1,2 MB, więc warunek
`rect.width * rect.height * 4 <= max_request_size` jest spełniony i do
dzielenia w ogóle nie dochodzi. Pierwsza hipoteza z pierwotnej wersji tego
ADR-a była błędna.

**Czego to też nie jest.** Porównanie plik po pliku: `encoder/bitmap.rs`
(dzielenie na kawałki po `u16::MAX / row_len` wierszy i `chunk.len() / stride`),
`BitmapUpdate::sub()` (koniec wycinka liczony jako
`(height − 1) × stride + width × bpp`, czyli krótszy o wyrównanie ostatniego
wiersza), `NoneHandler` (Surface Bits) oraz rdzeń `bitmap_diffs` są
**bajtowo identyczne** w 0.13.0 i na masterze. Wyścig jest więc w kodzie,
który się zmienił: `ironrdp-graphics` (`rdp6/bitmap_stream/encoder.rs`, `rle.rs`)
albo w kliencie `ironrdp-session` (`image.rs`, `fast_path.rs`), albo w warstwie
`ironrdp-pdu`.

**Czego nie udało się zrobić.** Rozdzielenie stron (serwer z mastera, klient
testowy z crates.io) **nie kompiluje się**: `ironrdp-pdu` z mastera jest
niezgodny z `ironrdp-graphics` 0.9.0 i `ironrdp-connector` 0.10.0
(`not_enough_bytes_err` ma inną arność, `ConnectionRequest` ma nowe pole).
Bez instrumentacji lokalnego klona nie da się powiedzieć, czy wiersz gubi
koder, czy dekoder.

**Wniosek dla decyzji.** Dla ścieżki z forkiem to jest bez znaczenia: fork
wychodzi z **taga 0.13.0**, gdzie test jest zielony, a przenosimy wyłącznie
mapowanie przycisku w `handler.rs` — nie dotykamy kodera ani grafiki. Błąd
zostaje po stronie upstreamu i jest **czwartym** argumentem, żeby mastera nie
brać. Zgłaszamy go issue z gotowym repro (pulpit 640×480, stride
z wyrównaniem, ścieżka Surface Bits / Bitmap bez kodeków) przy okazji PR-ów
z forku. F-004.20 to jedyny nasz test, który łapie piksele na drucie
(ADR-0006), więc jego czerwień jest wiążąca.

## Decyzja (propozycja)

**Nie pinujemy mastera.** Zamiast tego **fork z ADR-0011 wchodzi w Etap 1
teraz**, gałęzią od taga `0.13.0`, z trzema patchami w tej kolejności:

1. **Środkowy przycisk** — port poprawki z mastera, ale wyłącznie w zakresie
   `From<MousePdu>`: bez przebudowy całego API myszy. Dla nas znika wtedy
   migracja `convert.rs`, a ADR-0007 można zamknąć.
2. **Tolerancja PDU przy reaktywacji** (`connection.rs`) — zdejmuje odstępstwo
   zapisane w F-008.7v2 i pozwala wrócić do pełnego odświeżenia zgodnego
   z Appendix A <38>.
3. (Opcjonalnie, później) to, co z mastera okaże się potrzebne — każdorazowo
   z testem po naszej stronie.

Oba patche idą też jako PR-y do upstreamu; jeśli zostaną wydane, fork znika.

## Konsekwencje

- `Cargo.toml` przestaje wskazywać crates.io dla `ironrdp-*` i wskazuje nasz
  fork z **dokładną rewizją** (`rev = "…"`), nie gałęzią. Zasada pinowania
  z CLAUDE.md zostaje zachowana, tylko źródłem jest fork.
- ADR-0007 dostaje status Superseded z linkiem tutaj, gdy patch 1 wejdzie.
- Dochodzi obowiązek utrzymania: rebase forku przy każdym wydaniu upstreamu.
  To jest koszt, który ADR-0011 i tak przewidywał.
- F-004.20 zostaje zielone, bo nie bierzemy nowego kodu dzielenia prostokątów.
- Zagadka z `split_diff` zostaje **otwarta i zapisana**: przed jakimkolwiek
  późniejszym podejściem do mastera trzeba ją rozstrzygnąć i, jeśli to
  regresja upstreamu, zgłosić.

## Odrzucone alternatywy

1. **Pinowanie `rev = e4143f2` (master) teraz.** Odrzucone: pada F-004.20,
   a przyczyna nieznana. Migracja jest wykonalna (zmierzona wyżej) i można do
   niej wrócić po rozstrzygnięciu.
2. **Czekanie na wydanie upstreamu.** Odrzucone: trzy miesiące bez wydania,
   a środkowy przycisk i rozłączenia przy wznowieniu bolą dziś.
3. **Obejście po naszej stronie** (np. wnioskowanie środkowego przycisku
   z ruchu bez flag). Odrzucone wprost: zgadywanie intencji klienta
   w miejscu, gdzie protokół ma jawną flagę.
4. **Nic nie robić, zostawić ADR-0007 jako znany brak.** Odrzucone decyzją
   właściciela 2026-10-12 (środkowy przycisk na liście braków).
