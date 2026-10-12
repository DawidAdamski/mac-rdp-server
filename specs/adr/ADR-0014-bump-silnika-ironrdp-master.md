# ADR-0014 — Jak dojść do działającego środkowego przycisku myszy

- **Status:** Proposed (2026-10-12)
- **Dotyczy:** ADR-0007 (środkowy przycisk — ograniczenie IronRDP), ADR-0001
  (wybór silnika), ADR-0011 (fork IronRDP), F-003.7, F-008 §6
- **Decyzja właściciela:** oczekuje

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

Master ma w tym miejscu nowy kod: `encoder/mod.rs:333-410` dzieli prostokąty
różnic pod limit `max_request_size` (`split_diff`, „splits by height first,
then by width"), czego 0.13.0 nie robiło. **Przyczyna nierozstrzygnięta** —
albo regresja upstreamu w tym dzieleniu, albo zmiana kontraktu
`BitmapUpdate`, której nie dopełniamy. Bez rozstrzygnięcia nie wolno tego
wziąć: F-004.20 to jedyny test, który łapie piksele na drucie (ADR-0006).

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
