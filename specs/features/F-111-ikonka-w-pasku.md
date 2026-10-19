# F-111 — Ikonka w pasku menu z włącznikiem serwera

## 1. Cel

Serwer da się włączyć, wyłączyć i zobaczyć jego stan z górnego paska menu,
bez terminala.

## 2. Wymagania

### Tryb i widoczność

- **F-111.1** (MUST) Binarka MUSI umieć wystartować w trybie paska menu pod
  flagą **`--menu-bar`**. Bez tej flagi zachowanie MUSI być identyczne jak
  dziś — tryb paska nie może być regresją dla kogoś, kto używa serwera
  z terminala albo ze skryptu.
- **F-111.2** (MUST) W trybie paska proces NIE MOŻE pojawiać się w Docku ani
  przejmować fokusu przy starcie
  (`NSApplicationActivationPolicy::Accessory`, zmierzone w ADR-0015).
- **F-111.3** (MUST) Tryb paska NIE MOŻE wymagać bundla `.app` — pomiar
  2026-10-14 pokazał, że zwykła binarka wystarcza, a uzależnienie się od
  bundla wiązałoby F-111 z F-106 bez potrzeby.

### Co widać i co się klika

- **F-111.4** (MUST) Ikonka MUSI pokazywać stan serwera: **nasłuchuje** albo
  **nie nasłuchuje**. Stan ma być widoczny z samej ikonki, bez rozwijania
  menu.
- **F-111.5** (MUST) Menu MUSI zawierać pozycję, która **włącza i wyłącza**
  nasłuchiwanie, opisaną tym, co się stanie po kliknięciu (nie samym stanem).
- **F-111.6** (MUST) Menu MUSI pokazywać **adres i port**, na którym serwer
  nasłuchuje albo nasłuchiwałby po włączeniu.
- **F-111.7** (MUST) Menu MUSI pokazywać stan obu uprawnień (Screen Recording,
  Accessibility) i MUSI prowadzić do właściwego panelu Ustawień dla tego,
  którego brakuje. Logika sprawdzania już istnieje — F-110.15, `src/perms.rs`.
- **F-111.8** (MUST) Menu MUSI mieć pozycję kończącą program, a jej wybranie
  MUSI zamknąć serwer tak samo czysto jak `Ctrl-C` dzisiaj (F-001.17).
- **F-111.9** (SHOULD) Ikonka POWINNA być obrazkiem w trybie `template`, czyli
  takim, który sam dostosowuje się do jasnego i ciemnego paska, a nie
  tekstem ani kolorową bitmapą.

### Zachowanie serwera

- **F-111.10** (MUST) Wyłączenie z menu MUSI zamknąć gniazdo nasłuchujące
  i rozłączyć podłączonego klienta. Włączenie MUSI postawić nasłuchiwanie
  z powrotem — **bez restartu procesu**.
- **F-111.11** (MUST) Jeżeli włączenie się nie uda (zajęty port, brak
  certyfikatu, brak uprawnień), menu MUSI to pokazać, a ikonka MUSI zostać
  w stanie „nie nasłuchuje". Cicha porażka jest tu gorsza niż w terminalu, bo
  nie ma gdzie zobaczyć logu.
- **F-111.12** (MUST) Przełączanie MUSI dać się robić wielokrotnie w jednym
  procesie bez wycieku gniazd i wątków — dziesięć cykli włącz/wyłącz zostawia
  proces w tym samym stanie co jeden.
- **F-111.13** (MUST) Ustawienia, których używa tryb paska (adres, port,
  ekran), MUSZĄ pochodzić z tego samego źródła co w trybie CLI, czyli z
  **F-112**. Bez tego ikonka umiałaby tylko to, co ktoś wpisał w terminalu.

### Granice i struktura

- **F-111.14** (MUST) Kod paska menu MUSI żyć w osobnym module `src/menubar/`
  i **NIE MOŻE importować `ironrdp_*`** — tak jak reszta kodu poza `session`
  (F-001.12, test w CI pilnuje tego automatycznie). Symetria z resztą:
  `capture` nie zna RDP, `session` nie zna macOS, `menubar` zna macOS i nie
  zna RDP.
- **F-111.15** (MUST) Maszyna stanów przełącznika (co wolno zrobić w jakim
  stanie, co się dzieje przy błędzie startu) MUSI być oddzielona od AppKit na
  tyle, żeby dało się ją przetestować jednostkowo, bez paska menu i bez
  uprawnień.
- **F-111.16** (MUST) Główny wątek oddajemy AppKitowi **tylko** w trybie
  paska; runtime tokio budowany ręcznie, zgodnie z ADR-0015.

### Logi

- **F-111.17** (MUST) Każda zmiana stanu wywołana z menu MUSI zostawiać wpis
  w logu (włączono, wyłączono, próba nieudana i dlaczego) — tryb paska nie
  może być trybem, w którym nie wiadomo, co się stało.

## 3. Odniesienia do dokumentacji

- **ADR-0015** — wybór `objc2` w Ruście w jednym procesie, z pomiarami.
- `specs/waves.md` §Pomiar 2026-10-14 — `NSStatusItem` bez bundla `.app`.
- `specs/waves.md` §Pomiar 2026-10-16 — AppKit obok tokio, callback z menu
  zmieniający stan dzielony.
