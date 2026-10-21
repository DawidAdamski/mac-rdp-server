# F-112 — Domyślny plik konfiguracyjny

## 1. Cel

Ustawienia dają się zapamiętać: serwer czyta plik konfiguracyjny z ustalonej
ścieżki, bez podawania `--config` przy każdym uruchomieniu.

## 2. Wymagania

### Skąd czytamy

- **F-112.1** (MUST) Gdy nie podano `--config`, serwer MUSI przeczytać
  `~/.config/mac-rdp-server.toml`, jeżeli ten plik istnieje. Ścieżka jest
  **jedna** — nie ma listy przeszukiwania.
- **F-112.2** (MUST) Precedencja MUSI być: **flagi CLI > plik wskazany przez
  `--config` > plik domyślny > wartości domyślne**. Podanie `--config`
  **wyłącza** plik domyślny: czytamy jeden plik albo drugi, nigdy oba. To jest
  po to, żeby dało się odpowiedzieć na pytanie „skąd on wziął tę wartość"
  bez debugowania.
- **F-112.3** (MUST) `~` MUSI być rozwijane przez zmienną `HOME`. Brak `HOME`
  oznacza „nie ma pliku domyślnego", a nie błąd ani panikę.

### Jak reagujemy

- **F-112.4** (MUST) **Brak** pliku domyślnego jest cichy — to normalny stan,
  nie ostrzeżenie.
- **F-112.5** (MUST) Plik domyślny, którego **nie da się przeczytać albo
  sparsować**, JEST błędem: serwer kończy się kodem różnym od zera
  z komunikatem wskazującym plik i powód. Dokładnie tak, jak dziś zachowuje
  się `--config` (F-001.4, test formuły w F-110.4). Cicha praca na domyślnych
  po tym, jak użytkownik napisał plik i zrobił w nim literówkę, jest gorsza
  niż zatrzymanie się.
- **F-112.6** (SHOULD) POWINNA istnieć flaga wyłączająca czytanie pliku
  domyślnego (`--no-config`), żeby zły plik dało się ominąć bez kasowania go.
- **F-112.7** (MUST) Log startowy MUSI mówić, **skąd** przyszła konfiguracja:
  ścieżka pliku albo informacja, że użyto wartości domyślnych. Dziś log
  startowy wypisuje wynik, nie źródło — i to jest powód, dla którego problem
  z 2026-10-15 trzeba było sprawdzać eksperymentem.

### Dokumentacja

- **F-112.8** (MUST) README, `caveats` formuły Homebrew i komentarz nagłówkowy
  w `packaging/mac-rdp-server.toml` MUSZĄ mówić dokładnie to, co robi kod.
  To wymaganie istnieje, bo właśnie na tym się przejechaliśmy: `caveats`
  pokazywało `cp` przykładu do `~/.config/mac-rdp-server.toml` i kończyło,
  jakby ta ścieżka była czytana (poprawione 2026-10-15, commit `ec6a477`).

### Testy

- **F-112.9** (MUST) Testy jednostkowe MUSZĄ pokrywać precedencję z F-112.2
  w każdej kombinacji (flaga + plik wskazany, flaga + plik domyślny, sam plik
  domyślny, nic), a także F-112.3 (brak `HOME`) i F-112.5 (plik domyślny nie do
  sparsowania). Rozwiązywanie ścieżki MUSI dać się przetestować bez dotykania
  prawdziwego `$HOME` użytkownika.

## 3. Odniesienia do dokumentacji

- F-001.4 — istniejąca precedencja „flagi > TOML > domyślne"; to wymaganie ją
  rozszerza o jeden poziom, nie zmienia jej sensu.
- F-110.4 — test formuły Homebrew asertuje, że brakujący plik podany przez
  `--config` daje kod 1 i komunikat „reading config file". F-112.5 utrzymuje
  tę samą regułę dla pliku domyślnego, który istnieje, ale jest zepsuty.
- `specs/waves.md` §Pomiar 2026-10-15 — pomiar, z którego to wymaganie
  wynikło, i decyzja właściciela o ścieżce.

### Zbadany stan faktyczny (2026-10-15)

`Config::from_cli` (`src/config.rs:161`) woła `FileConfig::load` wyłącznie dla
`Some(path)`; dla `None` bierze `FileConfig::default()`. Żadnej domyślnej
ścieżki nie ma. Sprawdzone zainstalowaną binarką `0.1.1`, nie lekturą kodu:
plik z `bind_addr = "0.0.0.0"` i `port = 33891` położony
w `~/.config/mac-rdp-server.toml` → serwer startuje na `addr=127.0.0.1:3389`;
**ten sam** plik podany przez `--config` → `addr=0.0.0.0:33891`.

