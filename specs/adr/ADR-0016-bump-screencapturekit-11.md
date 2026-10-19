# ADR-0016 — `screencapturekit` 8.0.1 → 11.0.0, bo tam jest ograniczony czas oczekiwania

- **Status:** Proposed (2026-10-19) — czeka na approve właściciela
- **Dotyczy:** F-007 §8c i wymagania F-007.19 … F-007.25, F-002
  (`frame_from_sample`), ADR-0002, ADR-0005, `CLAUDE.md` §Zależności

## Kontekst

2026-10-19 właściciel zgłosił: „po dłuższym czasie nie korzystania rdp
freezuje. Ponowne podłączenie się nie pomaga. Dalej jest zfreezowane."
Diagnoza jest w F-007 §8c; tutaj liczy się jej sedno i to, co z niego wynika
dla zależności.

`SCStream::start_capture` i `stop_capture` w `screencapturekit 8.0.1` są
synchroniczne i kończą się na `completion.wait()`
(`src/stream/sc_stream.rs:659` i `:672`). `wait` to
`doom-fish-utils 0.3.3`, `src/completion.rs:99`:

```rust
self.inner.cvar.wait_while(state, |s| s.result.is_none()).unwrap()
```

**`Condvar::wait_while` bez timeoutu.** W całym crate `doom-fish-utils` jest
dokładnie jeden `fn wait` i nie istnieje wariant z ograniczeniem czasu. Kiedy
ScreenCaptureKit nie zawoła handlera zakończenia — a przy uśpieniu ekranu nie
zawoła — wątek stoi tam do końca życia procesu. Potwierdzone `sample`
zablokowanego procesu: 2563 próbki z 2563 w `__psynch_cvwait` pod
`SCStream::start_capture`.

Skutki zmierzone na żywej maszynie: odzyskiwanie strumienia nigdy nie wraca
z próby i nie loguje porażki; start serwera potrafi zawisnąć na wątku głównym
bez żadnego komunikatu; czyste zamknięcie wisi na `stop_capture`; a **żywy
zawieszony proces blokuje start przechwytywania dla całej maszyny**, dopóki go
nie zabić.

Bump `screencapturekit` czekał na decyzję od 2026-10-12 jako zwykłe
porządkowanie zależności. Po tym pomiarze nie jest porządkowaniem: jest
kandydatem na poprawkę.

### Zbadany stan faktyczny (2026-10-19)

Źródło `screencapturekit 11.0.0` pobrane z crates.io i przeczytane.
**`start_capture` ma teraz ograniczony czas oczekiwania i stan zgłoszenia:**

```rust
// src/stream/sc_stream.rs:861
pub fn start_capture(&self) -> Result<(), SCError> {
    let context = unsafe { &*self.context };
    if !claim_start(&context.capturing, &context.start_unconfirmed) {
        return Ok(());
    }
    let (completion, context) = UnitCompletion::new();
    unsafe { ffi::sc_stream_start_capture(self.ptr, context, UnitCompletion::callback) };
    match completion.wait() {
        Ok(()) => Ok(()),
        Err(error) => { /* if is_timeout_error(&error) … */ }
    }
}
```

`src/utils/completion.rs`:

| Element | Wartość |
|---|---|
| `DEFAULT_TIMEOUT` | `Duration::from_secs(30)` |
| Nadpisanie | zmienna `SCREENCAPTUREKIT_COMPLETION_TIMEOUT_SECS`, w pełnych sekundach; `0` wyłącza ograniczenie |
| Rozpoznanie | `is_timeout_error(&error)`, po prefiksie `"screencapturekit: completion callback did not fire"` |
| Odczyt | raz, przez `OnceLock` |

Dodatkowo `claim_start` / `start_unconfirmed` pilnują, że powtórzony start nie
wysyła drugiego native'owego żądania — czyli upstream trafił dokładnie na ten
problem, na który my trafiliśmy, i rozwiązał go po swojej stronie.

**Powierzchnia breaking changes dla API, którego faktycznie używamy.**
Sprawdzone symbol po symbolu wobec źródła 11.0.0. Obecne bez zmian nazwy:
`SCShareableContent::get`, `displays`, `SCDisplay::display_id`,
`SCContentFilter::create` / `with_display` / `with_excluding_windows` /
`build`, `SCStreamConfiguration::new` / `with_width` / `with_height` /
`with_pixel_format` / `with_fps` / `with_queue_depth` / `with_shows_cursor`,
`SCStream::new_with_delegate` / `add_output_handler` / `start_capture` /
`stop_capture`, `SCStreamDelegateTrait::did_stop_with_error`,
`SCStreamOutputTrait::did_output_sample_buffer`, `SCStreamOutputType`,
`SCFrameStatus`, `SCError`, `SCShareableContentInfo`, `frame_status`,
`pixel_format`.

Zmienia się **jedna funkcja** — `frame_from_sample` (`src/capture/screen.rs:124`):

| 8.0.1 | 11.0.0 |
|---|---|
| `sample.image_buffer()` | `sample.pixel_buffer()` |
| `buffer.lock_read_only()` | `buffer.lock(CVPixelBufferLockFlags::READ_ONLY)` |
| `buffer.is_planar()`, `plane_count()`, `bytes_per_row()` | w module `screencapturekit::cv`, za feature'em `cv` |
| typy z `screencapturekit::cm` | `screencapturekit::cv::{CVPixelBuffer, CVPixelBufferLockFlags, PixelBufferCursorExt}` |

11.0.0 zależy od rodziny `apple-cf` (`>=0.11, <0.12`, features `cg`,
`iosurface`, `dispatch`, `cv`, `cm`). To pięć wywołań w jednym miejscu, każde
z bezpośrednim odpowiednikiem — koszt zmierzony, nie oszacowany.

