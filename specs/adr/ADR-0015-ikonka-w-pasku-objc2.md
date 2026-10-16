# ADR-0015 — Ikonka w pasku menu: `objc2` w Ruście, w jednym procesie

- **Status:** Proposed (2026-10-16) — czeka na approve właściciela
- **Dotyczy:** F-111 (ikonka w pasku), F-112 (domyślny plik konfiguracyjny),
  F-109 (`launchd`), F-106 (bundel `.app`), `CLAUDE.md` §Granice
  architektoniczne i §Zależności

## Kontekst

Właściciel poprosił o ikonkę w górnym pasku z włącznikiem serwera (2026-10-12:
„myślałem, że można by zrobić w górnym pasku jakąś ikonkę. I tam dać suwaczek
czy włączyć, czy nie włączyć serwer"), a 2026-10-16 wskazał F-111 jako
następne zadanie.

Dwa pytania stały na drodze do specu. Pierwsze — czy pasek menu w ogóle działa
z binarki bez bundla `.app` — zostało rozstrzygnięte pomiarem 2026-10-14
(`waves.md` §Pomiar, sonda `specs/probes/f111-nsstatusitem/`): **działa**.
Drugie, i to jest przedmiotem tego ADR: **czym to napisać**.

Realne drogi są dwie:

- **A — Swift jako osobny proces.** Mały program AppKit obok binarki serwera,
  sterujący nim przez uruchomienie i ubicie procesu albo przez IPC.
- **B — `objc2` w Ruście, w tym samym procesie.** Pasek menu i serwer w jednej
  binarce.

### Zbadany stan faktyczny (2026-10-16)

Wersje z crates.io, odczytane tego dnia:

| Crate | Najnowsza stabilna | Ostatnie wydanie |
|---|---|---|
| `objc2` | 0.6.5 | 2026-10-05 |
| `objc2-app-kit` | 0.3.2 | 2025-10-04 |
| `objc2-foundation` | 0.3.2 | 2025-10-04 |

**Koszt w zależnościach: 15 nowych cratów.** Zmierzone na osobnym pakiecie
(`cargo generate-lockfile` na manifeście z samym `objc2-app-kit` i tokio):
`block2`, `dispatch2`, `objc2`, `objc2-app-kit`, `objc2-cloud-kit`,
`objc2-core-data`, `objc2-core-foundation`, `objc2-core-graphics`,
`objc2-core-image`, `objc2-core-text`, `objc2-core-video`, `objc2-encode`,
`objc2-foundation`, `objc2-io-surface`, `objc2-quartz-core`. Listy featurów to
nie zmieniają — `objc2-app-kit` zależy od tamtych bezwarunkowo, bo występują
w sygnaturach typów. Dwa z nich (`objc2-core-foundation`,
`objc2-core-graphics`) to **drugi, równoległy zestaw bindingów** do tych samych
frameworków, do których mamy już `core-foundation` i `core-graphics 0.25`
(ADR-0003). To jest świadomy koszt, nie przeoczenie.

**Sonda w Ruście** (`specs/probes/f111-objc2/`, 131 linii) skompilowała się za
pierwszym razem i pokazała:

| Pytanie | Wynik |
|---|---|
| Ikonka z binarki Rusta bez bundla | **Działa.** `RDP*ON` widoczne na zrzucie prawdziwego paska; `NSApplicationActivationPolicy::Accessory`, Dock czysty |
| Klasa Objective-C zdefiniowana w Ruście jako `target`/`action` | **Działa.** `define_class!` + `sel!(toggle:)`; w logu `[main-thread callback] toggle: -> running=true` |
| Czy callback zmienia stan dzielony z resztą programu | **Tak.** Wspólny `Arc<AtomicBool>` zmieniony z poziomu metody ObjC i odczytany po stronie Rusta |
| **Czy tokio żyje, gdy `NSApp.run()` trzyma główny wątek** | **Tak.** `[tokio worker ThreadId(3)] alive, tick 48` — runtime zbudowany ręcznie przed oddaniem głównego wątku AppKitowi tykał bez przerwy |
| Przynależność do głównego wątku | Wymuszona **typem**: `NSStatusItem::button(&self, mtm: MainThreadMarker)`, `NSMenu::new(mtm)`. Nie da się tego pomylić przypadkiem |

**Pułapka pomiarowa, ta sama co w sondzie Swift.** Pozycja przycisku odczytana
przez `window.convertRectToScreen(button.frame())` **przed** `app.run()` to
`x=0 y=0` — nieprawda, bo układ paska powstaje dopiero po starcie pętli
zdarzeń. Zrzut ekranu jest dowodem, API nie. Warto to pamiętać przy pisaniu
testów do F-111: `isVisible == true` nie znaczy „widać".

**Czego sonda nie sprawdziła**, i co trzeba będzie sprawdzić w implementacji:
nie uruchamiała prawdziwego serwera ani ScreenCaptureKit obok AppKit, nie
sprawdzała `Ctrl-C` (F-001.17) po zamianie `#[tokio::main]` na ręcznie
budowany runtime i nie klikała w menu prawdziwym zdarzeniem myszy — to ostatnie
zmierzyła sonda Swift 2026-10-14 i tamten wynik się przenosi.

## Decyzja

**B — `objc2` w Ruście, jeden proces.** Powody, w kolejności wagi:

1. **Jedna binarka to jeden podmiot TCC.** Przy wariancie A proces serwera
   byłby dzieckiem aplikacji Swift, więc odpowiedzialność za uprawnienia
   przeniosłaby się na tę aplikację. To nie jest samo w sobie złe — ale
   zmienia podmiot, do którego macOS przypina zgody, czyli wchodzi w materię
   F-106 i F-109, których jeszcze nie rozstrzygnęliśmy. Nie chcemy
   rozstrzygać ich przy okazji ikonki.
2. **Jedna binarka to jeden artefakt wydania.** Wariant A oznacza drugi plik
   w tarballu, drugi podpis `codesign`, drugą ścieżkę w formule i drugi
   powód, żeby wydanie się rozjechało. F-110 kosztowało dość, żeby tego nie
   komplikować.
3. **Stan dzielony zamiast protokołu.** Przełącznik ma włączyć i wyłączyć
   nasłuchiwanie. W jednym procesie to jest uchwyt do zadania tokio;
   w dwóch — protokół IPC, który trzeba zaprojektować, zabezpieczyć (bo
   steruje serwerem bez uwierzytelniania) i przetestować.
4. **Ryzyko techniczne zmierzone, nie założone.** Jedyne realne pytanie
   brzmiało „czy AppKit i tokio da się pogodzić w jednym procesie" i
   odpowiedź jest zmierzona: da się, bo runtime buduje się ręcznie i żyje na
   własnych wątkach, a główny wątek oddaje się AppKitowi.

Zależności przyjmujemy jako `objc2 = "0.6"`, `objc2-app-kit = "0.3"`,
`objc2-foundation = "0.3"` — **bez pinowania na dokładną wersję**. Zasada
z `CLAUDE.md` §Zależności dotyczy cratów o szybkim tempie breaking changes
(`ironrdp-server`, `screencapturekit`); `objc2-app-kit` nie wydał nic od
2025-10-04, więc pinowanie kupiłoby tu niewiele, a kosztowałoby ręczne bumpy.
`Cargo.lock` i tak jest w repo, więc wersja w wydaniu jest zamrożona.

## Konsekwencje

- **`src/main.rs` przestaje być `#[tokio::main]`.** Runtime budujemy ręcznie
  (`tokio::runtime::Builder`), a główny wątek oddajemy AppKitowi **tylko**
  w trybie paska menu. Tryb CLI zostaje bez zmian — to jest warunek, żeby
  F-111 nie było regresją dla kogoś, kto używa serwera z terminala.
- **Nowy moduł `src/menubar/`.** Granica z `CLAUDE.md` §Granice obowiązuje go
  tak samo jak resztę: **`menubar` nie importuje `ironrdp_*`** (pilnuje tego
  test CI z F-001.12) i rozmawia z serwerem przez nasze typy. Symetrycznie do
  reszty: `capture` nie zna RDP, `session` nie zna macOS, `menubar` zna macOS
  i nie zna RDP.
- **Drzewo zależności rośnie o 15 cratów**, w tym o drugi zestaw bindingów do
  Core Foundation i Core Graphics. Czas budowania wydania (dziś 11 minut)
  wzrośnie — o ile, zmierzymy przy pierwszym wydaniu z F-111, zamiast zgadywać.
- **Decyzja jest odwracalna w jedną stronę.** Jeśli F-106 pójdzie w stronę
  podpisanego bundla `.app`, wariant A wraca do rozważenia, bo wtedy stabilna
  tożsamość TCC jest celem samym w sobie i argument 1 się odwraca. Ten ADR
  dostanie wtedy status Superseded, nie zniknie.

## Odrzucone alternatywy

**A — Swift jako osobny proces.** Plusy są realne: zero nowych cratów, natywne
API bez tłumaczenia, kod interfejsu czytelny dla kogoś, kto zna AppKit, i
`swiftc` jest na tej maszynie. Minusy wyżej: zmiana podmiotu TCC przy okazji,
drugi artefakt w wydaniu, protokół IPC do zaprojektowania i obrony. Wraca do
rozważenia razem z F-106.

**C — gotowy crate `tray-icon` albo podobny.** Nie zmierzone i dlatego nie
odrzucone na podstawie faktów, tylko odłożone: biblioteka wieloplatformowa
płaci abstrakcją za rzeczy, których potrzebujemy konkretnie na macOS
(`Accessory`, zachowanie przy wielu ekranach), a zależności i tak sprowadza
przez `objc2`. Jeżeli implementacja F-111 okaże się żmudna, warto ją wtedy
zmierzyć, zanim napiszemy trzysta linii bindingów — i wtedy dopisać tu wynik.

**D — bundel `.app` od razu.** To jest F-106, osobny spec i osobny koszt
(konto Apple Developer, notaryzacja). Pomiar 2026-10-14 pokazał, że ikonka go
**nie wymaga**, więc wiązanie jednego z drugim tylko opóźniłoby obie rzeczy.
