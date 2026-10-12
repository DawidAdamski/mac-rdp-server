# F-110 — Dystrybucja przez Homebrew, dokumentacja i strona projektu

## 1. Cel

Żeby obca osoba na czystym Macu zainstalowała serwer jedną komendą, wiedziała
z czytelnej strony, co nadać w Ustawieniach i czym to grozi w sieci — bez
toolchaina Rusta w głowie i bez pytania autora.

## 2. Wymagania

### Tap i formuła

- **F-110.1** (MUST) Dystrybucja MUSI iść przez **własny tap Homebrew**
  w repozytorium `DawidAdamski/homebrew-tap`, instalowany jako:
  ```
  brew tap dawidadamski/tap
  brew install dawidadamski/tap/mac-rdp-server
  ```
  Nazwa `homebrew-tap` (a nie `homebrew-mac-rdp-server`) MUSI być użyta, żeby
  drugi program projektu nie wymagał drugiego tapa.
- **F-110.2** (MUST) Formuła MUSI budować **ze źródeł**, z tarballa
  przypiętego tagiem wydania i sumą `sha256`. Nie wolno jej wskazywać gałęzi.
- **F-110.3** (MUST) Formuła MUSI definiować metodę `fetch` i używać helperów
  Homebrew, nie własnej listy argumentów:
  ```ruby
  def fetch
    system "cargo", "fetch", *std_cargo_fetch_args
  end

  def install
    system "cargo", "install", *std_cargo_args
  end
  ```
  Powód jest twardy: `install` w Homebrew biegnie **bez dostępu do sieci**,
  a `std_cargo_args` dokłada `--offline`, gdy `fetch` jest zdefiniowane.
  Nasz `Cargo.lock` wskazuje **zależność gitową** (fork IronRDP, ADR-0014),
  więc `cargo fetch` MUSI ściągnąć także repozytorium forku — bez tego
  `install` padnie w trybie offline. To wymaganie jest testowalne i MUSI być
  sprawdzone realnym `brew install --build-from-source` (AC-2).
- **F-110.4** (MUST) Formuła MUSI mieć blok `test`, który uruchamia binarkę
  i sprawdza wyjście `--version`. Test nie może wymagać uprawnień TCC ani
  otwartego portu.
- **F-110.5** (MUST) Formuła MUSI wypisać `caveats` zawierające: dwa
  uprawnienia do nadania (Screen Recording, Accessibility), ścieżkę do pliku
  konfiguracyjnego, ostrzeżenie o braku uwierzytelniania (F-110.10) oraz
  informację z F-110.13 o konieczności ponownego nadania uprawnień po
  aktualizacji.
- **F-110.6** (MUST) Wydanie MUSI być opisanym, powtarzalnym procesem: tag
  `vX.Y.Z` → GitHub Release → aktualizacja `url` i `sha256` w formule.
  Procedura MUSI być zapisana w repo, nie w pamięci autora.
- **F-110.7** (SHOULD) Aktualizacja formuły POWINNA być zautomatyzowana
  workflow'em uruchamianym tagiem wydania, żeby wersja w tapie nie rozjeżdżała
  się z wydaniem.

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
- **F-110.13** (MUST) Dokumentacja MUSI opisywać zachowanie uprawnień przy
  aktualizacji, zgodnie ze stanem faktycznym z §3: po `brew upgrade` uprawnienia
  trzeba nadać **ponownie**, a przełącznik w Ustawieniach może przy tym nadal
  wyglądać na włączony. Dopóki F-106 nie da stabilnej tożsamości podpisu, to
  jest udokumentowane ograniczenie, nie błąd do ukrycia.

## 3. Odniesienia do dokumentacji

| Temat | Źródło |
|---|---|
| Helpery Cargo, brak sieci w `install` | Homebrew Formula Cookbook, „Package managers such as Cargo, Go modules, npm and Bundler download dependencies while `install` runs" |
| `std_cargo_args`, `std_cargo_fetch_args` | `Library/Homebrew/formula.rb:2206-2225` (Homebrew 7.0.9, sprawdzone lokalnie 2026-10-12) |
| Tap jako repozytorium `homebrew-*` | Homebrew Formula Cookbook, §Taps |
| Klucz grantu TCC = *designated requirement* podpisu | `waves.md` §Fala 5 pkt 2; raporty społeczności (skhd, runtime Node) |
| Brak uwierzytelniania w serwerze | `specs/00-vision.md` §Non-goals; F-001 |

### Zbadany stan faktyczny (2026-10-12)

**Nasza binarka jest podpisana ad-hoc i jej tożsamość zmienia się między
buildami.** Zmierzone na tej maszynie, `codesign -dvvv`:

| Build | `Identifier` | `CDHash` |
|---|---|---|
| `target/debug/mac-rdp-server` | `mac_rdp_server-7139bc4f053fe608` | `3a6bee57…` |
| `target/release/mac-rdp-server` | `mac_rdp_server-b7ab3a30d18fac95` | `e84ee64f…` |

W obu `Signature=adhoc`, `TeamIdentifier=not set`. Zmienia się **nie tylko**
skrót zawartości, ale i sam identyfikator podpisu — rustc dokłada do niego hash.
TCC nie ma więc czego trzymać: po każdej przebudowie, czyli po każdym
`brew upgrade` budującym ze źródeł, jest to dla systemu **inny program**.