## Decyzja

**Bump do `=11.0.0`** i oparcie F-007.19 na ograniczeniu czasu, które daje
crate, zamiast budowania własnego.

Powody, w kolejności wagi:

1. **Leczy przyczynę tam, gdzie ona jest.** Blokada siedzi w crate, nie
   w naszym kodzie. Każda nasza obrona (niżej, wariant B) zostawia zawieszony
   wątek żywy — a zmierzyliśmy, że **żywy zawieszony proces blokuje start
   capture dla całej maszyny**. Obrona, która nie zdejmuje blokady, leczy nasz
   log, a nie maszynę użytkownika.
2. **`claim_start` to drugi problem, którego sami byśmy nie znaleźli.**
   Istnienie `start_unconfirmed` w 11.0.0 mówi, że ponowny start przy
   niepotwierdzonym poprzednim jest realnym trybem awarii. Przy wariancie B
   wchodzilibyśmy w niego na każdej próbie odzyskania.
3. **Koszt jest zmierzony i mały.** Pięć wywołań w jednej funkcji, wszystkie
   z odpowiednikiem. Trzy główne wersje różnicy brzmią drogo, dopóki nie
   sprawdzi się, czego używamy.

Pinowanie zostaje: `=11.0.0`, zgodnie z `CLAUDE.md` §Zależności i ADR-0002
§Ryzyko. `toml 0.9 → 1.1` **nie wchodzi** do tego ADR — to osobna decyzja,
niezwiązana z tą awarią.

## Konsekwencje

- **30 s to dla nas za długo.** Serwer ma wracać w sekundach, nie po pół
  minuty. Nadpisanie istnieje tylko przez zmienną środowiskową, w pełnych
  sekundach i czytaną **raz** przez `OnceLock` — więc ustawienie jej musi
  nastąpić przed pierwszym wywołaniem SCK, a w edycji 2024 `std::env::set_var`
  jest `unsafe`. Wartość i miejsce ustawienia są częścią implementacji
  F-007.19 i trzeba je uzasadnić w kodzie, nie przyjąć domyślnie.
- **F-007.20 zostaje w mocy niezależnie od crate'a.** Nawet ograniczone 30 s
  (albo 3 s) oczekiwania nie ma prawa stać na wątku runtime'u ani na wątku
  głównym. Bump usuwa „na zawsze", nie usuwa „blokująco".
- **Do ponownej weryfikacji po bumpie:** F-002 AC-5 (rozmiar pulpitu brany
  z pierwszej klatki), arytmetyka stride wg ADR-0005 — bo zmieniają się
  akcesory bufora — oraz F-007 AC-1, AC-2, AC-3, AC-5, AC-7, AC-8. AC-8 jest
  po to, żeby ta weryfikacja była skryptem, nie wspomnieniem.
- **Nowy feature `cv`** w zależności i drugi zestaw bindingów z rodziny
  `apple-cf` obok tego, co już mamy. Policzymy to po bumpie, zamiast zgadywać
  teraz.
- **Obejście na czas przed poprawką zostaje udokumentowane, nie wdrożone:**
  `caffeinate -d mac-rdp-server …` (trzyma `PreventUserIdleDisplaySleep`,
  sprawdzone w `pmset -g assertions`) i `pkill -9 mac-rdp-server` **przed**
  ponownym uruchomieniem, bo inaczej następny start zawiśnie.

## Odrzucone alternatywy

**B — zostać na 8.0.1 i obudować wywołania po naszej stronie**
(`spawn_blocking` + `tokio::time::timeout`). Plus: zero zmian w zależnościach,
natychmiastowe. Minusy przeważają: zawieszony wątek **nie znika** po
timeoucie — zostaje zaparkowany na condvarze na zawsze, po jednym na każdą
nieudaną próbę, a to właśnie żywa blokada psuje start capture dla całej
maszyny. Do tego nie jest zbadane, czy świeży strumień wstanie, gdy poprzedni
native start wisi w locie; `claim_start` w 11.0.0 sugeruje, że nie. Zostaje
jako plan awaryjny, jeśli bump okaże się droższy, niż pokazał pomiar.

**C — unikać bodźca: trzymać asercję `PreventUserIdleDisplaySleep` na czas
serwowania.** Sprawdzone, że działa (`caffeinate -d` trzyma asercję). Ale omija
jeden bodziec z kilku — zamknięcie klapy, rekonfiguracja ekranów i restart
`replayd` prowadzą do tej samej blokady — i zmienia zachowanie energetyczne
maszyny użytkownika bez pytania. Jako **obejście na teraz** jest dobre i jest
opisane w sondzie; jako poprawka ukrywa błąd.

**D — własny fork `doom-fish-utils` z `wait_timeout_while`, podłączony przez
`[patch.crates-io]`.** Technicznie to jedna linia. Ale mamy już jeden fork do
utrzymywania (IronRDP, ADR-0011 i ADR-0014) i znamy jego koszt: rebase przy
każdym wydaniu upstreamu. Drugi fork dla zmiany, którą upstream **już zrobił
sam**, jest gorszym interesem niż bump wersji.

**E — nie robić nic i polegać na watchdogu zastoju (F-007.14).** Nie zadziała
z definicji: watchdog wymaga rosnącego `rejected`
(`new_rejections == 0 ⇒ return None`), a tu nie rośnie nic, bo strumień nie
oddaje już żadnych próbek. W tym incydencie wyzwalacz i tak odpalił poprawnie
przez delegata — zawiodło odzyskiwanie, więc i naprawa należy do odzyskiwania.
