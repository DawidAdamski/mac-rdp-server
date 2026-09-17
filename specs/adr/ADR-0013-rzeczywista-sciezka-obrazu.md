# ADR-0013: Rzeczywista ścieżka obrazu — Set Surface Bits bez kodeka, nie Bitmap Update RDP 6.0

- **Status:** Proposed
- **Data:** 2026-09-17
- **Dotyczy:** ADR-0006 (częściowo zastępuje po przyjęciu), F-004, F-008,
  F-104, `specs/02-ms-rdpbcgr.md` §4 P-01 i P-19
- **Powód powstania:** przegląd MS-RDPBCGR z 2026-09-17 wykazał, że ADR-0006
  opisuje inną ścieżkę wyjścia graficznego niż ta, którą wybiera kod
  IronRDP 0.13.0 dla klientów ogłaszających Surface Commands.

## Kontekst

ADR-0006 (Accepted, 2026-07-22) naprawił czarny ekran, ustawiając pustą listę
kodeków (`with_bitmap_codecs(BitmapCodecs(Vec::new()))`,
`src/session/mod.rs:88`). Ścieżkę, na której serwer wtedy zostaje, ADR opisuje
jako „`BitmapUpdater::None` (gołe bitmapy RDP 6.0)” i „ścieżka Bitmap Update
z kompresją RDP 6.0”. Na tym opisie stoją:
- uzasadnienie F-004.6 i F-004.9 (wielokrotność 4, wzięta
  z `encoder/bitmap.rs`),
- interpretacja pomiarów w F-004 §7 („przepustowość kopiowania i kompresji
  RDP 6.0”),
- wniosek z F-004 §8 pkt 4, że Frame Marker jest częścią ścieżki, „której
  świadomie nie używamy”.

**Poprawka z ADR-0006 pozostaje słuszna.** Czarny ekran zniknął, a test
`tests/rdp_pixels.rs` go pilnuje. Błędny jest wyłącznie **opis tego, co idzie
po drucie**.

## Zbadany stan faktyczny (2026-09-17)

### Wybór ścieżki w IronRDP 0.13.0

`UpdateEncoder::new` (`ironrdp-server-0.13.0/src/encoder/mod.rs:132-159`)
w uproszczeniu:

```rust
let bitmap_updater = if surface_flags.contains(CmdFlags::SET_SURFACE_BITS) {
    match codecs {
        /* qoiz */ .. => BitmapUpdater::Qoiz(..),
        /* qoi  */ .. => BitmapUpdater::Qoi(..),
        /* rfx  */ .. => BitmapUpdater::RemoteFx(..),
        /* nsc  */ .. => BitmapUpdater::NsCodec(..),   // #[cfg(feature = "nscodec")]
        _ => BitmapUpdater::None(NoneHandler),
    }
} else {
    BitmapUpdater::Bitmap(BitmapHandler::new())       // RDP 6.0 Bitmap Update
};
```

- **Dwa pozornie podobne warianty.** `BitmapUpdater::None` to **nie**
  „bitmapy RDP 6.0”. RDP 6.0 to `BitmapUpdater::Bitmap`, wybierany **tylko
  wtedy, gdy klient nie ogłasza `SET_SURFACE_BITS`** w Surface Commands
  Capability Set (MS-RDPBCGR 2.2.7.2.9, str. 157–158).
- **Co robi `NoneHandler`** (`encoder/mod.rs:491-499`):
  - odwraca kolejność wierszy (od dołu do góry),
  - ucina wypełnienie stride'u do `width * bpp`,
  - wysyła wynik przez `set_surface(bitmap, CodecId::None.as_u8(), &data)`.

  To jest **fast-path Surface Commands Update** (`updateCode = 4`,
  MS-RDPBCGR 2.2.9.1.2.1, str. 205–206) z **Set Surface Bits**
  (`CMDTYPE_SET_SURFACE_BITS`, 2.2.9.2.1, str. 213–215) i **`codecID = 0`**
  — „dane nie są zakodowane i można ich użyć bez dekodowania” (str. 215).
  Innymi słowy: **surowe BGRA 32 bpp, bez kompresji.**
- **Nasza konfiguracja:** lista kodeków jest pusta, więc każdy klient
  z `SET_SURFACE_BITS` dostaje `NoneHandler`.

### Które klienty i testy idą którą ścieżką

| Klient | Ogłasza `SET_SURFACE_BITS`? | Ścieżka | Pewność |
|---|---|---|---|
| `ironrdp-connector` 0.10.0 — nasz `examples/probe_client.rs` i `tests/rdp_pixels.rs` | **Tak**: `SET_SURFACE_BITS \| STREAM_SURFACE_BITS \| FRAME_MARKER` (`ironrdp-connector-0.10.0/src/connection_activation.rs:411`) | Surface Bits, `codecID = 0` | [Z] kod |
| Windows App (macOS) | ? | ? | **D-1** |
| mstsc (Windows) | ? | ? | **D-1** |
| xfreerdp | ? | ? | **D-1** |

