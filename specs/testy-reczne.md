# Testy ręczne — co trzeba zrobić i gdzie wpisać wynik

**Status:** dokument żywy
**Utworzony:** 2026-10-13, na wniosek właściciela („zrób mi dokument gdzie
opisowo będzie napisane co trzeba zrobić. I tam dokładnie opiszę odpowiadając
w tym dokumencie")
**Dotyczy:** F-003, F-003a, F-004, F-007, F-008, F-110

## Po co ten dokument

Część tego projektu nie da się sprawdzić bez prawdziwego klienta RDP, bez
drugiej maszyny i bez Twoich rąk — macOS nie pozwala zasymulować nadawania
uprawnień, a zachowanie okna klienta zależy od klienta, nie od nas. Takich
kryteriów jest dziś **dwadzieścia jedno**, rozsypanych po pięciu
specyfikacjach, i to one blokują nadanie trzem falom statusu Verified.

Zamiast kazać Ci je zbierać, są tutaj wszystkie, w kolejności od
najważniejszego, z dokładnymi komendami do skopiowania i z miejscem na
odpowiedź. **Wpisujesz odpowiedź pod testem, w tym pliku.** Ja potem przenoszę
wynik do właściwej specyfikacji, w razie porażki szukam przyczyny, i tylko na
podstawie tego, co tu napiszesz, zmieniam statusy. Nic nie zostanie uznane za
zaliczone bez Twojego zdania.

**Nie musisz robić wszystkiego naraz ani po kolei.** Każdy blok jest
samodzielny. Jeśli czegoś nie zrobisz, zostaw `NIE ROBIŁEM` — to też jest
informacja, lepsza niż puste pole.

**Jeśli test padnie, nie naprawiaj go.** Zapisz, co widziałeś, i zostaw log.
Porażka z dobrym opisem jest dla mnie warta więcej niż zaliczenie.

## Jak odpowiadać

Pod każdym testem jest blok:

```
**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Log:
```

- **Wynik** — `PRZESZŁO`, `NIE PRZESZŁO`, `CZĘŚCIOWO` albo `NIE ROBIŁEM`.
- **Co widziałem** — własnymi słowami, tyle ile chcesz. Najcenniejsze są
  zdania w stylu „obraz wrócił, ale po trzech sekundach, nie od razu".
- **Log** — ścieżka do pliku, jeśli zapisywałeś. Jak zapisywać: niżej.

## Przygotowanie (raz, przed wszystkim)

### 1. Zbuduj aktualną wersję

```sh
cd ~/Projects/rdp-server-macos
cargo build --release
```

Binarka ląduje w `target/release/mac-rdp-server`. **Każda nowa binarka to dla
macOS-a nowy program**, więc po przebudowaniu uprawnienia mogą wymagać
ponownego nadania — patrz punkt 2.

### 2. Sprawdź uprawnienia

```sh
./target/release/mac-rdp-server --check-permissions
```

Ma wypisać `granted` dwa razy i zakończyć się kodem 0. Jeśli nie — wykonaj to,
co wypisze. Pamiętaj o tym, co już nas ugryzło dwa razy: **po aktualizacji
iTerma uprawnienie wygląda na nadane, a nie działa**; trzeba usunąć stary wpis
minusem, dodać nowy i zrestartować terminal do końca (Cmd+Q, nie zamknięcie
okna).

### 3. Jak uruchamiać serwer i zapisywać log

Do większości testów potrzebny jest log na poziomie `debug`:

```sh
MAC_RDP_LOG=info,mac_rdp_server=debug,ironrdp_server=debug \
  ./target/release/mac-rdp-server 2>&1 | tee ~/Desktop/test-A1.log
```

Nazwę pliku zmieniaj na numer testu (`test-A1.log`, `test-B2.log`, …) — wtedy
w odpowiedzi wystarczy, że podasz nazwę.

**Żeby połączyć się z Windowsa albo z drugiego Maca**, dodaj `--bind-addr
0.0.0.0`:

```sh
MAC_RDP_LOG=info,mac_rdp_server=debug,ironrdp_server=debug \
  ./target/release/mac-rdp-server --bind-addr 0.0.0.0 2>&1 | tee ~/Desktop/test-A1.log
```

> **Serwer nie ma żadnego uwierzytelniania.** Z `0.0.0.0` każdy w Twojej sieci
> ma Twój ekran i klawiaturę. Tylko w domowej sieci, tylko na czas testu,
> i wyłącz go po skończeniu (Ctrl+C).

### 4. Czego szukać w logu

Te komunikaty pojawiają się w testach niżej, więc warto wiedzieć, jak
wyglądają:

| Komunikat w logu | Znaczy |
|---|---|
| `client paused display updates (suppress output)` | klient powiedział „nie wysyłaj, jestem zminimalizowany" |
| `client resumed display updates` | klient wrócił |
| `baseline invalidated (client resumed)` | po powrocie wysyłamy pełny obraz, nie różnicę |
| `full refresh: the next frame is sent whole` | odmalowanie całego pulpitu |
| `damage stats` | ile bajtów i klatek poszło — tu widać, czy ruch spadł do zera |
| `expected client confirm active` | **błąd**, którego nie powinno już być (patrz A-3) |

Do przeszukania logu po fakcie:

```sh
grep -nE 'suppress|resumed|baseline|full refresh|damage stats|ERROR|WARN' ~/Desktop/test-A1.log
```

---

# Blok A — F-008, bramka C (najpilniejszy)

**Dlaczego to jest pierwsze.** 2026-09-24 kryterium AC-5 **padło**: klient
z rodziny FreeRDP rozłączał się przy ponownej aktywacji obrazu. Poprawka
powstała (`F-008.7v2`, a potem druga od strony akceptora w naszym forku
IronRDP) i **nigdy nie została sprawdzona prawdziwym klientem**. Dopóki nie
jest, mamy w repo poprawkę na zrywanie połączenia, o której nie wiemy, czy
działa — a to jest najgorszy rodzaj niewiedzy, bo wygląda jak gotowe.

Cały blok zajmuje około 25 minut. Potrzebujesz **dwóch klientów**: `mstsc`
z Windowsa i czegoś z rodziny FreeRDP (Windows App na macOS albo `xfreerdp`).

## A-1 — Minimalizacja przy ruchu na Macu (F-008 AC-3)

**Po co.** Kiedy zminimalizujesz okno klienta, serwer powinien przestać
cokolwiek wysyłać, a po przywróceniu pokazać aktualny obraz — nie ten sprzed
minimalizacji.

**Co zrobić.**

1. Uruchom serwer z logiem (sekcja Przygotowanie 3).
2. Połącz się klientem.
3. Na **Macu** włącz jakiekolwiek wideo na pełnym ekranie (cokolwiek, co się
   ciągle zmienia).
4. **Zminimalizuj okno klienta** i odczekaj 30 sekund.
5. Przywróć okno.

**Co ma się stać.** W logu po minimalizacji pojawia się `client paused display
updates (suppress output)`, a licznik w `damage stats` **przestaje rosnąć**.
Po przywróceniu w ciągu sekundy widzisz aktualny obraz wideo, bez kawałków
sprzed minimalizacji.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Log:

## A-2 — Statyczny pulpit (F-008 AC-4)

**Po co.** To odwrotny przypadek: gdy nic się nie rusza, capture nie dostarcza
nowych klatek. Trzeba sprawdzić, że przywrócenie okna pokazuje zmianę, która
zaszła w czasie minimalizacji, mimo że żadna nowa klatka nie przyszła.

**Co zrobić.**

1. Zminimalizuj okno klienta.
2. Na Macu, **lokalną klawiaturą**, zrób jedną widoczną zmianę — otwórz okno,
   przełącz aplikację — i zostaw pulpit w spokoju, nie ruszaj myszą.
3. Przywróć okno klienta.

**Co ma się stać.** Zmiana jest widoczna natychmiast po przywróceniu.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Log:

## A-3 — Ponowna aktywacja nie zrywa sesji (F-008 AC-5) ⚠️ to padło

**Po co.** To jest **to** kryterium. Poprzednio sesja się rozłączała.
Oczekiwanie po poprawce: nie rozłącza się, w logu **nie ma** `expected client
confirm active`.

**Co zrobić — na obu klientach osobno.**

1. Połącz się.
2. Zminimalizuj i przywróć okno — kilka razy.
3. **Zmień rozmiar okna klienta** — kilka razy, także przeciągając narożnik
   powoli.
4. Spróbuj trybu pełnoekranowego i wyjścia z niego.

**Co ma się stać.** Sesja żyje. Okno nie zmienia skalowania samo z siebie.
Mignięcie najwyżej jednokrotne. W logu żadnego rozłączenia.

**ODPOWIEDŹ — mstsc (Windows):**
- Wynik:
- Co widziałem:
- Log:

**ODPOWIEDŹ — Windows App / xfreerdp (to na tym padło):**
- Wynik:
- Co widziałem:
- Log:

## A-4 — Dziesięć cykli pod rząd (F-008 AC-6)

**Po co.** Sprawdzenie koalescencji: szybkie minimalizacje nie mogą wywołać
dziesięciu ponownych aktywacji ani błędu. Poprzednio zrobiłeś dwa cykle — spec
wymaga dziesięciu, bo problem ujawnia się przy nawale, nie przy jednym kliknięciu.

**Co zrobić.** Dziesięć razy zminimalizuj i przywróć okno, w tempie około
dwóch na sekundę (łącznie ~5 sekund).

**Co ma się stać.** Sesja działa dalej. W logu najwyżej około sześciu
ponownych aktywacji, nie dziesięć. Żadnego błędu.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem (ile aktywacji w logu):
- Log:

## A-5 — Cmd+Tab przy spowolnionym kliencie (F-008 AC-7, F-004 AC-5)

**Po co.** Robiłeś to już i przeszło, ale **bez** `--max-fps 5`. Niski limit
klatek jest tu istotny: to scenariusz, w którym koalescencja gubi najwięcej
klatek, czyli najłatwiej zostawić „ducha" starej treści. Bez tego przebiegu
kryterium zostaje otwarte.

**Co zrobić.**

```sh
MAC_RDP_LOG=info,mac_rdp_server=debug,ironrdp_server=debug \
  ./target/release/mac-rdp-server --max-fps 5 2>&1 | tee ~/Desktop/test-A5.log
```

Przez klienta: Cmd+Tab między aplikacjami, zamknij okno, przeciągnij okno
przez cały ekran.

**Co ma się stać.** Nigdzie nie zostaje fragment poprzedniej treści. Obraz
jest wolniejszy — to normalne przy 5 fps — ale spójny.

**Jeśli zostanie duch:** sprawdź w logu, czy było `client paused display
updates`. Brak tego wpisu znaczy, że przyczyna jest inna niż ta, którą
naprawialiśmy — i to jest ważna informacja, bo F-004 §8 ma to pytanie otwarte
od sierpnia.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Czy po ruszeniu myszą po zamrożonym obszarze obraz się naprawiał:
- Log:

## A-6 — To samo na enkoderze pełnoklatkowym (F-008 AC-8)

**Po co.** Oddziela „zepsuty damage tracking" od „zepsuta ścieżka do klienta".
Nie było jeszcze robione.

**Co zrobić.** Uruchom z `--encoder full` i powtórz **A-1 i A-2**.

```sh
MAC_RDP_LOG=info,mac_rdp_server=debug,ironrdp_server=debug \
  ./target/release/mac-rdp-server --encoder full 2>&1 | tee ~/Desktop/test-A6.log
```

**Co ma się stać.** Ten sam wynik co w A-1 i A-2.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Log:

## A-7 — Kilkanaście minut ciągłej sesji (F-008 AC-9)

**Po co.** Regresje stabilności nie pokazują się w pierwszej minucie.
Poprzednio sprawdzone na 5 minutach; spec mówi „kilkanaście".

**Co zrobić.** Zostaw sesję na **15 minut**, pracuj normalnie, w trakcie
zminimalizuj i przywróć okno kilka razy. Na koniec zostaw pulpit bez ruchu na
minutę i spójrz na `damage stats`.

**Co ma się stać.** Przy nieruchomym pulpicie ruch spada do bliskiego zera.
Żadnego rozłączenia, żadnego narastającego zużycia.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem (bajty/s przy nieruchomym pulpicie):
- Log:

---

# Blok B — środkowy przycisk myszy (F-003 AC-3, AC-16)

**Dlaczego to ważne.** To była Twoja pierwsza prośba z listy braków.
Działający kod jest od 2026-10-12, w naszym forku IronRDP, i jest obudowany
testami — w tym takim, który **pada** na zależnościach z crates.io, czyli
dowodzi, że łatka robi dokładnie to, co miała. Ale żaden test nie klika
rolką w prawdziwym programie. To jest cała treść tego bloku: dwa kliknięcia.

Zajmuje 3 minuty. **Wymaga obu klientów**, bo flaga RDP dla środkowego
przycisku jest ta sama co dla rolki i chcę mieć pewność, że żaden klient nie
zamienia jednego w drugie.

## B-1 — Klik rolką otwiera link w nowej karcie

**Co zrobić.**

1. Połącz się klientem.
2. Otwórz przeglądarkę na Macu i kliknij **rolką** w jakikolwiek link.
3. Potem kliknij rolką w **kartę** przeglądarki (to ją zamyka).
4. Zakręć rolką w górę i w dół — osobno, żeby sprawdzić, że klik nie zepsuł
   przewijania.

**Co ma się stać.** Link otwiera się w nowej karcie. Klik w kartę zamyka ją.
Przewijanie działa jak dotąd, w obie strony.

**ODPOWIEDŹ — mstsc (Windows):**
- Wynik:
- Co widziałem:

**ODPOWIEDŹ — Windows App / xfreerdp:**
- Wynik:
- Co widziałem:

## B-2 — Lewy i prawy przycisk nadal działają (F-003 AC-3)

**Po co.** Łatka dotyka funkcji, która tłumaczy wszystkie przyciski naraz.
Sprawdzenie, że nic się nie przesunęło.

**Co zrobić.** Klik, podwójny klik, menu kontekstowe prawym, przeciągnięcie
ikony na pulpicie.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:

---

# Blok C — stabilność (F-007 AC-3, AC-6)

## C-1 — Uśpienie i wybudzenie ekranu (F-007 AC-3) — **zrobione 2026-10-19, NIEZALICZONE**

**Nie trać na to czasu — to już jest zmierzone i zdiagnozowane.** Wyszło
dokładnie to, co zgłosiłeś w trakcie testów: „po dłuższym czasie nie
korzystania rdp freezuje. Ponowne podłączenie się nie pomaga."

Co ustalił pomiar (szczegóły: F-007 §8c, sonda
`specs/probes/f007-ac3-displaysleep/`):

- Przyczyna: `SCStream::start_capture` i `stop_capture` w
  `screencapturekit 8.0.1` czekają na `Condvar::wait_while` **bez timeoutu**.
  Po uśpieniu ekranu handler zakończenia już nie odpala, więc odzyskiwanie
  strumienia wisi na zawsze.
- Reconnect nie pomaga, bo strumień jest **jeden na proces** i startuje przed
  serwowaniem; nowa sesja dostaje ostatnią wartość martwego kanału.
- Poprawka czeka na approve: wymagania F-007.19 … F-007.25 i **ADR-0016**
  (bump `screencapturekit` do 11.0.0, gdzie oczekiwanie jest ograniczone).

**Co z tym robić w praktyce, dopóki nie jest naprawione:**

```sh
caffeinate -d mac-rdp-server …   # nie pozwala ekranowi zasnąć
pkill -9 mac-rdp-server          # jeśli już zamarzł — PRZED ponownym startem
```

Drugie polecenie nie jest kosmetyką: dopóki zawieszony proces żyje, **każdy**
następny start serwera zawiśnie, bo blokuje przechwytywanie dla całej maszyny.

Do ponownego wykonania **po** poprawce — wtedy skryptem, nie ręcznie:

```sh
zsh specs/probes/f007-ac3-displaysleep/run.sh
```

Skrypt gasi ekran, budzi go i sam mówi PASS albo FAIL (F-007 AC-8).

## C-2 — Pięć cykli połącz–rozłącz (F-007 AC-6)

**Co zrobić.** Pięć razy połącz się i rozłącz, przy czym **co najmniej raz
zabij klienta na twardo** (zamknięcie okna bez wylogowania, albo Force Quit).

**Co ma się stać.** Serwer żyje dalej po każdym razie, a każde nowe połączenie
pokazuje obraz od razu.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Log:

## C-3 — Obraz wraca u podłączonego klienta (F-007 AC-10, F-007.25, F-007.27)

**To jest teraz najtańszy test ręczny w całym dokumencie i domyka trzy rzeczy
naraz.** Awarii nie trzeba wyczekiwać — sonda wywołuje ją na żądanie, w ciągu
kilku sekund, bez gaszenia ekranu.

**Co zrobić.**

1. Uruchom serwer tak jak zwykle (patrz „Przygotowanie" §3).
2. Połącz się z `mstsc` i **zostaw okno widoczne** — nie minimalizuj go.
   Minimalizacja włącza suppress output i wtedy nic nie zobaczysz; tak
   przepadły dwa poprzednie podejścia do F-007.25.
3. Patrząc na okno klienta, uruchom w drugim terminalu:

   ```sh
   ./target/release/mac-rdp-server --port 33900
   ```

   Poczekaj, aż napisze `first frame received`, i zatrzymaj je przez Ctrl-C.

**Co ma się stać.** Po kilku sekundach obraz zamiera, potem — **bez
rozłączania i bez restartu** — wraca sam. W logu serwera:

```
WARN  capture stream stopped calling us entirely (no frames, no idle, no rejects); recovering
INFO  capture recovered display=5
```

Zmierzone automatycznie: cisza wykryta po 5 s, strumień przebudowany w 0,6 s,
czyli przerwa rzędu sześciu sekund (F-007 §8i).

**Na co patrzeć oprócz tego, czy wróciło.** Jeśli przerwa przeciągnie się
powyżej trzech sekund, klient MA pokazać **pomarańczowy ekran z dwiema
pauzami** zamiast zamrożonej klatki (F-007.27). Przy sześciosekundowej
przerwie może mignąć bardzo krótko albo nie zdążyć się pojawić — i to jest
poprawne. Napisz, co widziałeś: zamrożoną klatkę, pomarańczowy ekran, czy nic
nie zauważyłeś.

**ODPOWIEDŹ:**
- Wynik:
- Czy obraz wrócił bez rozłączania:
- Czy mignął pomarańczowy ekran:
- Ile trwała przerwa (z zegarka albo z logu):
- Log:

---

# Blok D — emoji (F-003a AC-10)

**Stan sprawy, żeby nie szukać od nowa.** To padło 2026-09-18 i **wiemy
dlaczego**: `mstsc` w ogóle nie wysyła zdarzeń Unicode. Zmierzone — w logu
z 190 zdarzeń klawiatury było **zero** Unicode. Strona macOS jest dowiedziona
osobnym programem, który wstrzyknął 😀 do TextEdit dokładnie tą samą drogą,
której używa serwer. Czyli nasz kod jest w porządku, a `mstsc` nie ma czym go
wywołać.

Kryterium zostało przez Ciebie przeformułowane i zatwierdzone: zalicza się na
**kliencie, który wysyła Unicode**.

## D-1 — Emoji z klienta, który wysyła Unicode

**Co zrobić.** Z Windows App na macOS albo z `xfreerdp` (nie z `mstsc`)
wklej albo wpisz emoji i polski znak diakrytyczny (`ą`, `ż`) do TextEdit na
Macu.

**Co ma się stać.** Znaki docierają. Jeśli nie — potrzebny będzie log na
poziomie `trace`, ale **uwaga: `trace` zapisuje wszystko, co piszesz, łącznie
z hasłami.** Nie włączaj go przy okazji, tylko do tego testu i na krótko.

**ODPOWIEDŹ:**
- Wynik:
- Którym klientem:
- Co widziałem:

---

# Blok E — instalacja z Homebrew (F-110)

**Stan na 2026-10-13: wydanie `v0.1.0` jest opublikowane, a większość tego
bloku już przeszedła** — formuła z wydania została wstawiona do lokalnego tapa
i przeszła na niej pełny cykl: `brew audit --strict --online` → 0,
`brew style` → 0, `brew test` → 0, instalacja w 1 sekundę, wersja zgodna
z tagiem, `brew uninstall` bez śmieci. Szczegóły w F-110 §Status.

**Aktualizacja 2026-10-14: wyszło `v0.1.1`** (poprawka `rustls`, patrz
`CHANGELOG.md`), więc E-6 przestało wymagać „następnego wydania" i jego
mierzalna część została zrobiona: `brew upgrade` z `0.1.0` na `0.1.1` kończy
się kodem 0, a ścieżka i cdhash binarki się zmieniają. Do zobaczenia zostaje
sama lista w Ustawieniach.

**Aktualizacja 2026-10-15: E-1 zrobione.** Zainstalowałeś `0.1.1` z tapa
i powiedziałeś, że działa; wynik zmierzony i wpisany niżej. Przy okazji
wyszedł brak, który nie jest błędem instalacji: ustawień nie da się
zapamiętać, bo plik konfiguracyjny jest czytany tylko przez `--config`.
Propozycja wymagania czeka na Twoją decyzję — `waves.md` §Pomiar 2026-10-15.

**Zostały trzy punkty, których nie da się zrobić za Ciebie:** E-2 (wymaga
drugiego Maca), E-3 (wymaga Twojej zgody na aktualizację trzech pakietów),
E-5 (wymaga obcej osoby) — plus połowa E-6, niżej. E-4 zostaje poniżej jako
zapis tego, co już przeszło.

## E-1 — Instalacja na tej maszynie (F-110 AC-1) — **zrobione 2026-10-15**

Zrobiłeś to sam („odpaliłem z command line, zainstalowana działała").
Zmierzone po fakcie na tym, co zostało na dysku: `brew list --versions` →
`mac-rdp-server 0.1.1`; źródło `dawidadamski/mac-rdp-server`, czyli prawdziwy
tap, nie lokalna kopia; dowiązanie na
`Cellar/mac-rdp-server/0.1.1/bin/mac-rdp-server`; `--version` → `0.1.1`;
`codesign` → `adhoc`, bez `TeamIdentifier`; `xattr` bez kwarantanny;
`--check-permissions` → oba `granted` i kod 0; serwer startuje i wczytuje
istniejący certyfikat.

**Przewidywanie, które stało w tym punkcie, było błędne — i to pouczająco.**
Było napisane, że `--check-permissions` powie, że uprawnień brakuje, bo
zgoda była nadana binarce z `target/release`, a to „inny program" dla
macOS-a. Tak się nie stało i stać nie mogło: uprawnienie jest nadane
**aplikacji terminalowej**, z której startujesz serwer, a nie żadnej binarce
serwera — więc binarka z Homebrew pracuje na tej samej zgodzie co binarka
z `target/release`. To ten sam mechanizm, który pomiar z 2026-10-14 pokazał
na `NSStatusItem`, i to on przenosi połowę E-6 za F-109.

**Dwie rzeczy, które przy okazji zgłosiłeś, i co z nimi:**

- **`--bind-addr 0.0.0.0` trzeba podawać ręcznie przy każdym uruchomieniu.**
  Domyślny `127.0.0.1` jest celowy, ale okazało się, że **nie da się tego
  zapamiętać**: plik konfiguracyjny jest czytany tylko wtedy, gdy podasz
  `--config`, domyślnej ścieżki nie ma. Sprawdzone pomiarem (F-110 §Status).
  `caveats` formuły poprawione tego dnia, żeby nie sugerowało inaczej.
  Propozycja wymagania o domyślnej ścieżce — `waves.md` §Pomiar 2026-10-15,
  czeka na Twój approve.
- **Brak ikonki w pasku menu.** Oczekiwane: to F-111, spec jeszcze nie
  istnieje, więc w binarce nie ma po niej ani linijki. Następne w kolejce.

**ODPOWIEDŹ:** zrobione, wynik wyżej — nic do uzupełnienia.

## E-2 — Mac bez Rusta (F-110 AC-4)

**Po co.** Główna ścieżka instalacji to gotowa binarka. Na maszynie z Rustem
nie da się sprawdzić, czy naprawdę nie buduje ze źródeł.

**Co zrobić.** Na drugim Macu (bez Rusta) ta sama komenda co w E-1, a potem:

```sh
xattr -l "$(brew --prefix)/bin/mac-rdp-server"
```

**Co ma się stać.** Instalacja trwa sekundy, nie minuty. `xattr` **nie**
pokazuje `com.apple.quarantine` — to zmierzyliśmy na Twojej formule `bsi`
i dlatego nie potrzebujemy notaryzacji. Jeśli kwarantanna jednak będzie,
powiedz od razu: to wywraca założenie F-110.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:

## E-3 — Instalacja ze źródeł (F-110 AC-2)

**Uwaga, dotyka Twojej maszyny.** `brew install --HEAD` chciał zaktualizować
`llvm`, `pkgconf` i `sqlite`. Przerwałem to wtedy celowo. Ten test wymaga
Twojej zgody na te aktualizacje — albo pominięcia go.

```sh
brew install --HEAD DawidAdamski/mac-rdp-server/mac-rdp-server
```

**ODPOWIEDŹ:**
- Wynik:
- Czy zgadzasz się na aktualizację llvm/pkgconf/sqlite:
- Co widziałem:

## E-4 — Odinstalowanie (F-110 AC-5) — **już przeszło**

Zaliczone 2026-10-13: binarka i dowiązanie znikają, katalog z certyfikatem
zostaje nietknięty. **Uwaga przy powtarzaniu:** `brew uninstall` usunął wtedy
automatycznie cztery osierocone zależności (`llvm@22`, `python@3.13`,
`libnghttp3`, `libngtcp2`). Nic się przez to nie zepsuło, ale jeśli nie
chcesz tego efektu, użyj `HOMEBREW_NO_AUTOREMOVE=1`.

```sh
brew uninstall mac-rdp-server
ls ~/Library/Application\ Support/mac-rdp-server
```

**Co ma się stać.** Binarka znika. Katalog z certyfikatem i konfiguracją
**zostaje** — to świadome, odinstalowanie nie kasuje Twoich danych.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:

## E-5 — Czy README wystarcza obcej osobie (F-110 AC-6)

**Po co.** Jedyne kryterium, którego nie da się sprawdzić samemu, bo znasz
projekt od środka.

**Co zrobić.** Daj komuś link <https://dawidadamski.github.io/mac-rdp-server/>
i poproś, żeby nadał oba uprawnienia **bez pytania Cię o nic**.

**ODPOWIEDŹ:**
- Wynik:
- Gdzie się zaciął:

## E-6 — Aktualizacja i stary wpis w Ustawieniach (F-110 AC-7) — **połowa już przeszła**

**Co już zmierzone (2026-10-14, na lokalnym tapie).** `brew upgrade` z `0.1.0`
na `0.1.1` kończy się kodem 0. Dowiązanie przeskakuje z
`Cellar/mac-rdp-server/0.1.0/bin` na `.../0.1.1/bin`, a cdhash binarki zmienia
się z `a847844d…` na `78f78c9f…` — czyli tożsamość, do której macOS przypina
zgodę, przestaje istnieć. Homebrew wypisał przy tym `caveats` z instrukcją
o usunięciu starego wpisu, więc trafia ona do użytkownika dokładnie wtedy,
kiedy jest potrzebna.

**Co zostaje — i dlaczego jeszcze nie teraz.** Pomiar z 2026-10-14
(`waves.md` §Pomiar) pokazał, że **binarka uruchomiona z terminala korzysta
z uprawnień terminala**, a własnym podmiotem TCC staje się dopiero wtedy, gdy
startuje spod `launchd` (albo z bundla `.app`). To znaczy, że dopóki
uruchamiasz serwer z iTerma, `brew upgrade` **niczego Ci nie zepsuje** —
zgoda należy do iTerma, nie do binarki, więc stary wpis nie ma czego
unieważnić.

Czyli: połowa AC-7 dotycząca listy w Ustawieniach jest sprawdzalna dopiero
**po F-109** (uruchamianie przez `launchd`). Wtedy przebieg jest taki:

1. serwer wystartowany przez `launchd`, oba uprawnienia nadane **tej binarce**
   (pojawi się w listach pod ścieżką z `Cellar/…/0.1.1/bin`);
2. następne wydanie i `brew upgrade mac-rdp-server`;
3. Ustawienia → Prywatność i bezpieczeństwo → Nagrywanie ekranu i Dostępność
   — tam ma być stary wpis obok nowego.

Do tego czasu instrukcja z F-110.13 i tak jest potrzebna, bo **ten sam problem
wywołuje aktualizacja samego iTerma** — to już nas trafiło dwa razy.

**ODPOWIEDŹ:**
- Wynik:
- Czy stary wpis został na liście:
- Czy wyglądał na włączony:
- Czy serwer ruszył po usunięciu starego i dodaniu nowego:

---

# Blok F — reszta Fali 3 (F-004 AC-5)

## F-1 — Duchy po Cmd+Tab bez spowolnienia

**Stan sprawy.** To padło 2026-08-25: po Cmd+Tab obraz czasem zostawał
nieodświeżony, objaw niewyjaśniony i świadomie przez Ciebie odłożony.
Od tego czasu doszedł F-008 (Suppress Output), który mógł to naprawić jako
efekt uboczny — albo nie, bo przyczyna może być inna.

Test A-5 pokrywa ten scenariusz przy `--max-fps 5`. Ten punkt to to samo
**bez** ograniczenia klatek, czyli w warunkach, w których objaw wystąpił
pierwotnie.

**Co zrobić.** Normalny serwer (bez `--max-fps`), przez klienta: Cmd+Tab
w tę i z powrotem, 20 razy, szybko. Zamknij i otwórz okna.

**Co ma się stać.** Nigdzie nie zostaje kawałek starej treści.

**ODPOWIEDŹ:**
- Wynik:
- Co widziałem:
- Jeśli został duch — czy w logu było `client paused display updates`:
- Log:

---

# Blok G — ikonka w pasku menu (F-111)

**Czego ten blok dotyczy.** Nowy tryb: `--menu-bar`. Serwer startuje
**wyłączony** — uruchomienie programu nie jest uruchomieniem usługi (F-111.18,
Twoje wymaganie z 2026-10-21) — a włącza się i wyłącza z menu, bez restartu
procesu.

**Jak uruchomić.** Z terminala, żeby widzieć log:

```sh
./target/release/mac-rdp-server --menu-bar
```

Zostaw to okno terminala otwarte. Ikonka pojawia się po prawej stronie górnego
paska. **Uwaga:** uprawnienia TCC należą do programu, który uruchamia serwer,
czyli do Twojego terminala — tak samo jak dotąd, nic się tu nie zmienia.

---

## G-1 — Ikonka jest, Docka nie ma, portu nie ma (F-111 AC-4, AC-12)

**Co zrobić.** Uruchom jak wyżej. **Nie klikaj jeszcze nic.** Potem
w drugim terminalu:

```sh
lsof -nP -iTCP:3389
```

**Co ma się stać.**
- ikonka widać w pasku (kółko z poziomą strzałką, niewypełnione);
- w Docku **nie ma nic** — żadnej nowej ikony programu;
- `lsof` **nie pokazuje nic** — nikt nie nasłuchuje;
- w pasku menu **nie ma** systemowego wskaźnika nagrywania ekranu
  (F-111.19 — usługa wyłączona znaczy też „ekran nie jest przechwytywany");
- w logu: `menu bar ready; the server is NOT listening until you start it from
  the menu`.

**ODPOWIEDŹ:**
- Wynik:
- Czy ikonka była widoczna i jak wyglądała:
- Czy coś pojawiło się w Docku:
- Wynik `lsof`:
- Czy był wskaźnik nagrywania ekranu:

---

## G-2 — Menu pokazuje adres i uprawnienia (F-111 AC-5)

**Co zrobić.** Kliknij ikonkę.

**Co ma się stać.** Menu z pięcioma rzeczami: `Not listening`,
`Would listen on 127.0.0.1:3389`, `Start the server`, dwa wiersze uprawnień
(`Screen Recording: granted`, `Accessibility: granted` — albo `NOT granted —
click to open Settings`), `Quit mac-rdp-server`.

**ODPOWIEDŹ:**
- Wynik:
- Co dokładnie było w menu (przepisz wiersze):

---

## G-3 — Włączenie i wyłączenie dostępu z menu (F-111 AC-6, AC-7)

**To jest sedno tego bloku.**

**Co zrobić.**
1. Przy **wyłączonej** usłudze spróbuj połączyć się z `mstsc`. Ma **nie**
   połączyć się.
2. Kliknij `Start the server`. Poczekaj chwilę (startuje capture, więc to nie
   jest natychmiastowe) i połącz się tym samym klientem.
3. Przy podłączonym kliencie kliknij ikonkę → `Stop the server`.

**Co ma się stać.** Po punkcie 2 obraz jest, bez restartu programu; ikonka
zmienia się na wypełnioną, a menu na `Listening on 127.0.0.1:3389` i
`Stop the server`. Po punkcie 3 klient zostaje **rozłączony**, ikonka wraca do
niewypełnionej. W logu `server started from the menu` i `server stopped`.

**ODPOWIEDŹ:**
- Wynik:
- Czy przy wyłączonej usłudze klient się nie połączył:
- Czy po włączeniu był obraz:
- Czy wyłączenie rozłączyło klienta:
- Czy ikonka zmieniała wygląd:
- Log:

---

## G-4 — Dziesięć cykli i czysty port (F-111 AC-8)

**Co zrobić.** Dziesięć razy pod rząd: `Start the server`, połącz klientem,
`Stop the server`. Po ostatnim wyłączeniu:

```sh
lsof -nP -iTCP:3389
ps -o rss= -p $(pgrep -f 'mac-rdp-server --menu-bar')
```

**Co ma się stać.** Za każdym razem da się połączyć, `lsof` po wyłączeniu nie
pokazuje nic, a zużycie pamięci po dziesiątym cyklu jest tego samego rzędu co
po pierwszym (zanotuj obie liczby — dokładna wartość nie ma znaczenia,
dziesięciokrotny wzrost ma). W logu **nie powinno** być linii
`the address is still taken after stopping the server`.

**ODPOWIEDŹ:**
- Wynik:
- Czy każdy z dziesięciu startów dał połączenie:
- `lsof` po ostatnim wyłączeniu:
- RSS po pierwszym i po dziesiątym cyklu:
- Czy w logu była linia o zajętym adresie:

---

## G-5 — Zajęty port i Quit (F-111 AC-9 … AC-11)

**Co zrobić.**
1. **Zajęty port.** Uruchom zwykły serwer z terminala (`./target/release/mac-rdp-server`),
   a potem drugi raz ten sam program z `--menu-bar` i kliknij `Start the server`.
2. **Brak uprawnienia** (tylko jeśli chcesz — to jest odbieranie i oddawanie
   grantu TCC): odbierz Screen Recording terminalowi i kliknij wiersz
   uprawnienia w menu.
3. **Konfiguracja.** Zatrzymaj wszystko, wpisz do `~/.config/mac-rdp-server.toml`
   `port = 33891`, uruchom `--menu-bar` jeszcze raz i otwórz menu.
4. **Quit.** Przy **włączonej** usłudze i podłączonym kliencie wybierz
   `Quit mac-rdp-server`.

**Co ma się stać.**
1. Usługa się **nie** włącza, ikonka zmienia się w trójkąt ostrzegawczy, a
   pierwszy wiersz menu mówi `Could not start: cannot listen on …` z powodem.
   Kolejne kliknięcie `Start the server` próbuje jeszcze raz.
2. Wiersz mówi, którego uprawnienia brakuje, a kliknięcie otwiera właściwy
   panel Ustawień.
3. Menu pokazuje `Would listen on 127.0.0.1:33891` — port z pliku, nie
   domyślny (spięcie z F-112).
4. Proces się kończy, ikonka znika z paska, klient zostaje rozłączony,
   a `lsof -nP -iTCP:<port>` nic nie pokazuje.

**ODPOWIEDŹ:**
- Wynik (1. zajęty port):
- Wynik (2. uprawnienie — albo „pominięte"):
- Wynik (3. port z pliku):
- Wynik (4. Quit):
- Log:

---

# Blok H — dwuklik (F-003 AC-17 … AC-20)

Nowe, po naprawie z 2026-10-21. Przyczyna jest zmierzona i poprawka
przetestowana jednostkowo, ale czy dwuklik **wchodzi do folderu**, może
powiedzieć tylko ręka na myszy. Cały blok to dwie, trzy minuty.

## H-1 — Dwuklik otwiera folder (F-003 AC-17)

**Po co.** To jest objaw, który zgłosiłeś: „szybki 2 klik nie działa".

**Co zrobić.** W oknie klienta: dwuklik na folderze na pulpicie albo
w Finderze — ma wejść do środka. Potem to samo na stronie Dysku Google,
na folderze. **Trzy razy pod rząd**, bo chodzi o powtarzalność, a nie
o jedno trafienie.

**ODPOWIEDŹ:**
- Wynik (Finder):
- Wynik (Dysk Google):
- Czy któryś z trzech razy nie zadziałał:

---

## H-2 — Dwa kliknięcia w różne rzeczy nie otwierają niczego (F-003 AC-18)

**Po co.** Licznik kliknięć wymaga, by drugi klik padł blisko pierwszego.
Bez tego warunku dwa szybkie kliknięcia w **różne** foldery otwierałyby ten
drugi — czyli naprawa jednego błędu zrobiłaby gorszy.

**Co zrobić.** Kliknij raz na jednym folderze, potem **szybko** (szybciej niż
pół sekundy) raz na innym, wyraźnie dalej. Oba mają się tylko zaznaczyć.
Nic nie ma się otworzyć.

**ODPOWIEDŹ:**
- Wynik:
- Czy coś się otworzyło:

---

## H-3 — Potrójny klik zaznacza akapit (F-003 AC-19)

**Po co.** Licznik nie jest obcięty do dwóch (F-003.30). Jeśli potrójny klik
działa, to znaczy, że liczy dalej, a nie tylko „zna" dwuklik.

**Co zrobić.** W TextEdit albo w polu tekstowym w przeglądarce kliknij trzy
razy szybko w akapit. Ma się zaznaczyć cały akapit (albo linia — zależy od
aplikacji), a nie jedno słowo.

**ODPOWIEDŹ:**
- Wynik:
- Co się zaznaczyło:

---

## H-4 — Pojedynczy klik, prawy i przeciąganie bez zmian (F-003 AC-20)

**Po co.** Licznik dotyka **każdego** kliknięcia, więc najgorszy możliwy
skutek tej zmiany to zepsuty zwykły klik. To jest test na to, że nie zepsuł.

**Co zrobić.** Pojedynczy klik zaznacza ikonę (nie otwiera!). Prawy przycisk
daje menu kontekstowe. Przeciągnij ikonę po pulpicie i okno za pasek tytułu.
Jeśli masz pod ręką mysz z rolką — klik rolką w link otwiera kartę (B-1).

**ODPOWIEDŹ:**
- Wynik (pojedynczy klik):
- Wynik (prawy):
- Wynik (przeciąganie):

---

## Blok I — Uwierzytelnianie (F-101)

**Przed blokiem.** Ustaw hasło raz:

```
mac-rdp-server --auth-user rdp --set-password
```

Przy pierwszym starcie z hasłem **wyskoczy okienko Keychaina**. Kliknij
**„Zawsze zezwalaj"** — to zmierzone i konieczne (ADR-0012 §Pomiar
2026-10-21c); po tym kolejne starty tej samej binarki są ciche. Okienko wróci
po `brew upgrade` albo po przebudowaniu ze źródeł, i to jest znane, nie awaria.

---

## I-1 — `mstsc` pyta o hasło **przed** pulpitem (F-101 AC-2)

**Po co.** To jest całe F-101 w jednym zdaniu. Nie „pyta po pokazaniu pulpitu",
nie „pyta zamiast" — pyta przed, i dopóki nie dostanie hasła, nie ma obrazu.

**Co zrobić.** Uruchom serwer z `--auth-user rdp` i `--bind-addr 0.0.0.0`.
Połącz się `mstsc` z innej maszyny albo z tej samej. Zanotuj, w którym
momencie pojawia się pytanie o nazwę użytkownika i hasło — przed czy po
pierwszym obrazie pulpitu.

**ODPOWIEDŹ:**
- Wynik:
- Czy przed obrazem:

---

## I-2 — Złe hasło nie wpuszcza (F-101 AC-3)

**Po co.** Odmowa musi być odmową, a nie wpuszczeniem z opóźnieniem. Przy
okazji sprawdza log audytowy (F-101.14), który jest jedynym śladem próby.

**Co zrobić.** Podaj `mstsc` **złe** hasło przy poprawnej nazwie użytkownika.
Pulpit nie może się pokazać. Potem zajrzyj w log serwera — powinna być jedna
linia `WARN` z `authentication rejected`, adresem klienta i nazwą użytkownika,
**bez hasła**.

**ODPOWIEDŹ:**
- Czy wpuściło:
- Czy w logu jest linia WARN (wklej ją):

---

## I-3 — Dobre hasło wpuszcza i nic nie psuje (F-101 AC-4)

**Po co.** NLA zmienia sekwencję połączenia na poziomie protokołu
(`with_hybrid` zamiast `with_tls`), więc może zepsuć wszystko, co było
sprawdzone wcześniej. To test, że nie zepsuło.

**Co zrobić.** Połącz się poprawnym hasłem. Sprawdź: obraz się rusza, mysz
klika, klawiatura pisze, **dwuklik wchodzi do folderu** (to blok H, tutaj
tylko czy przeżył NLA). W logu powinna być jedna linia `INFO`
`authentication accepted` z adresem i nazwą użytkownika (F-101.13).

**ODPOWIEDŹ:**
- Obraz / mysz / klawiatura:
- Dwuklik:
- Czy w logu jest linia INFO (wklej ją):

---

## I-4 — Poprawne hasło z inną nazwą użytkownika jest odrzucane (F-101 AC-10)

**Po co.** Dowód, że sprawdzana jest **para**, a nie samo hasło. Bez tego testu
serwer mógłby wpuszczać każdego, kto zna hasło, pod dowolną nazwą.

**Co zrobić.** Podaj `mstsc` poprawne hasło, ale nazwę użytkownika inną niż
skonfigurowana w `--auth-user`. Musi odrzucić.

**ODPOWIEDŹ:**
- Wynik:

---

## I-5 — Hasło po `brew upgrade` (F-101 AC-8a)

**Po co.** Kopia z brew ma inną tożsamość podpisu niż binarka lokalna, a
wydaniowa niesie identyfikator wyprowadzony z LC_UUID, zmienny przy każdym
linkowaniu. Zmierzone jest, że zgoda ginie z cdhashem — ten test mówi, ile to
kosztuje przy prawdziwej aktualizacji.

**Co zrobić.** Po najbliższym `brew upgrade` połącz się **bez ponownego
ustawiania hasła**. Zanotuj, czy wyskoczyło okienko i czy jedno kliknięcie
„Zawsze zezwalaj" wystarczyło na stałe dla tej wersji.

**ODPOWIEDŹ:**
- Czy okienko wyskoczyło:
- Czy hasło trzeba było ustawiać ponownie:
- Czy po kliknięciu kolejne starty są ciche:

---

## Co się stanie z Twoimi odpowiedziami

Dla każdego `PRZESZŁO` przenoszę wynik do sekcji Status właściwej
specyfikacji, z datą i Twoim opisem. Kiedy zbiorą się wszystkie kryteria
jednej fali, fala dostaje **Verified** — tego statusu nie nadaje się
pojedynczemu specowi (F-007 §8).

Dla każdego `NIE PRZESZŁO` szukam przyczyny i wracam z propozycją: albo
poprawka, albo zmiana wymagania z nowym ADR-em, jeśli okaże się, że wymaganie
było złe. Nie naprawiam po cichu.

Blok A zamyka F-008 i część F-004. Blok B zamyka F-003. Blok C zamyka F-007.
Bloki A+B+C+F razem zamykają **Falę 3**. Blok E zamyka F-110 i może poczekać
na wydanie. Blok G zamyka F-111 i należy już do Fali 5 — nie wstrzymuje
Fali 3. Blok H zamyka dwuklik (F-003 AC-17 … AC-20). Blok I zamyka **F-101**
i jest tym, czego brakuje do **Fali 4**; I-5 może poczekać na najbliższe
wydanie, bo potrzebuje prawdziwego `brew upgrade`.
