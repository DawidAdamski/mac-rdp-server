# F-110 — Dystrybucja przez Homebrew, dokumentacja i strona projektu

## 1. Cel

Żeby obca osoba na czystym Macu zainstalowała serwer jedną komendą, wiedziała
z czytelnej strony, co nadać w Ustawieniach i czym to grozi w sieci — bez
toolchaina Rusta w głowie i bez pytania autora.

## 2. Wymagania

### Tap i formuła

- **F-110.1** (MUST) Dystrybucja MUSI iść przez **własny tap Homebrew**
  w repozytorium `DawidAdamski/homebrew-mac-rdp-server` (decyzja właściciela
  2026-10-12), instalowany jako:
  ```
  brew install DawidAdamski/mac-rdp-server/mac-rdp-server
  ```
  lub, rozpisane:
  ```
  brew tap DawidAdamski/mac-rdp-server
  brew install mac-rdp-server
  ```
  Jeden tap na program jest utrwaloną konwencją właściciela — ma już
  `homebrew-bsi` i `homebrew-floci-podman` (sprawdzone lokalnie 2026-10-12
  w `/opt/homebrew/Library/Taps/dawidadamski/`).
- **F-110.2** (MUST) Formuła MUSI mieć **dwie ścieżki**, w układzie jak
  w istniejącej formule `bsi` właściciela:
  1. **stabilna** — gotowa binarka z GitHub Release, przypięta tagiem i sumą
     `sha256`, osobny `url` dla `on_arm` i `on_intel`, instalacja przez
     `bin.install`. Bez zależności budujących, bez czekania na kompilację.
  2. **`head do`** — build ze źródeł (`depends_on "rust" => :build`).

  Powód, dla którego gotowa binarka jest ścieżką główną, jest zmierzony:
  artefakt pobrany **przez formułę** nie dostaje atrybutu kwarantanny, więc
  notaryzacja nie jest potrzebna (§3, „Kwarantanna"). Formuła nie MOŻE
  wskazywać gałęzi zamiast tagu.
- **F-110.3** (MUST) Ścieżka ze źródeł (`--HEAD`, `--build-from-source`) MUSI
  definiować metodę `fetch` i używać helperów Homebrew, nie własnej listy
  argumentów:
  ```ruby
  def fetch
    system "cargo", "fetch", *std_cargo_fetch_args
  end
  ```
  Powód jest twardy: `install` w Homebrew biegnie **bez dostępu do sieci**,
  a `std_cargo_args` dokłada `--offline`, gdy `fetch` jest zdefiniowane.
  Nasz `Cargo.lock` wskazuje **zależność gitową** (fork IronRDP, ADR-0014),
  więc `cargo fetch` MUSI ściągnąć także repozytorium forku — bez tego
  `install` padnie w trybie offline. Testowalne, sprawdzane przez AC-2.
- **F-110.4** (MUST) Formuła MUSI mieć blok `test`, który uruchamia binarkę
  i sprawdza wyjście `--version`. Test nie może wymagać uprawnień TCC,
  otwartego portu ani sieci.
- **F-110.5** (MUST) Formuła MUSI wypisać `caveats` zawierające: dwa
  uprawnienia do nadania (Screen Recording, Accessibility), ścieżkę do pliku
  konfiguracyjnego, ostrzeżenie o braku uwierzytelniania (F-110.10) oraz
  zdanie z F-110.13 o ponownym nadaniu uprawnień po aktualizacji.
  Odstępstwo od konwencji jest świadome: `yabai`, `skhd` i `borders` **nie**
  wspominają uprawnień w `caveats` (§3), ale one proszą system o okno dialogowe
  przy starcie, a nasz `CGEventPost` bez Accessibility **milczy** — użytkownik
  nie dostanie żadnego sygnału, jeśli nie powiemy mu tego wcześniej.
- **F-110.6** (MUST) Wydanie MUSI być opisanym, powtarzalnym procesem: tag
  `vX.Y.Z` → build obu architektur → GitHub Release → aktualizacja `url`
  i `sha256` w formule. Procedura MUSI być zapisana w repo, nie w pamięci
  autora.
- **F-110.7** (SHOULD) Build artefaktów i aktualizacja formuły POWINNY być
  zautomatyzowane workflow'em uruchamianym tagiem wydania, żeby wersja w tapie
  nie rozjeżdżała się z wydaniem.

### Dokumentacja

- **F-110.8** (MUST) `README.md` w korzeniu MUSI zawierać: czym to jest i czym
  nie jest, wymagania (wersja macOS, Apple Silicon/Intel), instalację z F-110.1,
  **nadanie uprawnień krok po kroku**, konfigurację, sposób połączenia
  z Windows/macOS/Linuksa oraz odesłanie do `SECURITY.md`.
- **F-110.9** (MUST) Repo MUSI zawierać pliki licencji odpowiadające polu
  `license` w `Cargo.toml` (`MIT OR Apache-2.0`) — dziś ich **nie ma**, a pole
  już deklaruje obie.
- **F-110.10** (MUST) `SECURITY.md` MUSI mówić wprost, że serwer **nie ma
  uwierzytelniania**, że `--bind-addr 0.0.0.0` wystawia pulpit każdemu w sieci
  lokalnej i że nie wolno go wystawiać do internetu ani przekierowywać portu.
  MUSI też podawać kanał zgłaszania podatności.
- **F-110.11** (SHOULD) Strona projektu POWINNA stać na **GitHub Pages**
  z katalogu `docs/` na gałęzi `main` i zawierać to samo co README, plus
  zrzuty ekranu. Jedno źródło treści: strona POWINNA być generowana z tych
  samych plików Markdown, a nie utrzymywana równolegle.
- **F-110.12** (MAY) `.github/FUNDING.yml` MOŻE zostać dodany, jeśli
  właściciel zdecyduje się na donacje (model z `waves.md` §Model wydania).

### Uprawnienia po aktualizacji

- **F-110.13** (MUST) Dokumentacja MUSI opisywać zachowanie uprawnień przy
  aktualizacji zgodnie ze stanem faktycznym z §3: po `brew upgrade`
  **oba uprawnienia trzeba nadać ponownie**, na liście w Ustawieniach zostaje
  **stary wpis**, który może wyglądać na włączony, i trzeba go usunąć (minus),
  a potem dodać nowy. To nie jest nasz błąd ani przeoczenie, tylko cena braku
  Developer ID — ta sama, którą płacą `yabai` i `skhd`. Dokumentujemy ją
  wprost, zamiast udawać, że jej nie ma.
- **F-110.14** (MUST) Komunikat z `perms::explain()` MUSI wymieniać ten
  przypadek: nie wystarczy „włącz w Ustawieniach", musi być „jeśli
  aktualizowałeś serwer albo terminal, usuń stary wpis z listy i dodaj
  ponownie". To jedyna zmiana w `src/` w tym specu i jedyne wymaganie
  z własnym testem jednostkowym.
- **F-110.15** (SHOULD) Serwer POWINIEN mieć tryb sprawdzenia uprawnień
  (`--check-permissions`), który wypisuje stan obu uprawnień i instrukcję,
  i kończy się kodem ≠ 0, gdy któregoś brakuje. To jest odpowiedź na jedyny
  realny problem zgłaszany użytkownikom takich narzędzi: „nadałem, a nie
  działa". Jeśli F-109 (podkomendy CLI) wejdzie wcześniej, wymaganie przenosi
  się tam i ten punkt znika.
- **F-110.16** (MUST) Projekt MUSI mieć jawną politykę wersjonowania:
  **SemVer 2.0.0**, z zastrzeżeniem, że w serii `0.y.z` **`y` wolno łamać
  zgodność** — i to jest seria, w której projekt jest dziś. Jedynym
  źródłem prawdy o numerze jest `version` w `Cargo.toml`; tag to ten numer
  z przedrostkiem `v`, a `release.yml` odrzuca tag, który się z nim nie zgadza
  (to zachowanie istnieje od F-110.6 i tutaj zyskuje uzasadnienie zapisane
  w specu). Numer wersji zmienia się **w osobnym commicie**, który nie robi nic
  innego — żeby dokładnie jeden commit odpowiadał na pytanie „skąd wzięła się
  ta wersja".
- **F-110.17** (MUST) Repozytorium MUSI mieć `CHANGELOG.md` w formacie
  **Keep a Changelog 1.1.0**, a treść notatek wydania MUSI być z niego brana,
  nie wpisywana ręcznie w workflow. `release.yml` MUSI odmówić taga, dla
  którego nie ma sekcji w `CHANGELOG.md` — z tego samego powodu, dla którego
  odmawia taga niezgodnego z `Cargo.toml`: wydanie bez opisu zmian jest
  błędem, który najtańszej wyłapać przed publikacją, a nie po.
  Ostrzeżenie o ponownym nadaniu uprawnień (F-110.13) zostaje doklejane do
  każdego wydania automatycznie, bo dotyczy każdej aktualizacji, nie jednej.

## 3. Odniesienia do dokumentacji

| Temat | Źródło |
|---|---|
| Helpery Cargo, brak sieci w `install` | Homebrew Formula Cookbook, „Package managers such as Cargo, Go modules, npm and Bundler download dependencies while `install` runs" |
| `std_cargo_args`, `std_cargo_fetch_args` | `Library/Homebrew/formula.rb:2206-2225` (Homebrew 7.0.9, sprawdzone lokalnie 2026-10-12) |
| Kwarantanna dotyczy casków, nie formuł | `Library/Homebrew/cask/quarantine.rb:19` — `QUARANTINE_ATTRIBUTE = "com.apple.quarantine"`; w kodzie formuł nie występuje |
| Tap jako repozytorium `homebrew-*` | Homebrew Formula Cookbook, §Taps |
| Granty TCC a aktualizacja przez Homebrew | yabai issue #1968 (wypowiedź opiekuna, 2024-02-13) — <https://github.com/koekeishiya/yabai/issues/1968> |
| Jak robią to inni | formuły `yabai` 7.1.25, `skhd` 0.3.9 (tap `koekeishiya/homebrew-formulae`), `borders` 1.9.0 (tap `FelixKratz/homebrew-formulae`), pobrane 2026-10-12 |
| Brak uwierzytelniania w serwerze | `specs/00-vision.md` §Non-goals; F-001 |

### Zbadany stan faktyczny (2026-10-12)

#### Co TCC trzyma przy podpisie ad-hoc — pomiar

`codesign -d --requirements -` na trzech binarkach na tej maszynie:

| Binarka | *Designated requirement* |
|---|---|
| `/opt/homebrew/Cellar/bsi/0.1.0/bin/bsi` (z tapa właściciela) | `cdhash H"6061fcea…"` |
| `target/release/mac-rdp-server` (nasza) | `cdhash H"e84ee64f…"` |
| nasza, przepodpisana `--sign - --identifier com.dawidadamski.mac-rdp-server` | `cdhash H"6410ee93…"` |

Trzeci wiersz jest tu najważniejszy: **jawny, stabilny identyfikator podpisu
nie trafia do DR w ogóle.** Przy podpisie ad-hoc DR to *wyłącznie* skrót
zawartości. Wniosek praktyczny: nie ma sensu kombinować z `--identifier`
w formule — to nic nie kupuje.

**Korekta wcześniejszej wersji tego speca.** Pierwsza wersja (2026-10-12)
tłumaczyła problem tym, że rustc wkleja hash do pola `Identifier`
(`mac_rdp_server-b7ab3a30d18fac95`). To prawda, ale **nie to** jest mechanizmem
— identyfikatora w DR nie ma. Konkluzja zostaje, powód był zły.

#### Druga, niezależna przyczyna: ścieżka

Homebrew instaluje każdą wersję w katalogu z numerem wersji, a w `bin/` kładzie
tylko symlink. Zmierzone: `/opt/homebrew/bin/bsi -> ../Cellar/bsi/0.1.0/bin/bsi`.
TCC zapisuje dla binarki bez bundle'a **ścieżkę**, więc po `brew upgrade`
powstaje nowy wpis, a stary zostaje na liście.

Opiekun `yabai` opisuje dokładnie to w issue #1968 (2024-02-13): każdy
`brew upgrade yabai` pyta o Accessibility od nowa i **dokłada kolejny wpis**,
starych nie usuwa. Jego dwa zdania warte zapamiętania: „Just remove stale entry
when upgrading" oraz „I'm not aware of any workaround as long as you install
using homebrew".

To znaczy, że **same dwie przyczyny działają niezależnie**: nawet gdyby F-106
dał stabilny DR (certyfikat self-signed), wersjonowana ścieżka Cellara dalej
tworzyłaby nowy wpis. Przy instalacji przez Homebrew ponowne nadanie uprawnień
jest nieuniknione — niezależnie od podpisu.

#### Kwarantanna: formuła ≠ cask (zmierzone)

Na gotowej binarce zainstalowanej formułą z GitHub Release:

- `xattr -l /opt/homebrew/Cellar/bsi/0.1.0/bin/bsi` → tylko
  `com.apple.provenance`, **brak `com.apple.quarantine`**;
- `INSTALL_RECEIPT.json` → `spec: stable`, `poured_from_bottle: false`, czyli
  plik przyszedł z tarballa wydania, nie z lokalnego builda ani z bottle;
- `codesign` → `Signature=adhoc`, `TeamIdentifier=not set`;
- binarka **działa** (`bsi --version` → `bsi 0.1.0`).

Kwarantannę nakłada **Homebrew Cask**, nie formuła (`cask/quarantine.rb`).
Stąd korekta: zdanie „artefakt pobrany caskiem dostaje kwarantannę, więc bez
notaryzacji Gatekeeper zablokuje" jest prawdziwe dla casków i `.dmg`,
ale **nie** dla formuły z gotową binarką. Ten sam błąd siedzi w `waves.md`
§Fala 5 („Tap Homebrew z formułą budującą ze źródeł omija to czysto") —
omija to także formuła z gotową binarką, i taniej dla użytkownika.

#### Jak robią to inni (pobrane 2026-10-12)

`yabai` 7.1.25, `skhd` 0.3.9, `borders` (JankyBorders) 1.9.0 — trzy narzędzia
CLI dla macOS, które potrzebują Accessibility i/lub nagrywania ekranu:

- wszystkie trzy to **formuły w prywatnym tapie**, nie caski;
- wszystkie podpisują się **ad-hoc, wprost w formule** — `codesign -fs -`
  (`yabai`, `skhd`), `codesign --force -s -` (`borders`);
- **żadne nie ma Developer ID ani notaryzacji**;
- **żadne nie wspomina uprawnień w `caveats`** — tam jest konfiguracja, usługa
  i logi; uprawnienia są w README/wiki;
- wszystkie trzy oferują launchd: `yabai --start-service`, `skhd --start-service`,
  `service do` w `borders`;
- README `yabai`: proces prosi o dostęp przy starcie i **trzeba go zrestartować
  po nadaniu uprawnienia** — czyli dokładnie to, co robi nasz
  `perms::ensure` + „restart the server" w `explain()`.

Czyli odpowiedź na pytanie właściciela („po co nam Apple Developer, inni po
prostu mówią, co nadać") jest zmierzona i brzmi: **nie jest nam potrzebne.**
Cała ta klasa narzędzi żyje na podpisie ad-hoc, a koszt — ponowne nadanie
uprawnień po aktualizacji — płaci i dokumentuje.

#### Stan repo w dniu pisania speca

Brak `README.md`, brak plików licencji, brak `SECURITY.md`. Repozytorium
`DawidAdamski/mac-rdp-server` jest **publiczne** i **bez opisu** — serwer bez
uwierzytelniania leży publicznie bez jednego zdania ostrzeżenia.
F-110.8–F-110.10 zamykają tę dziurę, nie zależą od niczego innego i decyzją
właściciela z 2026-10-12 idą **przed** resztą Fali 5.

## 4. Kryteria akceptacji

1. **AC-1** `brew install DawidAdamski/mac-rdp-server/mac-rdp-server`
   na **czystym koncie użytkownika** kończy się sukcesem, a `mac-rdp-server
   --version` wypisuje wersję zgodną z tagiem.
2. **AC-2** Ścieżka ze źródeł przechodzi: `brew install --HEAD` (albo
   `--build-from-source`) przy włączonej piaskownicy Homebrew, czyli zależność
   gitowa forku IronRDP została pobrana w fazie `fetch` (F-110.3). Dowód: log
   instalacji bez błędu sieci w `install`.
3. **AC-3** `brew audit --strict --online` i `brew test` przechodzą.
4. **AC-4** Na Macu, **na którym nigdy nie było Rusta**, instalacja ze ścieżki
   stabilnej (gotowa binarka) kończy się działającym `--version`, a
   `xattr -l` na zainstalowanym pliku **nie pokazuje `com.apple.quarantine`**.
   To jest dowód, że notaryzacja nie jest potrzebna — hipoteza z §3 sprawdzona
   na innej maszynie niż ta, na której ją zmierzono.
5. **AC-5** `brew uninstall` usuwa binarkę i nie zostawia plików poza
   katalogiem konfiguracyjnym użytkownika.
6. **AC-6** Osoba, która nie zna projektu, po samym README nadaje oba
   uprawnienia i łączy się z Windowsa — bez pytania autora. Test na żywym
   człowieku, nie myślowy.
7. **AC-7** `brew upgrade` na nowszy tag: na liście w Ustawieniach widać
   **stary wpis obok nowego**, serwer działa po usunięciu starego i nadaniu
   uprawnień na nowo, a instrukcja z F-110.13 opisuje dokładnie ten przebieg.
8. **AC-8** Serwer uruchomiony bez uprawnień wypisuje komunikat z F-110.14,
   zawierający zdanie o usunięciu starego wpisu po aktualizacji. Test
   jednostkowy sprawdza tę treść, ręczny — że faktycznie pojawia się w logu.
9. **AC-9** Strona GitHub Pages otwiera się pod publicznym adresem i jej treść
   zgadza się z README (F-110.11).
10. **AC-10** `SECURITY.md` i README ostrzegają o braku uwierzytelniania
    w miejscu, którego nie da się przeoczyć — nie w przypisie na końcu.
11. **AC-11** Tag, dla którego nie ma sekcji w `CHANGELOG.md`, **nie tworzy
    wydania** — workflow kończy się błędem z komunikatem wskazującym brakującą
    sekcję (F-110.17). Sprawdzalne lokalnie, tym samym skryptem, który używa
    workflow, bez tworzenia taga.
12. **AC-12** Notatki opublikowanego wydania zawierają treść sekcji
    `CHANGELOG.md` dla tej wersji **i** ostrzeżenie o uprawnieniach
    (F-110.16, F-110.17).

## 5. Poza zakresem

- **Developer ID, notaryzacja, bundle `.app`, stabilna tożsamość TCC** — F-106.
  Ten spec świadomie żyje z ograniczeniem i je dokumentuje. §3 zawęża przy tym
  F-106: sam certyfikat **nie wystarczy**, dopóki instalacja idzie w wersjonowaną
  ścieżkę Cellara.
- **LaunchAgent i podkomendy `install`/`uninstall`/`status`** — F-109. Formuła
  nie instaluje usługi; jeśli F-109 wejdzie wcześniej, dojdzie tu `service do`
  w formule jako osobne wymaganie (tak jak w `borders`).
- **Ikonka w pasku menu** — F-111.
- **Zgłoszenie do `homebrew-core`** — wymaga progu popularności; własny tap jest
  docelowy na tę falę.
- **Dystrybucja przez MacPorts, nix, `.pkg`, `.dmg`** — poza projektem.

## 6. Status

**Implemented** (2026-10-12). Napisany na wniosek właściciela („warto też od razu
pomyśleć o tapie dla brew"), zrewidowany tego samego dnia po jego pytaniach
i zaapprowany po rewizji („approve na spec").

**Odpowiedzi właściciela na trzy pytania z pierwszej wersji (2026-10-12):**

1. Nazwa tapa → **`homebrew-mac-rdp-server`** („1. homebrew-mac-rdp-server").
   F-110.1 poprawione; zgodne z jego własną konwencją (`homebrew-bsi`,
   `homebrew-floci-podman`).
2. Czy README/licencje/`SECURITY.md` przed Falą 5 → **tak** („Wypadało by").
   Zapisane w §3 i w `waves.md`.
3. Apple Developer 99 USD/rok → **nie**, z uzasadnieniem: „Z tego co widzę
   większości aplikacji na homebrew to po prostu mówią, że appka wymaga danych
   uprawnień i ludzie sami klikają dodając uprawnienia. Sprawdź sobie jak inni
   to robią." Sprawdzone — §3 „Jak robią to inni" potwierdza to pomiarem
   na trzech formułach. Konsekwencje wpisane w F-110.13, F-110.14, F-110.15
   i AC-7/AC-8; zakup konta nie wraca do rozważenia, dopóki ktoś nie poprosi
   o `.app` z menu (F-111) albo o caska.

Zmiany względem pierwszej wersji, objęte tym approve: F-110.2 (gotowa binarka
zamiast builda ze źródeł jako ścieżka główna), F-110.14 i F-110.15 (nowe),
korekty mechanizmu w §3.

**Kolejność realizacji** (dokumentacja pierwsza, decyzją właściciela):
F-110.9 → F-110.10 → F-110.8 → F-110.14 → F-110.15 → F-110.2…F-110.7 → F-110.11.

### Implemented (2026-10-12)

Wszystkie wymagania mają kod albo pliki w repo. Czego brakuje do **Verified**:
kryteriów, których nie da się sprawdzić bez opublikowanego wydania i drugiego
Maca — spisane niżej.

| Wymaganie | Stan | Dowód |
|---|---|---|
| F-110.1 | gotowe (nazwa z decyzji właściciela) | `packaging/homebrew/mac-rdp-server.rb`, README §Install |
| F-110.2 | gotowe | formuła: `head do` + `on_macos/on_arm/on_intel`, tarball składany przez `release.yml` |
| F-110.3 | **sprawdzone pomiarem** | na czystym `CARGO_HOME`: `cargo fetch --locked --target host-tuple` (tyle rozwija `std_cargo_fetch_args`) wciągnął fork do `$CARGO_HOME/git/db/ironrdp-*`, a `cargo install --offline --locked --root … --path .` skompilował `ironrdp-server v0.13.0 (git …ac4cd7db)` i zainstalował działającą binarkę. `CARGO_HOME` Homebrew wskazuje na cache wspólny dla fazy fetch i build (`package_manager_cache.rb:54`) |
| F-110.4 | gotowe | blok `test`: `--version`, `--help` zawiera `--check-permissions`, brakujący plik konfiguracyjny daje kod 1 i „reading config file" — wszystko offline, bez TCC, bez portu |
| F-110.5 | gotowe | `caveats` zaczyna się od braku uwierzytelniania, ma oba uprawnienia, akapit o ponownym nadaniu po `brew upgrade` i ścieżkę przykładowej konfiguracji |
| F-110.6 | gotowe | `packaging/RELEASING.md` (procedura + wariant ręczny + znane ostre krawędzie) |
| F-110.7 | gotowe | `.github/workflows/release.yml`: tag → build obu architektur → podpis ad-hoc → tarballe → `render-formula.sh` → GitHub Release. Odrzuca tag niezgodny z `version` w `Cargo.toml` (sprawdzone lokalnie na `v0.1.0` i `v9.9.9`) |
| F-110.8 | gotowe | `README.md` |
| F-110.9 | gotowe | `LICENSE-MIT`, `LICENSE-APACHE` (tekst kanoniczny z apache.org) |
| F-110.10 | gotowe | `SECURITY.md` |
| F-110.11 | gotowe i opublikowane | `docs/` + `packaging/build-docs.py`, CI biegnie z `--check`; strona żyje pod <https://dawidadamski.github.io/mac-rdp-server/> |
| F-110.12 | **nie zrobione** | właściciel nie zdecydował o donacjach (MAY) |
| F-110.13 | gotowe | README §„When it stops working after an update", `caveats`, notatki wydania w `release.yml` |
| F-110.14 | gotowe | `src/perms.rs`, test `f110_14_instructions_cover_a_stale_entry_after_an_upgrade` |
| F-110.15 | gotowe | `--check-permissions`, trzy testy `f110_15_*` |
| F-110.16 | gotowe | polityka w `CHANGELOG.md` §Wersjonowanie, wymuszana przez `release.yml` |
| F-110.17 | gotowe | `CHANGELOG.md`, `packaging/release-notes.sh`, test negatywny w CI |

**Co zostało sprawdzone prawdziwym `brew` (2026-10-12).** Wyrenderowana formuła
została wstawiona do lokalnego tapa i przeszła:

- `brew audit --strict` → **0**,
- `brew style` → **0**, „no offenses detected".

Audyt wykrył przy tym dwa błędy kolejności bloków, oba poprawione: `head` musi
stać przed `on_macos`, a `depends_on` przed `on_macos`. Bez tego kroku poszłyby
do tapa.

`brew audit --online` i `brew test` wymagają istniejącego wydania — zostają na
moment publikacji.

**Czego świadomie nie dokończono.** `brew install --HEAD` z lokalnego tapa
zostało uruchomione i **przerwane**, bo Homebrew zapowiedział aktualizację
trzech pakietów na maszynie właściciela (`llvm`, `pkgconf`, `sqlite`). Zdążył
się zaktualizować `pkgconf` 3.0.7 → 3.0.8 (stary keg został, więc jest
odwracalne). Pełny test instalacji przez `brew` wymaga zgody właściciela na te
aktualizacje — stąd AC-2 zostaje otwarte, mimo że mechanizm z F-110.3 jest już
udowodniony na poziomie `cargo`.

**Uzupełnienie 2026-10-13 — wersjonowanie.** F-110.16 i F-110.17 dopisane
i **Approved** na wniosek właściciela tego samego dnia („Wprowadźmy też
wersjonowanie jeśli jeszcze nie ma"). Powstały przed tagiem `v0.1.0`, bo
inaczej pierwsze wydanie wyszedłby bez jednego zdania o tym, co się w nim
zmieniło, a notatki były wpisane na sztywno w `release.yml`.

**Co zweryfikowała publikacja (2026-10-13).** Właściciel zezwolił na push
(„wypychaj byle nie bylo personalnych danych"), więc przed wypchnięciem repo
zostało przeskanowane: brak adresów e-mail, brak ścieżek `/Users/…`, brak
kluczy, logów i plików binarnych w całej historii, a jedyne trafienia na
`dawidadamski` to publiczny login GitHuba. Po pushu:

- **AC-9 spełnione.** <https://dawidadamski.github.io/mac-rdp-server/> i
  `…/security.html` zwracają HTTP 200, tytuł i nagłówki zgadzają się z README,
  a zgodność treści pilnuje `--check` w CI. Pages: `main`, katalog `/docs`.
- **AC-10 spełnione.** Blok ostrzeżenia o braku uwierzytelniania stoi jako
  pierwsza treść i w README, i na stronie, i w `SECURITY.md`.
- **Private vulnerability reporting jest włączone** — kanał, który wskazuje
  `SECURITY.md`, działa.
- **CI na macOS przechodzi z forkiem jako zależnością git** (7 min 27 s) —
  pierwszy dowód, że `[patch.crates-io]` z ADR-0014 działa poza tą maszyną.

Pierwszy build Pages padł (`No such file or directory … /github/workspace/docs`),
bo Pages włączono, zanim `docs/` trafiło na origin. Kolejny, po pushu, przeszedł
w 41 s. Sam push odsłonił też jedną nieprawdę w opublikowanej treści: README
reklamował `brew install` z tapa, który nie istnieje do pierwszego wydania —
dopisana nota o tym kieruje tymczasem na budowę ze źródeł.

**Do Verified brakuje:** AC-1, AC-2, AC-4, AC-5, AC-6, AC-7 (ręczne, część
wymaga drugiego Maca i opublikowanego wydania). AC-3 zrobione w części offline,
AC-8 ma test jednostkowy i czeka na potwierdzenie w logu.

**Co wymaga osobnej zgody właściciela, bo wychodzi na zewnątrz:**

1. ~~`git push`~~ — **zrobione 2026-10-13**, 54 commity, `f3beacb..dde428c`;
2. utworzenie repozytorium tapa `DawidAdamski/homebrew-mac-rdp-server` — wciąż
   nie istnieje, więc instrukcja instalacji z AC-1 nie zadziała;
3. tag `v0.1.0` i opublikowanie wydania (uruchamia `release.yml`);
4. ~~**Settings → Pages → main, katalog `/docs`**~~ — **zrobione**;
5. ~~**Settings → Security → Private vulnerability reporting**~~ — **zrobione**;
6. opis repozytorium, którego wciąż nie ma (`description`, `homepage`
   i `topics` są puste).