## 4. Kryteria akceptacji

1. **AC-1** Plik `~/.config/mac-rdp-server.toml` z `bind_addr = "0.0.0.0"`
   i `port = 33891`, serwer uruchomiony **bez żadnych flag** → log startowy
   pokazuje `addr=0.0.0.0:33891`. To jest dokładnie ten pomiar, który dziś
   wychodzi odwrotnie.
2. **AC-2** Ten sam plik, ale uruchomienie z `--bind-addr 127.0.0.1` → adres
   z flagi wygrywa, port z pliku zostaje.
3. **AC-3** Ten sam plik plus `--config <inny plik>` → wartości pochodzą
   **wyłącznie** z pliku wskazanego; plik domyślny nie jest nawet czytany
   (sprawdzalne tak, że plik domyślny zawiera klucz, którego nie ma w drugim).
4. **AC-4** Brak pliku domyślnego → serwer startuje na wartościach domyślnych,
   bez ostrzeżenia w logu.
5. **AC-5** Plik domyślny z literówką (np. `bind_addr = "nie-adres"`) → serwer
   **nie startuje**, kod wyjścia ≠ 0, komunikat wskazuje ścieżkę i powód.
6. **AC-6** Log startowy pozwala odpowiedzieć na pytanie „skąd ta wartość":
   widać w nim użytą ścieżkę albo informację o wartościach domyślnych.
7. **AC-7** README, `caveats` formuły i nagłówek przykładowego pliku opisują
   to samo zachowanie co kod — sprawdzone przez przeczytanie wszystkich trzech
   po implementacji, nie przez pamięć.

Kryteria AC-1 … AC-5 są sprawdzalne bez GUI i bez uprawnień TCC, więc MUSZĄ
mieć odpowiadające im testy automatyczne; AC-6 i AC-7 są do przeczytania.

## 5. Poza zakresem

- **Zapisywanie pliku przez program.** Ten spec tylko czyta. Zapis (np. żeby
  przełącznik z F-111 zapamiętał stan) to osobna decyzja — kto jest
  właścicielem pliku pisanego ręcznie przez użytkownika, co się dzieje
  z komentarzami w TOML-u po przepisaniu, i czy w ogóle chcemy, żeby program
  nadpisywał plik, który użytkownik uważa za swój.
- **Katalog `~/Library/Application Support/mac-rdp-server/`** jako druga
  lokalizacja — świadomie odrzucone przez właściciela 2026-10-16 na rzecz
  jednej ścieżki unixowej.
- **Zmienne środowiskowe dla każdej opcji.** `MAC_RDP_LOG` zostaje wyjątkiem,
  bo ratuje w sytuacji, w której to właśnie konfiguracja jest zepsuta.
- **Format inny niż TOML.**

## 6. Status

**Implemented (2026-10-21)**. Draft 2026-10-16, approve właściciela
2026-10-19, kod 2026-10-21. Kolejność: po naprawie zamarzania (F-007
iteracja 1d + ADR-0016), przed F-111.

Implementacja:
- `src/config.rs` — stała `DEFAULT_CONFIG_FILE = ".config/mac-rdp-server.toml"`
  (F-112.1, jedna ścieżka, bez listy przeszukiwania); `ConfigSource`
  (`Requested` / `Default` / `Builtin`) jako osobny typ obok `Config`, żeby
  „na czym serwer stoi" i „skąd to wziął" nie mieszały się w jednej
  strukturze; `ConfigSource::decide` rozstrzyga źródło przed czytaniem
  czegokolwiek (F-112.2, F-112.3, F-112.6); `--no-config` z `conflicts_with =
  "config"` (F-112.6 — dwie flagi odpowiadają na to samo pytanie, więc podanie
  obu jest błędem użycia, nie cichą regułą precedencji); `Config::from_cli`
  zwraca `(Config, ConfigSource)`, a `from_cli_in` przyjmuje katalog domowy
  argumentem, co czyni **całą** drogę — nie tylko wybór ścieżki — testowalną
  bez prawdziwego `$HOME` (F-112.9).
- `src/main.rs` — `config = %config_source` w linii startowej (F-112.7).

