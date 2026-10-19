# Sonda — F-007 AC-3: obraz po uśpieniu ekranu

Odtwarza zgłoszenie właściciela z 2026-10-19: „po dłuższym czasie nie
korzystania rdp freezuje. Ponowne podłączenie się nie pomaga. Dalej jest
zfreezowane."

```
zsh run.sh                      # binarka z PATH
zsh run.sh ./target/release/mac-rdp-server
```

Skrypt nie potrzebuje klienta RDP: `source.start()` jest wołane w `main`
**przed** `serve`, więc cała awaria odtwarza się przy nikim podłączonym.
Trwa około 90 s i **gasi ekran na 25 s** — tak ma być, to jest badany bodziec.

Terminal, z którego to uruchamiasz, musi mieć zgodę Screen Recording,
inaczej serwer poda placeholder i sonda nic nie zmierzy.

## Wynik 2026-10-19: NIEZALICZONE

| Czas | Zdarzenie |
|---|---|
| 10:24:20.649 | `dropping frame error=sample buffer carries no image buffer`, `rejected` 0 → 1 |
| 10:24:20.701 | `capture stream stopped unexpectedly; recovering reason=SCStream error (No capture source provided): Failed to find any displays or windows to capture display=5` |
| 10:24:25 → 10:25:35 | `received` **stoi na 749** — przez uśpienie i 40 s po wybudzeniu |
| nigdy | brak `capture recovered`, brak `recovery attempt failed; backing off` |
| 10:25:39 | `capture supervisor did not stop within the timeout` |

Wyzwalacz zadziałał. Zawiodło odzyskiwanie, w ciszy. `sample` zablokowanego
procesu pokazał cały odcinek w jednym miejscu:

```
main → tokio block_on → ScreenCaptureKitSource::<FrameSource>::start
  → capture::screen::build_stream
    → screencapturekit::stream::sc_stream::SCStream::start_capture
      → doom_fish_utils::completion::SyncCompletion::wait
        → _pthread_cond_wait → __psynch_cvwait
```

`screencapturekit 8.0.1` (`src/stream/sc_stream.rs:659`) kończy `start_capture`
na `completion.wait()`, a `wait` (`doom-fish-utils 0.3.3`,
`src/completion.rs:99`) to `Condvar::wait_while` **bez timeoutu**. W całym
crate jest jeden `fn wait` i nie ma wariantu z ograniczeniem czasu.
`stop_capture` (`:672`) czeka identycznie. Szczegóły i decyzja: ADR-0016.

## Dwie rzeczy, które sonda wykryła przy okazji

1. **Start ma tę samą blokadę, na głównym wątku.** Drugi przebieg zawiesił się
   już na starcie: log kończy się na `display available` i przez 90 s nie ma
   nic — nawet ostrzeżenia `FIRST_FRAME_TIMEOUT`. Dlatego skrypt traktuje brak
   pierwszej klatki jako FAIL, a nie jako problem z konfiguracją.
2. **Żywy zawieszony proces blokuje start capture dla całej maszyny.**
   Zmierzone: dopóki proces zaparkowany na condvarze żył, każdy nowy serwer
   wisiał na starcie. Po `kill -9` tego procesu świeży serwer dostał pierwszą
   klatkę natychmiast (5120×1440). Nie trzeba restartować `replayd` ani
   maszyny.

   Stąd procedura ratunkowa na czas, gdy to nie jest naprawione:

   ```
   pkill -9 mac-rdp-server   # najpierw, inaczej następny start zawiśnie
   mac-rdp-server …
   ```

## Obejście na czas przed poprawką

Bodźcem jest uśpienie ekranu (`displaysleep` na tej maszynie: 10 min na
zasilaniu, 2 min na baterii). Serwer uruchomiony pod `caffeinate -d` trzyma
asercję `PreventUserIdleDisplaySleep` (sprawdzone w `pmset -g assertions`),
więc licznik nie dojdzie do uśpienia:

```
caffeinate -d mac-rdp-server …
```

To tylko omija jeden bodziec. Zamknięcie klapy, rekonfiguracja ekranów
i restart `replayd` dalej prowadzą do tej samej blokady.