Wynik [Z] ma konsekwencję wsteczną: **pomiar z ADR-0006** (99,6% pikseli po
poprawce) oraz test `tests/rdp_pixels.rs` sprawdzały ścieżkę **Surface Bits**,
a nie RDP 6.0. **Ścieżka RDP 6.0 (`BitmapHandler`) nie ma dziś ani jednego
testu na pikselach** — a `02-ms-rdpbcgr.md` §4 P-19 wskazuje w niej możliwy
błąd wysokości fragmentu [I].

### Skala

Surowe dane: 5120 × 1440 × 4 = **29,5 MB na pełną klatkę**, bez kompresji.
To jest zgodne [I] z sufitem ~2 fps przy zmianie całego ekranu, zmierzonym
w F-004 §7.

## Procedura pomiaru D-1 (dla właściciela)

Cel: ustalić dla każdego klienta z tabeli, czy ogłasza `SET_SURFACE_BITS`
i jaki `updateCode` wysyła serwer.

**Krok 1 — log (wystarcza do rozstrzygnięcia, bez Wiresharka).** Wybór
ścieżki jest deterministyczny: zależy od flag klienta i naszej pustej listy
kodeków. Wystarczy więc zobaczyć Confirm Active klienta.

1. Uruchom serwer:
   ```sh
   MAC_RDP_LOG=info,ironrdp_acceptor=debug cargo run --release -- <zwykłe flagi>
   ```
2. Połącz się klientem.
3. W logu znajdź wpis `Received` z `ClientConfirmActive`
   (`ironrdp-acceptor-0.10.0/src/connection.rs:844`), a w nim
   `SurfaceCommands(SurfaceCommands { flags: CmdFlags(…) })`. Przy `flags`
   zawierających `SET_SURFACE_BITS` ⇒ Surface Bits `codecID = 0`. Przy braku
   zestawu `SurfaceCommands` albo tej flagi ⇒ RDP 6.0.
4. Przy okazji zanotuj z tego samego wpisu `pointer_cache_size` (zestaw
   `Pointer`) i `max_request_size` (zestaw `MultiFragmentUpdate`). To pomiar
   D-5, potrzebny F-005.
5. Powtórz dla każdego klienta z tabeli.

**Krok 2 — potwierdzenie na drucie (opcjonalne, zalecane dla jednego
klienta).**

1. `brew install --cask wireshark`.
2. Uruchom serwer z `SSLKEYLOGFILE=$HOME/rdp-keys.log`: `make_acceptor()`
   włącza `rustls::KeyLogFile` (ADR-0004).
3. Wireshark: Preferences → Protocols → TLS → (Pre)-Master-Secret log
   filename = ten plik. Przechwytuj na interfejsie klienta (`lo0` dla
   klienta lokalnego), filtr `tcp.port == 3389`.
4. W odszyfrowanych danych serwer → klient, w każdym `TS_FP_UPDATE`
   (str. 205–206), niskie 4 bity pierwszego bajtu (`updateHeader`) to
   `updateCode`: `1` = Bitmap, `4` = Surface Commands.
   - Przy `4` dane aktualizacji (za polem długości) zaczynają się od
     `cmdType`, 2 bajty; `0x0001` = Set Surface Bits.
   - `codecID` leży w `TS_BITMAP_DATA_EX` (str. 215).

**Wyniki D-1** (wypełnia właściciel):

| Data | Klient i wersja | `SET_SURFACE_BITS` | `updateCode` | `pointer_cache_size` | `max_request_size` | Uwagi |
|---|---|---|---|---|---|---|
| 2026-09-17 | Klient Windows, `client_build: 26100`, `client_name: "DESKTOP-2UHRU13"` (aplikacja do potwierdzenia: mstsc czy Windows App) | **Tak** — `CmdFlags(SET_SURFACE_BITS \| FRAME_MARKER)` | 4 (wniosek z flag i pustej listy kodeków; Wireshark nie wykonany) | 21 (`color_pointer_cache_size`: 20) | 8 388 608 | Log `target/measure-windowsapp.log`. `desktop_resize_flag: true`. Kodeki klienta: NSCodec (id 1), ImageRemoteFX (id 5) i dalsze — nieużywane, bo nasza lista jest pusta. Pełna klatka 5120×1440 = 29 491 200 B (`avg_frame_bytes`). |
| | mstsc / Windows App (druga aplikacja) | | | | | |
| | xfreerdp | | | | | |

## Decyzja (proponowana)

1. **Stan faktyczny powyżej obowiązuje** jako opis ścieżki obrazu dla klientów
   z `SET_SURFACE_BITS`. Pusta lista kodeków z ADR-0006 **zostaje** — to ona
   chroni przed czarnym ekranem.