- `specs/probes/f111-nsstatusitem/`, `specs/probes/f111-objc2/` — kod obu sond.
- AppKit: `NSStatusBar.systemStatusBar`, `NSStatusItem.button`,
  `NSApplication.setActivationPolicy(_:)`, `NSImage.isTemplate`.
- F-110.15 i `src/perms.rs` — gotowe sprawdzanie uprawnień i teksty
  instrukcji, których nie powielamy.
- F-112 — skąd biorą się ustawienia.

## 4. Kryteria akceptacji

Automatyczne (bez GUI, bez TCC):

1. **AC-1** Testy jednostkowe maszyny stanów z F-111.15: przejścia
   wyłączony → włączony → wyłączony, próba włączenia przy zajętym porcie
   zostawia stan „wyłączony" i niesie powód błędu.
2. **AC-2** Test CI z F-001.12 przechodzi z nowym modułem, czyli `menubar`
   nie importuje `ironrdp_*`.
3. **AC-3** Uruchomienie **bez** `--menu-bar` zachowuje się dokładnie jak
   przed zmianą: te same logi startowe, ten sam kod wyjścia, działający
   `Ctrl-C`. Sprawdzane testem integracyjnym, który już istnieje dla trybu
   CLI.

Ręczne (checklista właściciela — część macOS-owa, `CLAUDE.md` §Testy):

4. **AC-4** `mac-rdp-server --menu-bar` → ikonka **widać w pasku**, a w Docku
   **nie ma nic**. Dowodem jest to, co widać, nie wartość zwrócona przez API
   (pomiar 2026-10-14: `isVisible == true` zanim pasek się przerysował).
5. **AC-5** Kliknięcie ikonki rozwija menu; widać w nim adres z portem i stan
   obu uprawnień.
6. **AC-6** Przy wyłączonym serwerze klient RDP **nie łączy się**. Po
   kliknięciu „włącz" ten sam klient łączy się bez restartu programu i widać
   ekran.
7. **AC-7** Przy podłączonym kliencie kliknięcie „wyłącz" rozłącza go, a
   ikonka zmienia stan.
8. **AC-8** Dziesięć cykli włącz/wyłącz pod rząd: za każdym razem da się
   połączyć, a proces nie puchnie ani nie zostawia zajętego portu
   (`lsof -nP -iTCP:3389` po wyłączeniu nie pokazuje nic).
9. **AC-9** Przy odebranym uprawnieniu Screen Recording menu mówi, którego
   uprawnienia brakuje, i otwiera właściwy panel Ustawień.
10. **AC-10** Wybranie „Quit" kończy proces, ikonka znika z paska, port
    zostaje zwolniony.
11. **AC-11** Plik `~/.config/mac-rdp-server.toml` z innym portem → menu
    pokazuje **ten** port i serwer na nim nasłuchuje (spięcie z F-112).

## 5. Poza zakresem

- **Autostart przy logowaniu i usługa `launchd`** — F-109. Ten spec dotyczy
  programu uruchomionego ręcznie. Pomiar 2026-10-14 pokazał przy okazji, że
  pod `launchd` proces jest **własnym** podmiotem TCC, co jest problemem
  F-109, nie F-111.
- **Bundel `.app`, podpis Developer ID, notaryzacja** — F-106.
- **Edycja ustawień z menu** (pola na adres, port, wybór ekranu). Najpierw
  F-112 ma umieć czytać plik; pisanie go przez program jest jawnie poza
  zakresem F-112 i wymaga osobnej decyzji.
- **Zapamiętywanie stanu przełącznika między uruchomieniami.** Ma sens
  dopiero razem z F-109: dopóki program uruchamia się ręcznie, „czy serwer ma
  wstawać włączony" nie ma kogo pytać.
- **Podgląd podłączonego klienta, statystyki, wykres FPS.**
- **Ikonka na innych platformach.**

## 6. Status

**Approved** (2026-10-19). Draft 2026-10-16, approve właściciela 2026-10-19,
razem z ADR-0015. Kolejność: po F-007 iteracji 1d i po F-112.

Powstał z listy braków zgłoszonej 2026-10-12: „myślałem, że można by zrobić
w górnym pasku jakąś ikonkę. I tam dać suwaczek czy włączyć, czy nie włączyć
serwer". Właściciel wskazał F-111 jako następne zadanie 2026-10-16.

**Zależy od F-112** (F-111.13). To jest kolejność, a nie uzasadnienie: F-112
jest mały i zamknięty, F-111 bez niego pokazywałby wyłącznie wartości
domyślne.

**Co jest już rozstrzygnięte pomiarem, a nie zostało do dyskusji przy
approve:** że ikonka działa bez bundla `.app` (2026-10-14) i że AppKit da się
pogodzić z tokio w jednym procesie (2026-10-16). Do rozstrzygnięcia przy
approve zostaje sam zakres powyżej oraz **ADR-0015**, który jest w statusie
Proposed i bez którego F-111.16 nie ma podstawy.