**Skutek, na który trzeba uważać w instrukcji.** Zgłoszenia społeczności
(skhd na macOS 15+, runtime Node) opisują zgodnie, że przełącznik w Ustawieniach
→ Prywatność potrafi **nadal pokazywać nadane uprawnienie**, choć zdarzenia nie
płyną: w bazie TCC zostaje wpis z nieaktualnym wymaganiem podpisu. To jest ten
sam objaw, który w tym projekcie wraca po każdej aktualizacji iTerma (patrz
pamięć `iterm-update-breaks-accessibility`). Instrukcja MUSI uczyć rozpoznawać
go po objawie, nie po przełączniku: usuń wpis z listy i dodaj ponownie.

**Stan repo w dniu pisania speca.** Brak `README.md`, brak plików licencji,
brak `SECURITY.md`. Repozytorium `DawidAdamski/mac-rdp-server` jest **publiczne**
i **bez opisu** — serwer bez uwierzytelniania leży publicznie bez jednego zdania
ostrzeżenia. F-110.8–F-110.10 zamykają tę dziurę i są najpilniejszą częścią
tego speca, niezależnie od Homebrew.

**Dlaczego formuła, a nie cask z gotową binarką.** Artefakt pobrany
przeglądarką albo caskiem dostaje atrybut kwarantanny, a bez notaryzacji
(Apple Developer, 99 USD/rok) Gatekeeper go zablokuje. Formuła budująca na
maszynie użytkownika kwarantanny nie dostaje — to jest ścieżka, która działa
dziś, bez konta deweloperskiego. Cena: użytkownik musi mieć toolchain Rusta
(`depends_on "rust" => :build` załatwia to przez Homebrew) i czeka na
kompilację.

## 4. Kryteria akceptacji

1. **AC-1** `brew tap dawidadamski/tap && brew install dawidadamski/tap/mac-rdp-server`
   na **czystym koncie użytkownika** kończy się sukcesem, a `mac-rdp-server
   --version` wypisuje wersję zgodną z tagiem.
2. **AC-2** Instalacja przechodzi z `--build-from-source` przy włączonej
   piaskownicy Homebrew, czyli zależność gitowa forku IronRDP została pobrana
   w fazie `fetch` (F-110.3). Dowód: log instalacji bez błędu sieci w `install`.
3. **AC-3** `brew audit --strict --online dawidadamski/tap/mac-rdp-server`
   i `brew test` przechodzą.
4. **AC-4** `brew uninstall` usuwa binarkę i nie zostawia plików poza
   katalogiem konfiguracyjnym użytkownika.
5. **AC-5** Osoba, która nie zna projektu, po samym README nadaje oba
   uprawnienia i łączy się z Windowsa — bez pytania autora. Test na żywym
   człowieku, nie myślowy.
6. **AC-6** `brew upgrade` na nowszy tag: serwer działa po ponownym nadaniu
   uprawnień, a instrukcja z F-110.13 opisuje dokładnie to, co się stało
   (łącznie z przełącznikiem, który może wyglądać na włączony).
7. **AC-7** Strona GitHub Pages otwiera się pod publicznym adresem i jej treść
   zgadza się z README (F-110.11).
8. **AC-8** `SECURITY.md` i README ostrzegają o braku uwierzytelniania
   w miejscu, którego nie da się przeoczyć — nie w przypisie na końcu.

## 5. Poza zakresem

- **Podpis Developer ID, notaryzacja, bundle `.app`, stabilna tożsamość TCC** —
  F-106. Ten spec świadomie żyje z ograniczeniem i je dokumentuje.
- **LaunchAgent i podkomendy `install`/`uninstall`/`status`** — F-109. Formuła
  nie instaluje usługi; jeśli F-109 wejdzie wcześniej, dojdzie tu
  `service do` w formule jako osobne wymaganie.
- **Ikonka w pasku menu** — F-111.
- **Zgłoszenie do `homebrew-core`** — wymaga progu popularności; własny tap jest
  docelowy na tę falę.
- **Dystrybucja przez MacPorts, nix, `.pkg`** — poza projektem.

## 6. Status

**Draft** (2026-10-12). Napisany na wniosek właściciela („warto też od razu
pomyśleć o tapie dla brew"). Czeka na approve.

**Do decyzji właściciela przy approve:**
1. Nazwa tapa: `homebrew-tap` (rekomendacja, F-110.1) czy
   `homebrew-mac-rdp-server`?
2. Czy F-110.8–F-110.10 (README, licencje, `SECURITY.md`) wyciągamy **przed**
   Falą 5 jako osobną, małą pozycję? Repo jest publiczne już dziś i nie ma ani
   zdania ostrzeżenia, a to nie zależy od niczego innego.
3. Czy w ogóle wchodzimy w Apple Developer (99 USD/rok). Odpowiedź „nie"
   zamraża F-110.13 jako trwałe ograniczenie i upraszcza F-106; odpowiedź
   „tak" otwiera cask z gotową binarką i granty TCC przeżywające aktualizacje.