2. **Po wypełnieniu tabeli D-1** ADR-0006 dostaje status
   „Accepted — opis ścieżki częściowo zastąpiony przez ADR-0013”, a zdania
   z sekcji „Konsekwencje dla dokumentów” zostają poprawione **jednym**
   commitem `specs:`.
3. **Obie ścieżki są żywe**, dopóki D-1 nie wykaże, że żaden realny klient nie
   trafia do RDP 6.0. Jeśli któryś trafia, powstaje wymaganie na test
   pikselowy ścieżki RDP 6.0: sonda **bez** `SET_SURFACE_BITS`, co ujawni też
   P-19. Wymaganie trafia do F-004 albo osobnego speca — nie do tego ADR.
4. **Konsekwencja dla F-104 i Frame Markera:** skoro obraz już jedzie Surface
   Commands, Frame Marker (2.2.9.2.2, str. 216–217) przestaje być „zmianą
   ścieżki graficznej” (F-004 §8 pkt 4). Staje się **brakującym PDU w IronRDP**
   — w całym `ironrdp-server` 0.13.0 nadal nie ma odwołania do frame markera.
   To kandydat na poprawkę w forku (ADR-0011). Kompresja (NSCodec, RemoteFX)
   pozostaje sprawą F-104 i nierozwiązanego czarnego ekranu z ADR-0006
   §„Otwarta kwestia”.

## Konsekwencje dla dokumentów (do zmiany po D-1)

| Dokument | Zdanie / miejsce | Zmiana |
|---|---|---|
| ADR-0006 | Tytuł „…zostajemy przy RDP 6.0” | Dopisek, że ścieżką dla klientów z `SET_SURFACE_BITS` jest Surface Bits `codecID = 0` (ADR-0013) |
| ADR-0006 §„Zbadany stan faktyczny” | „`qoiz → qoi → remotefx → BitmapUpdater::None (gołe bitmapy RDP 6.0)`” | `None` = Set Surface Bits bez kodeka; RDP 6.0 = `BitmapUpdater::Bitmap`, tylko bez `SET_SURFACE_BITS` |
| ADR-0006 §„Decyzja” | „serwer ma zostać na ścieżce Bitmap Update z kompresją RDP 6.0” | „…na ścieżce bez kodeka: Surface Bits `codecID = 0` albo Bitmap Update RDP 6.0, zależnie od klienta” |
| ADR-0006 §„Konsekwencje” | „Wydajność zostaje na poziomie gołych bitmap” | Doprecyzowanie: dla klientów z Surface Commands — **nieskompresowanych** |
| F-004 §2 | „Kompresja RDP 6.0 po stronie IronRDP to zbija” | Dla klientów z Surface Commands kompresji nie ma |
| F-004 §3 „Kontrakt z enkoderem IronRDP” | Uzasadnienie F-004.6/F-004.9 z `encoder/bitmap.rs` | Wymagania zostają (nieszkodliwe, chronią ścieżkę RDP 6.0); uzasadnienie dotyczy tylko tej ścieżki |
| F-004 §7 „Sufit jest wspólny” | „przepustowość kopiowania i kompresji RDP 6.0 dla 5120×1440” | „…kopiowania i przesłania surowych 29,5 MB/klatkę” (jeśli D-1 potwierdzi) |
| F-004 §8 pkt 4 | „Surface Commands — ścieżki, której świadomie **nie używamy**” | Ścieżka **jest** używana; Frame Marker to brakujący PDU w IronRDP (Decyzja pkt 4) |
| `01-architecture.md` §3 | „RDP 6.0 bitmap compression → FastPath” | Obie ścieżki, zależnie od klienta |
| `00-vision.md` §4, ADR-0001 „Deklarowane wsparcie”, F-001 §4 | wzmianki o RDP 6.0 | Bez zmian treści (opis ówczesnego stanu i README IronRDP); ewentualnie przypis do ADR-0013 |
| `02-ms-rdpbcgr.md` §4 P-01, P-19 | oznaczenia [I] | Zmiana na [Z] po D-1 |

## Odrzucone alternatywy

- **Poprawić ADR-0006 od razu, bez pomiaru.** Odrzucone. Dla naszych testów
  fakt jest pewny (kod), ale dla prawdziwych klientów to wciąż wniosek.
  Zasada z `CLAUDE.md`: stan faktyczny z datą, a nie konkluzja.
- **Wymusić RDP 6.0 dla wszystkich klientów** (ukryć `SET_SURFACE_BITS` przed
  enkoderem). W 0.13.0 nie ma do tego publicznego API. Byłby to też krok
  wstecz wobec F-104, bo Surface Commands to droga do kodeków i Frame Markera.
- **Nic nie robić, bo „działa”.** Odrzucone: błędny opis ścieżki już raz
  zafałszował wniosek (F-004 §8 pkt 4) i zafałszuje planowanie F-104 i F-107.
