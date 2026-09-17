# ADR-0011: Strategia F-200 — własny fork IronRDP zamiast stosu od zera

- **Status:** Proposed
- **Data:** 2026-09-17
- **Dotyczy:** F-200, ADR-0001 (silnik), `specs/02-ms-rdpbcgr.md` §7
- **Powód powstania:** decyzja kierunkowa właściciela z 2026-09-17 po przeglądzie
  MS-RDPBCGR: „teraz używamy IronRDP, później zrobimy to lepiej; myślę
  o forku i własnym podejściu, w pełni prowadzonym przez AI”.

## Kontekst

ADR-0001 przyjął `ironrdp-server` jako silnik MVP i odłożył własną
implementację MS-RDPBCGR (F-200) jako cel długoterminowy. Szkic
`02-ms-rdpbcgr.md` §7 opisał F-200 jako **nowy stos od zera** (moduł sans-IO
`src/rdp/`, 13 etapów, pierwszy piksel dopiero w etapie E8).

Przegląd z 2026-09-17 (`02-ms-rdpbcgr.md` §4) znalazł w IronRDP 0.13.0 kilka
odstępstw od specyfikacji. Część z nich ma realny wpływ na użytkownika: brak
timeoutu, przyciski boczne myszy działające jak lewy/prawy klik, Refresh Rect
bez odmalowania. Są dwa sposoby, żeby je usunąć i jednocześnie iść w stronę F-200:

1. pisać własny stos i czekać z poprawkami, aż dojrzeje,
2. przejąć kod IronRDP we własny fork i poprawiać go od razu, stopniowo
   zamieniając go w „naszą implementację”.

Ten ADR proponuje wariant 2 jako **pierwszą propozycję** i świadomie nie
zamyka tematu, jak F-200 ma wyglądać ostatecznie.

## Zbadany stan faktyczny (2026-09-17)

### Licencja i repozytorium

| Fakt | Wartość | Źródło |
|---|---|---|
| Licencja crate'ów | `MIT OR Apache-2.0` | `ironrdp-server-0.13.0/Cargo.toml`, `ironrdp-acceptor-0.10.0/Cargo.toml` |
| Pliki licencji w repo | `LICENSE-APACHE`, `LICENSE-MIT` (GitHub rozpoznaje tylko Apache-2.0) | `gh api repos/Devolutions/IronRDP` |
| Repo | `github.com/Devolutions/IronRDP`, gałąź `master`, 3177 gwiazdek, 273 forki | jw. |
| Układ | monorepo, 69 katalogów w `crates/`, do tego `fuzz/`, `ffi/`, `web-client/`, `xtask/` | `gh api …/contents` |
| Wytyczne dla agentów AI | repo ma własne `AGENTS.md` i `CLAUDE.md` („Auto-Pilot Workflow”, wymagane linki do specyfikacji przy zmianach protokołu) | `AGENTS.md` na `master` |

### Skąd pochodzą nasze crate'y

Wszystkie crate'y IronRDP z naszego `Cargo.lock` poza jednym pochodzą
z **jednego commita upstream**. Odczytane z `.cargo_vcs_info.json`:

| Crate | Wersja | Commit |
|---|---|---|
| server, acceptor, pdu, connector, svc, core, graphics, session, cliprdr, dvc | 0.13.0 / 0.10.0 / 0.9.0 / … | `11a0810cfbbabd8b8023875a05e3041216d4b01b` (2026-07-10, „chore(release): prepare for publishing (#1364)”) |
| `ironrdp-bulk` | 0.1.1 | `e3dc0e70d0…` |

W `Cargo.lock` są też `ironrdp-ainput`, `-async`, `-blocking`, `-displaycontrol`,
`-echo`, `-error`, `-rdpsnd` i `-tokio`. Przy forku trzeba je objąć tym samym
`[patch]`, inaczej w drzewie znajdą się dwie kopie typów.

### Co upstream zmienił po naszej wersji

`master` @ `cd1d484e406459e9296d577fa86ac2f14a3098a6` (2026-09-16) jest
**405 commitów** przed `11a0810`. Numery wersji w `Cargo.toml` na `master` to
nadal `0.13.0` / `0.10.0`, czyli zmiany nie są jeszcze wydane.
`ironrdp-server/src/server.rs` urósł z 1870 do 4842 linii.

| Ustalenie z `02-ms-rdpbcgr.md` §4 | Stan na `master` |
|---|---|
| P-03 `TS_POINTERX` → lewy/prawy | **Naprawione** — `MouseButton::X1/X2` z cytatem MS-RDPBCGR (`handler.rs:243-263`) |
| P-07 Monitor Layout: prawy i dolny brzeg o 1 za daleko | **Naprawione**: `width - 1`, `height - 1` (`connection.rs:923-924`). Jeden monitor na sztywno i `pduSource` — nie sprawdzone |
| P-02 brak timeoutu | **Częściowo** — `FINALIZE_TIMEOUT = 30 s` (`server.rs:91`), `CANDIDATE_NEGOTIATION_TIMEOUT = 10 s` (`:1147`) i wypieranie połączeń (`EVICTION_GRACE`, `:1119`). Keepalive w trakcie sesji — nie sprawdzone |
| (poza §4) `MouseRelPdu` z przyciskiem gubił deltę ruchu | **Naprawione** (`handler.rs:268-275`, komentarz z 2.2.8.1.1.3.1.1.7). Nasza część P-16 (`cgevent.rs`) zostaje |
| P-06 Refresh Rect bez odmalowania | **Bez zmian** (`server.rs:3751-3755`) |
| P-13 Order Caps bez `NEGOTIATEORDERSUPPORT`, `keyboard_function_key = 128` | **Bez zmian** (`capabilities.rs:54`, `:85`) |
| P-17 `USER_CHANNEL_ID = 1002`, `called_connect_id = 1` | **Bez zmian** (`connection.rs:26`, `:737`) |

Wniosek: **upstream aktywnie usuwa te same błędy**, które znaleźliśmy. Fork
regularnie synchronizowany z upstreamem dostaje te poprawki za darmo. Fork,
który odjedzie, musi je robić sam.

## Decyzja (proponowana)

### Etap 0 — teraz (bez zmian)

Zostajemy na `ironrdp-server = "=0.13.0"` z crates.io. Poprawki z Fali 3
(`02-ms-rdpbcgr.md` §6) robimy **po naszej stronie granicy `session`**
wszędzie, gdzie się da.

### Etap 1 — fork, gdy pierwsza poprawka wymaga zmiany w IronRDP

1. **Fork na GitHubie** (`<konto właściciela>/IronRDP`), gałąź robocza
   `mac-rdp/main`.
2. **Baza forka:** commit z `master` z chwili założenia, **nie** tag 0.13.0 —
   bo `master` ma już poprawki P-02 (częściowo), P-03 i P-07. Taki przeskok jest
   jednak zmianą API (server.rs ×2,6), więc przed przełączeniem trzeba:
   przejrzeć `CHANGELOG`, zaktualizować ADR-0001 i przejść pełną weryfikację
   Fali 3 od nowa.
3. **Konsumpcja w naszym repo:** `[patch.crates-io]` dla **wszystkich**
   crate'ów `ironrdp-*` z `Cargo.lock`, z `git = …` i **`rev = "<pełny sha>"`**.
   Pinowanie commitem jest odpowiednikiem reguły `=x.y.z` z `CLAUDE.md`.
   `Cargo.lock` zostaje w repo.
4. **Granica bez zmian:** tylko `src/session/` importuje `ironrdp_*`
   (F-001.12). Fork niczego tu nie rozluźnia.

### Etap 2 — zasady pracy w forku

1. **Spec-driven obowiązuje także w forku.** Specy i ADR-y mieszkają
   **w tym repo**, a nie w forku. Każdy commit w forku odwołuje się do ID
   wymagania (`F-2xx.N`) albo ustalenia z przeglądu (`P-xx`), tak jak
   commity tutaj.
2. **Prowadzenie przez AI.** Agent implementuje zmiany w forku według
   zatwierdzonych speców. Przestrzega zarówno naszego `CLAUDE.md`, jak
   i `AGENTS.md`/`CLAUDE.md` upstreamu, w tej kolejności. Właściciel zatwierdza
   specy i robi weryfikację ręczną — tak samo jak dziś.
3. **Każda poprawka protokołu ma test** z bajtami ze specyfikacji
   (`specs/ref/MS-RDPBCGR-v20260309.txt`, §4) albo z przechwycenia prawdziwego
   klienta. Dekodery wejścia dostają cel w istniejącym `fuzz/` upstreamu.
4. **CI w forku:** to, co upstream już ma, plus nasz zestaw testów
   z `tests/` uruchamiany na forku. Bump `rev` w naszym repo następuje tylko
   przy zielonym CI.
5. **Synchronizacja z upstream** to świadoma decyzja, jak bump wersji:
   merge `master` upstreamu, przegląd zmian w crate'ach, których używamy,
   i wpis w sekcji „Dziennik synchronizacji” poniżej.
6. **Poprawki ogólne** (niezależne od macOS) zgłaszamy upstream jako PR
   — o ile właściciel tak zdecyduje dla danej zmiany. Każda przyjęta poprawka
   to mniejszy dług przy następnej synchronizacji.

### Etap 3 — F-200 przez ewolucję

Etapy E0–E12 z `02-ms-rdpbcgr.md` §7.4 **zmieniają rolę**. Zamiast listy
modułów do napisania od zera stają się **listą audytów zgodności** crate'ów
forka: dla każdego etapu test zgodności z MS-RDPBCGR, a tam, gdzie test pada,
poprawka w forku. F-200 jest osiągnięte, gdy lista zgodności z §7.5 jest
zielona na forku, a każdy etap ma test.

Czy fork z czasem odetnie się od upstreamu i zostanie przepisany (np. na
sans-IO), to **osobna, późniejsza decyzja** — nowy ADR.

## Konsekwencje

**Plus**
- Poprawki odstępstw od specyfikacji trafiają do użytkownika od razu, bez
  czekania na wydanie upstream ani na nowy stos.
- Pierwszy piksel jest od pierwszego dnia — koszt z ADR-0001 („kilka tygodni
  przed widocznym efektem”) znika.
- Dostajemy za darmo poprawki upstreamu (§„Co upstream zmienił”), fuzzing
  i CI.
- Granica `session` i test architektury zostają bez zmian, więc późniejsza
  podmiana forka na coś innego jest nadal zmianą jednego modułu.

**Minus / dług**
- **Utrzymanie dużej bazy kodu:** sam `server.rs` na `master` ma 4842 linie.
  Synchronizacja przy tym tempie zmian (405 commitów w ~2 miesiące) będzie
  pracochłonna. Im dłużej fork odjeżdża, tym drożej.
- **Zależność gitowa:** build wymaga dostępu do GitHuba. Dla tapu Homebrew
  budującego ze źródeł (Fala 5) to akceptowalne, ale formuła musi to
  uwzględnić. Crate'a nie publikujemy (`publish = false`), więc crates.io
  nie jest ograniczeniem.
- **Licencja:** fork musi zachować `LICENSE-MIT`, `LICENSE-APACHE` i noty
  autorskie Devolutions. Przy F-110 (dystrybucja) trzeba to wymienić
  w dokumentacji licencji.
- Przeskok bazy na `master` to realna zmiana API i powód do pełnej ponownej
  weryfikacji Fali 3 (Etap 1 pkt 2).

## Odrzucone alternatywy

- **Stos od zera teraz** (`02-ms-rdpbcgr.md` §7.2, sans-IO) — **odłożone, nie
  skreślone.** Pierwszy obraz dopiero w etapie E8, a w tym czasie użytkownik
  żyje z błędami z §4. Wraca jako osobny ADR, jeśli fork okaże się
  nieutrzymywalny.
- **Wyłącznie PR-y do upstreamu, bez forka** — nie daje kontroli nad
  terminem wydania (`master` ma od dwóch miesięcy niewydane poprawki).
  Zostaje jako **uzupełnienie** forka (Etap 2 pkt 6).
- **Vendoring** (kopia crate'ów do `vendor/` w naszym repo) — traci historię
  gita i utrudnia synchronizację z upstreamem. Jedyna zaleta, build bez sieci,
  nie jest nam dziś potrzebna.
- **Czekanie na następne wydanie crates.io** — rozsądny krok pośredni, ale nie
  daje kontroli. Jeżeli wydanie z poprawkami z `master` pojawi się przed
  założeniem forka, **bierzemy je jako bazę Etapu 1** zamiast gołego commita.

## Otwarte pytania (do approve)

1. Konto lub organizacja, na której powstaje fork.
2. Baza forka: `master` w chwili założenia (rekomendacja) czy tag 0.13.0?
3. Czy Etap 1 startuje od razu, czy dopiero przy pierwszej poprawce, której
   nie da się zrobić po naszej stronie (rekomendacja: przy pierwszej takiej
   poprawce — dziś jest nią P-06, odmalowanie po Refresh Rect)?

## Dziennik synchronizacji

| Data | Nasz `rev` | Upstream | Uwagi |
|---|---|---|---|
| — | — | — | Fork jeszcze nie istnieje |
