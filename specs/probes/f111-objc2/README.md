# Sonda F-111 — pasek menu z Rusta przez `objc2`

Ten katalog **nie jest częścią programu**. `cargo` go nie widzi (nie ma go
w workspace), CI go nie buduje. To jest dowód do pomiaru opisanego
w `specs/waves.md` (§„Pomiar 2026-10-16") i w ADR-0015, zachowany dlatego, że
ADR ma zawierać zbadany stan faktyczny, a nie samą konkluzję
(`CLAUDE.md` §ADR).

Sonda w Swifcie (`../f111-nsstatusitem/`) odpowiedziała na pytanie, czy pasek
menu działa z binarki bez bundla `.app`. Ta odpowiada na następne: czy da się
to zrobić **w Ruście, w tym samym procesie co serwer**.

## Co sprawdza

1. Czy `objc2` + `objc2-app-kit` wystarczą, żeby binarka Rusta miała własny
   element w pasku menu.
2. Czy klasę Objective-C zdefiniowaną w Ruście (`define_class!`) da się
   podpiąć jako `target`/`action` pozycji menu i czy jej metoda naprawdę się
   wykonuje.
3. Czy ta metoda może zmienić stan dzielony z resztą programu
   (`Arc<AtomicBool>`).
4. **Czy tokio żyje, kiedy `NSApp.run()` zajmuje główny wątek** — to jest
   pytanie, od którego zależy kształt `src/main.rs`.

## Jak uruchomić

```sh
cp -r specs/probes/f111-objc2 /tmp/f111-objc2 && cd /tmp/f111-objc2
cargo run
```

Sonda sama kończy się po 25 sekundach, żeby nie została w pasku.

## Co zmierzono 2026-10-16

- Ikonka `RDP*ON` **widoczna na zrzucie prawdziwego paska menu**, z binarki
  bez bundla, przy `NSApplicationActivationPolicy::Accessory` (Dock czysty).
- `menu.performActionForItemAtIndex(0)` wywołało metodę `toggle:`
  zdefiniowaną w Ruście; w logu `[main-thread callback] toggle: -> running=true`,
  a zaraz po niej `[probe] after programmatic action, running=true` — czyli
  wspólny `AtomicBool` naprawdę się zmienił.
- `[tokio worker ThreadId(3)] alive, tick 48` — runtime tokio tykał przez cały
  czas, kiedy AppKit trzymał główny wątek.
- `MainThreadMarker` jest w **sygnaturach** (`NSStatusItem::button(&self, mtm)`),
  więc przynależność do głównego wątku wymusza typ, a nie dokumentacja.

**Pułapka pomiarowa, ta sama co w sondzie Swift.** Pozycja przycisku odczytana
przez `window.convertRectToScreen(button.frame())` **przed** `app.run()` to
`x=0 y=0 w=77 h=22` — czyli nieprawda. Układ paska powstaje dopiero po starcie
pętli zdarzeń. Zrzut ekranu jest dowodem, API nie.

## Czego ta sonda NIE sprawdziła

- Nie uruchamiała prawdziwego serwera ani ScreenCaptureKit obok AppKit.
- Nie sprawdzała `Ctrl-C` (F-001.17) po zamianie `#[tokio::main]` na ręcznie
  budowany runtime.
- Nie klikała w menu prawdziwym zdarzeniem myszy — to sonda Swift sprawdziła
  2026-10-14 i tamten wynik się przenosi.