**Decyzja przy implementacji, nieprzewidziana w specu:** plik domyślny jest
czytany także wtedy, gdy `try_exists()` **nie umie odpowiedzieć** (np. brak
prawa wykonania na katalogu w ścieżce). `Path::exists()` zwróciłoby wtedy
`false`, czyli plik napisany przez użytkownika zostałby po cichu pominięty —
dokładnie to, czemu zapobiega F-112.5. Jedyny cichy przypadek to potwierdzone
„pliku nie ma".

Testy jednostkowe `config::tests::f112_*` — osiem, po jednym na każdą
kombinację z F-112.2 plus brak `HOME`, zepsuty plik domyślny, `--no-config`
na tym samym zepsutym pliku i odrzucenie `--no-config --config` razem.
Suite: 174 → 182.

### Weryfikacja (2026-10-21)

Kryteria sprawdzone **zbudowaną binarką**, nie lekturą kodu — pomiar
z 2026-10-15 wyszedł odwrotnie niż kod sugerował, więc ta kolejność jest tu
zasadą. `HOME` wskazany na katalog tymczasowy; `cert_path` w pliku ustawiony
na nieistniejącą ścieżkę, żeby przebieg kończył się zaraz po linii startowej
i żeby **żaden port się nie otworzył** (interesuje nas log, nie nasłuch).

| Kryterium | Uruchomienie | Wynik |
|---|---|---|
| AC-1 | bez żadnych flag, plik z `0.0.0.0` i `33891` | `config=<ścieżka> addr=0.0.0.0:33891` |
| AC-2 | `--bind-addr 127.0.0.1` | `addr=127.0.0.1:33891` — flaga bierze adres, port zostaje z pliku |
| AC-3 | `--config <inny plik>` (tylko `port = 4242`) | `addr=127.0.0.1:4242` — `bind_addr` domyślny, czyli plik domyślny nie został przeczytany |
| AC-4 | pusty katalog domowy | `config=built-in defaults addr=127.0.0.1:3389`, zero ostrzeżeń |
| AC-5 | plik domyślny z `bind_addr = "nie-adres"` | `Error: parsing config file '<ścieżka>'` + `invalid IP address syntax`, **kod wyjścia 1** |
| AC-6 | wszystkie powyższe | linia startowa zawiera albo ścieżkę, albo `built-in defaults` |
| F-112.6 | ten sam zepsuty plik + `--no-config` | serwer przechodzi dalej na wartościach domyślnych |

AC-7 (dokumentacja) — poprawione i przeczytane po implementacji, nie
z pamięci: README §Configuration (precedencja, `--config` **zamiast** pliku
domyślnego, cisza przy braku, błąd przy literówce, `--no-config`, dwie
przykładowe linie loga), `caveats` formuły (zniknęła linia
`--config ~/.config/...`, bo właśnie przestała być obowiązkowa) i nagłówek
`packaging/mac-rdp-server.toml`. `docs/` przegenerowane, `build-docs.py
--check` zielone.

**Zastrzeżenie:** README opisuje stan źródła, a tap serwuje 0.1.1, która tej
ścieżki nie czyta. Żeby README nie zastawiło na 0.1.1 tej samej pułapki, co
`caveats` do 2026-10-15, sekcja §Configuration ma `[!NOTE]` o tym, że samo
czytanie pliku domyślnego nie jest jeszcze w żadnym wydaniu.

Powstał z uwagi właściciela po pierwszej instalacji z Homebrew: „musiałem
dorzucić `--bind-addr 0.0.0.0`". Samo zachowanie było poprawne (domyślny
`127.0.0.1` jest celowy, F-110.10), ale okazało się, że tej opcji nie da się
zapamiętać.

**Decyzja właściciela 2026-10-16:** ścieżka to **`~/.config/mac-rdp-server.toml`**
— konwencja unixowa, zgodna z tym, co już piszą README, przykładowy plik
i `caveats` formuły.

**Dlaczego to blokuje F-111.** Ikonka w pasku menu pokazuje i uruchamia serwer
na jakimś adresie, porcie i ekranie. Bez tego specu byłyby to zawsze wartości
domyślne albo flagi podane w terminalu — czyli ikonka nie umiałaby tego, po co
powstaje: zastąpić terminal. Osobną sprawą, **nie** rozstrzyganą tutaj, jest
to, czy sam stan przełącznika („serwer ma wstawać włączony") ma przeżywać
restart; to ma sens dopiero razem z F-109 i wymaga zapisu do pliku, który jest
tu jawnie poza zakresem.
