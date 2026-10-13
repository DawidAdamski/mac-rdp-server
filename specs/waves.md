# Roadmapa fal

**Status:** Approved (2026-07-22) — dokument żywy, aktualizowany po każdej fali
**Data ostatniej aktualizacji:** 2026-09-17

## Reguła

Fala jest **Verified** dopiero, gdy: (a) wszystkie kryteria akceptacji jej speców
są spełnione, (b) właściciel projektu ręcznie przetestował prawdziwym klientem RDP.
Dopiero wtedy zaczyna się następna fala.

| Fala | Nazwa | Specy | Status |
|---|---|---|---|
| 0 | Szkielet | F-001 | **Verified** (2026-07-22) |
| 1 | Żywy ekran | F-002 | **Verified** (2026-07-24) |
| 2 | Input | F-003, F-003a | **F-003 Verified** (2026-07-27); F-003a — do decyzji |
| 3 | Jakość MVP | F-004…F-007 + poprawki zgodności z MS-RDPBCGR | In progress (F-007 i F-004 Implemented; poprawki z `02-ms-rdpbcgr.md` §6 przed F-005; F-005 Draft; F-006 do napisania) |
| 4 | Bezpieczeństwo | F-101, F-108 | Planned |
| 5 | Produkt | F-106, F-109, F-110 | Planned |
| 6 | Komfort | F-102, F-003a, F-105 | Planned |
| 7 | Dźwięk | F-103 | Planned (2026-08-25) |
| — | Backlog | F-1xx | Planned |

## Cel Fal 4–6: z „działa u autora" na „da się zainstalować i używać"

Fale 0–3 realizują MVP z `00-vision.md` §4: **coś, co działa**. Fale 4–6
odpowiadają na osobne pytanie: **co musi być prawdą, żeby ktoś inny mógł tego
używać na swoim Macu, w swojej sieci, bez czytania kodu.**

Kierunek został ustalony przez właściciela **2026-08-07**:

| Decyzja | Wybór | Konsekwencja dla fal |
|---|---|---|
| Kolejność | Fala 3 → bezpieczeństwo → pakowanie | Damage/kursor/resize przed auth; auth przed dystrybucją |
| Uwierzytelnianie | **NLA/CredSSP**, hasło w Keychainie | Fala 4, spec F-101 + ADR-0012 |
| Ekspozycja sieciowa | **Pełny LAN jako scenariusz pierwszoklasowy** | Model zagrożeń musi zakładać napastnika w sieci — F-108 |
| Podpisywanie | **Bez Developer ID** — decyzja właściciela 2026-10-12 | Podpis ad-hoc, dystrybucja formułą w tapie Homebrew; cena (ponowne nadanie uprawnień po aktualizacji) udokumentowana w F-110.13, tak jak u `yabai`/`skhd` |
| Model wydania | Darmowy OSS + opcjonalne donacje | F-110: README, licencje, `SECURITY.md`, `FUNDING.yml` |

**Zasada porządkująca te trzy fale:** *najpierw serwer wolno wystawić, potem
łatwo zainstalować.* Podpisany LaunchAgent nasłuchujący bez uwierzytelniania
byłby gorszy od binarki odpalanej ręcznie — łatwiejszy w instalacji znaczy
wtedy tylko „łatwiejszy do nieświadomego wystawienia". Dlatego Fala 4 idzie
przed Falą 5 i nie ma od tego wyjątku.

> **Ta zasada została później zawieszona** decyzją właściciela z 2026-10-12
> (§„Decyzja właściciela 2026-10-12 — kolejność fal"): Fala 5 weszła przed
> Falą 4, a 2026-10-13 serwer został **opublikowany** bez uwierzytelniania,
> świadomie, jako wersja deweloperska. Argument powyżej się przez to nie
> zmienił — i dlatego uwierzytelnianie wraca na wysokie miejsce w §„Decyzje
> właściciela 2026-10-13".

---

## Fala 0 — Szkielet  → spec `F-001-skeleton.md`

Repo, CI, binarka startuje, generuje self-signed cert, nasłuchuje na porcie,
`ironrdp-server` przyjmuje połączenie i serwuje statyczny obraz testowy.

**Kryterium fali:** Windows App / mstsc pokazuje obraz testowy; zdarzenia
myszy i klawiatury widać w logach serwera.

**Po co ta fala:** oddziela „czy nasz łańcuch protokół+TLS+obraz działa" od
„czy nasz ScreenCaptureKit działa". Gdy w Fali 1 obraz będzie czarny, będziemy
wiedzieć, że wina jest po stronie capture — bo ścieżka wysyłania już jest
udowodniona.

**Zamknięta 2026-07-22.** Zweryfikowane prawdziwym klientem: obraz, kolejność
kanałów, orientacja i stride, żywy licznik, logi inputu, trzy cykle
połącz–rozłącz. Zapis w `features/F-001-skeleton.md` §7.

**Co ta fala już udowodniła dla następnych:**
- mapowanie `Bgra8` → `PixelFormat::BgrA32` jest poprawne (ADR-0005),
- stride ≠ `width * 4` przechodzi całą ścieżkę bez zniekształceń,
- `RdpServer::run()` sam obsługuje rozłączenia klienta — nie potrzebujemy
  własnej pętli reconnectu (ADR-0001).

---

## Fala 1 — Żywy ekran  → spec `F-002-live-screen.md`

ScreenCaptureKit → strumień klatek → `Frame` → `BitmapUpdate` → klient.

Zakres:
- `ScreenCaptureKitSource` implementujący `FrameSource` (ADR-0002),
- wybór ekranu z konfiguracji, ograniczenie FPS,
- koalescencja klatek przez `watch` (patrz `01-architecture.md` §4b),
- `perms`: wykrycie braku Screen Recording, czytelny komunikat, dialog systemowy,
  degradacja do wzorca testowego zamiast pustego ekranu.

**Kryterium fali:** widzę na kliencie żywy pulpit Maca; kolory się zgadzają;
odmowa uprawnienia daje zrozumiały komunikat, nie panikę.

**Zamknięta 2026-07-24.** Żywy pulpit, płynny, kolory i geometria poprawne,
strumień zatrzymuje się czysto. Po drodze naprawiony czarny ekran (ADR-0006:
serwer po cichu reklamował RemoteFX zamiast gołych bitmap). Zapis w
`features/F-002-live-screen.md` §7.

**Co ta fala udowodniła dla następnych:**
- `screencapturekit` wystawia `dirty_rects()` — F-004 bierze damage od systemu,
- pełny łańcuch capture→enkoder→klient jest sprawdzalny bez GUI
  (`examples/probe_client.rs`, `tests/rdp_pixels.rs`),
- Retina niesprawdzona (clamshell) — dług przeniesiony do F-003.

**Ryzyka:** stride ≠ `width * 4` (ADR-0005); ekran Retina ⇒ pulpit RDP ma
wymiary w pikselach backing store, nie w punktach.

---

## Fala 2 — Input  → specy `F-003-input.md` + `F-003a-keyboard-mapping.md`

Mysz (ruch, przyciski, scroll) i klawiatura (scancode → CGKeyCode).

Zakres:
- `CgEventSink` implementujący `InputSink` (ADR-0003),
- kanał bounded + zadanie inputu (`01-architecture.md` §4a),
- konwersja współrzędnych piksele pulpitu → punkty macOS (`scale_factor`),
- **osobny spec na klawiaturę** — tablica scancode set 1 → CGKeyCode,
  układ klawiatury, stan modyfikatorów, `Synchronize` (Caps/Num/Scroll lock),
- `perms`: brak Accessibility ⇒ ostrzeżenie (bo `CGEventPost` po cichu milczy).

**Kryterium fali:** klikam, przeciągam, scrolluję i piszę na kliencie —
Mac reaguje. Modyfikatory (Cmd, Shift, Alt, Ctrl) działają. Polskie znaki
działają lub mają udokumentowany, znany brak.

**Ryzyko:** mapowanie klawiatury to najtrudniejsza część MVP — zakładamy
osobną, pełną iterację, nie „przy okazji".

---

## Fala 3 — Jakość MVP  → specy F-004 … F-007

| Spec | Temat | Stan |
|---|---|---|
| F-007 | Stabilność — reconnect, sleep/wake, zmiana rozdzielczości | **Implemented** — AC-2 zaliczone po iter. 1b+1c; AC-3/AC-6 do weryfikacji zamykającej |
| F-004 | Damage tracking — wysyłanie tylko zmienionych regionów | **Implemented** — czeka na pomiar i weryfikację ręczną |
| F-005 | Kursor — `DisplayUpdate::{RGBAPointer, CachedPointer, DefaultPointer}` | **Draft** (2026-08-24) — czeka na approve; approve = zgoda na ADR-0010 |
| **ADR-0013** (MS-RDPBCGR §6 pkt 1) | Pomiar D-1 + korekta opisu ścieżki obrazu z ADR-0006: Surface Bits `codecID = 0` zamiast RDP 6.0 (P-01) | **Proposed** (2026-09-17) — pomiar D-1 u właściciela; przed F-005 |
| **F-003.22–F-003.26**, **F-003a.14–F-003a.18** (MS-RDPBCGR §6 pkt 2) | Input: Synchronize i koniec połączenia zwalniają trzymane klawisze i przyciski (P-04); surogaty UTF-16 i flagi zdarzeń Unicode (P-05) | F-003.22–26 **Verified** (2026-09-18); F-003a.14–18 i .19 **Implemented** — AC-10 czeka na klienta wysyłającego Unicode (FreeRDP / Windows App) |
| **F-008** (MS-RDPBCGR §6 pkt 3) | Suppress Output / Refresh Rect (P-06): przerwa w wysyłaniu, pełne odświeżenie przez ponowną aktywację; kandydat na przyczynę Cmd+Tab z F-004 AC-5 | **Implemented** (2026-09-18, G3) — czeka na test ręczny (BRAMKA C, AC-3…AC-9); przed F-005 |
| F-006 | Resize sesji — `RdpServerDisplay::request_layout` + MS-RDPEDISP | Spec do napisania |
| F-007 (iter. 2) | Przełączanie ekranów: F-007.4/.4a/.6/.6a/.7 | Odblokowane przez F-006 |

**Kolejność w tej fali jest wymuszona zależnościami, nie preferencją:**

```
F-007 iter.1b + 1c                       ← ZROBIONE (watchdog zastoju,
                                            odświeżanie geometrii wejścia)
        ↓
F-004 damage tracking                    ← największy zysk dla używalności
        ↓
poprawki zgodności MS-RDPBCGR            ← 02-ms-rdpbcgr.md §6 pkt 1–3
  (ADR-0013 ścieżka obrazu,                 (decyzja właściciela 2026-09-17)
   F-003.22+ / F-003a.14+ input,
   F-008 Suppress/Refresh)
        ↓
F-005 kursor sprzętowy                   ← zaraz po F-004 i poprawkach (patrz niżej)
        ↓
F-006 resize sesji
        ↓
F-007 iter.2 (przełączanie ekranów)      ← wymaga F-006 (F-007 §3, F-007.6a)
```

**Dlaczego F-005 zaraz po F-004, a nie „kiedyś".** Dziś kursor jest wypalany
w klatkę (`with_shows_cursor(true)`). Z chwilą, gdy F-004 zacznie wysyłać tylko
zmienione regiony, **każdy ruch myszy stanie się źródłem damage'u** — samo
przesuwanie kursora po statycznym pulpicie będzie generować ruch sieciowy.
F-005 (`shows_cursor(false)` + kursor jako PDU) nie jest więc kosmetyką:
domyka zysk z F-004 i przy okazji zdejmuje z kursora opóźnienie rundy RDP,
bo klient rysuje go lokalnie.

**Dlaczego poprawki zgodności przed F-005 (2026-09-17).** Przegląd
MS-RDPBCGR (`02-ms-rdpbcgr.md` §4) wykazał, że ADR-0006 opisuje inną ścieżkę
obrazu niż ta, która prawdopodobnie idzie po drucie (P-01) — a na tym opisie
stoją pomiary F-004 i założenia F-005. Z kolei P-06 (Suppress Output /
Refresh Rect) jest najmocniejszym kandydatem na niezaliczone F-004 AC-5.
Zamknięcie ich przed F-005 oznacza, że kursor powstaje na zweryfikowanej
ścieżce, a nie na założonej.

**Kwestia zamknięta w Fali 1 (ADR-0002):** `screencapturekit` **wystawia**
`dirty_rects()` przez `CMSampleBufferSCExt`. Zmiana bindingu niepotrzebna.
**Ale** — patrz ADR-0009 — samo wzięcie `dirty_rects()` z ostatniej klatki nie
wystarcza, bo kanał `watch` koalescuje klatki i damage klatek porzuconych
przepadłby. To jest główna decyzja do rozstrzygnięcia w F-004.

**Kryterium fali:** sesja nadaje się do pracy przez kilkadziesiąt minut;
płynność przy typowej pracy (przewijanie tekstu, przeglądarka) jest znośna;
uśpienie i wybudzenie Maca nie zabija serwera. Zysk z F-004 **zmierzony
liczbą** (bajty/s przed i po), nie odczuciem.

---

## Fala 4 — Bezpieczeństwo  → specy F-101, F-108

Fala, po której serwer **wolno** wystawić poza loopback. Dziś nie wolno:
`session::run` buduje serwer przez `with_tls(acceptor)` bez żadnego
uwierzytelniania, więc kto dojdzie do portu, ten steruje Makiem.

| Spec | Temat |
|---|---|
| F-101 | NLA / CredSSP — uwierzytelnianie **przed** sesją graficzną |
| F-108 | Hardening i model zagrożeń dla LAN |

**Zbadany stan IronRDP 0.13.0 (2026-08-07), determinujący projekt F-101:**
CredSSP po stronie serwera wymaga **odwracalnego hasła**.
`ironrdp-acceptor-0.10.0/src/lib.rs:170` składa `AuthIdentity { username,
password }` z `acceptor.creds` i podaje je `CredSspServer` jako
`CredentialsProxy`; NTLM to challenge-response, więc hash jednokierunkowy tam
nie wystarczy. Stąd hasło w **Keychainie**, a nie w pliku i nie jako hash.

`CredentialValidator` (0.13.0) tego nie zastąpi: działa **wyłącznie w trybie
TLS**, na poświadczeniach z `ClientInfoPdu` (`ironrdp-server-0.13.0/src/server.rs:146`,
`:1350`), czyli już po pełnej sekwencji połączenia. Zapisane w ADR-0012 jako
odrzucona alternatywa — inaczej za pół roku ktoś to „naprawi".

**Kryterium fali:** `mstsc` pyta o hasło **zanim** pokaże pulpit; złe hasło daje
odmowę i wpis w logu audytowym; serwer **odmawia startu** na adresie
nie-loopback, jeśli uwierzytelnianie nie jest skonfigurowane.

---

## Decyzje właściciela 2026-10-13

Cztery, podjęte po opublikowaniu dokumentacji i strony:

1. **Wydanie `v0.1.0` wychodzi teraz, testy ręczne potem.** Właściciel wybrał
   tę kolejność wiedząc, że `F-008 AC-5` (zrywanie połączenia przy ponownej
   aktywacji) nie zostało po poprawce sprawdzone żadnym klientem:
   „To jest jeszcze wersja developerska to można". Stąd `CHANGELOG.md` mówi
   wprost, że `0.1.0` jest wydaniem deweloperskim, i wylicza, czego nie umie.
2. **Następna praca to F-111** — ikonka w pasku menu z włącznikiem serwera.
   Wymaga ADR-a (Swift `NSStatusItem` vs Rust `objc2`) i według tabeli
   zależności niżej ciągnie za sobą F-106 i F-109.
3. **Wersjonowanie** — „Wprowadźmy też wersjonowanie jeśli jeszcze nie ma".
   Zrobione tego samego dnia: F-110.16 i F-110.17, `CHANGELOG.md`,
   `packaging/release-notes.sh`, bramka w `release.yml` i test w CI.
4. **Uwierzytelnianie wraca wysoko** — „gdzieś wysoko będzie trzeba zrobić
   autentykację". To jest powrót do pierwotnej zasady z 2026-08-07
   (§„Kierunek"), zawieszonej 2026-10-12. Konkret: **F-101 jest pierwszym
   specem do napisania po F-111**, a nie dalekim backlogiem. Dopóki go nie ma,
   `--bind-addr 0.0.0.0` zostaje scenariuszem „tylko domowa sieć, tylko na
   czas testu", a `README` i `SECURITY.md` mówią o tym w pierwszym akapicie.

**Testy ręczne mają od 2026-10-13 jedno miejsce:** `specs/testy-reczne.md`.
Zebrane są tam wszystkie 21 kryteriów, które wymagają prawdziwego klienta,
z komendami i miejscem na odpowiedź właściciela — na jego wniosek („zrób mi
dokument gdzie opisowo będzie napisane co trzeba zrobić. I tam dokładnie
opiszę odpowiadając w tym dokumencie"). Checklisty w specach zostają jako
źródło prawdy o tym, **co** trzeba sprawdzić; ten dokument jest o tym, **jak**
i z jakim wynikiem.

---

## Decyzja właściciela 2026-10-12 — kolejność fal

Właściciel przestawił kolejność: **produkt (Fala 5) i dźwięk (Fala 7) wchodzą
przed domknięciem weryfikacji Fali 3**, bo testy ręczne Fali 3 są odłożone.
Reguła „jedna fala naraz" z `CLAUDE.md` jest tu świadomie zawieszona, a nie
pominięta — ten wpis jest jej śladem.

Lista braków zgłoszona przez właściciela 2026-10-12, z przypisaniem:

| Brak | Gdzie trafia | Stan |
|---|---|---|
| Środkowy przycisk myszy nie działa | **ADR-0014** (2026-10-12, Proposed) — fork IronRDP, Etap 1 z ADR-0011 | pierwsze w kolejce, czeka na approve ADR-0014 |
| Brak dźwięku | F-103 (Fala 7) — spec do napisania | po środkowym przycisku |
| Instalacja przez Homebrew | F-110 (**Implemented 2026-10-12**) + F-106 + F-109 (Fala 5) | formuła, procedura wydania, README, licencje, `SECURITY.md`, strona — zrobione; `brew audit --strict` i `brew style` zielone. Do Verified: publikacja wydania i testy ręczne |
| Ikonka w pasku menu z włącznikiem serwera | **F-111** — nowy spec do napisania, zależy od F-106 i F-109 | — |
| **README, licencje, `SECURITY.md`** | F-110.8–F-110.10 — wyciągnięte **przed** resztą Fali 5 (decyzja właściciela 2026-10-12: „Wypadało by") | zrobione: **opublikowane 2026-10-13**, repo nie jest już publiczne bez ostrzeżenia |
| Strona projektu (GitHub Pages) | F-110.11 — **żyje od 2026-10-13** pod <https://dawidadamski.github.io/mac-rdp-server/>, generowana z README | najtańsze, niezależne |
| Przegląd aktualizacji pakietów | `screencapturekit` 8.0.1 → 11.0.0 i `toml` 0.9 → 1.1 wymagają ADR (ADR-0002 §Ryzyko); patche robione na bieżąco | 2026-10-12: zrobione rcgen, thiserror, clap, async-trait |

---

## Fala 5 — Produkt  → specy F-106, F-109, F-110

Fala, po której instalacja nie wymaga toolchaina Rusta w głowie użytkownika.

| Spec | Temat |
|---|---|
| F-106 | Bundle `.app`, `Info.plist`, własna tożsamość TCC, podpis |
| F-109 | Usługa launchd (LaunchAgent) + podkomendy CLI: `install`, `uninstall`, `status` |
| F-110 | Dystrybucja i dokumentacja: README, licencje, `SECURITY.md`, tap Homebrew, GitHub Pages, donacje — **Implemented 2026-10-12** (`F-110-dystrybucja.md`) |

**Dwa fakty o macOS, które kształtują tę falę:**

1. **LaunchAgent, nie LaunchDaemon.** Capture i wstrzykiwanie inputu muszą się
   dziać w sesji graficznej zalogowanego użytkownika (`gui/$UID`). Demon roota
   tego nie zrobi — to nie jest kwestia uprawnień do naprawienia, tylko tego,
   czym jest sesja okienkowa w macOS.
2. **Granty TCC gubią się po aktualizacji z dwóch niezależnych powodów.**
   Zmierzone 2026-10-12, szczegóły w F-110 §3:
   - *designated requirement* podpisu ad-hoc to **wyłącznie `cdhash`** — jawny,
     stabilny `--identifier` do DR nie trafia w ogóle, więc kombinowanie z nim
     nic nie daje;
   - Homebrew instaluje każdą wersję w **wersjonowanej ścieżce** Cellara,
     a TCC zapisuje dla binarki bez bundle'a ścieżkę — po `brew upgrade`
     powstaje nowy wpis, a stary zostaje na liście (yabai #1968, wypowiedź
     opiekuna).

   To zawęża F-106: **sam certyfikat self-signed nie wystarczy**, dopóki
   instalacja idzie w wersjonowaną ścieżkę. Stabilna tożsamość TCC wymaga
   stabilnej ścieżki, czyli bundle'a `.app` w stałym miejscu. Przy instalacji
   przez Homebrew dokumentujemy „nadaj uprawnienia ponownie i usuń stary wpis"
   (F-110.13) — tak samo jak `yabai` i `skhd`.

**Kanał dystrybucji przy braku notaryzacji.** `.dmg` pobrany przeglądarką
i artefakt instalowany **caskiem** dostają atrybut kwarantanny, więc bez
notaryzacji Gatekeeper je zablokuje. **Formuła w tapie Homebrew** omija to
czysto — i, co zmierzone 2026-10-12 (F-110 §3 „Kwarantanna"), dotyczy to także
formuły z **gotową binarką**: kwarantannę nakłada Homebrew Cask, nie formuła.
Dlatego ścieżką główną jest gotowa binarka z wydania (szybko, bez toolchaina
Rusta u użytkownika), a build ze źródeł zostaje jako `--HEAD`. Wcześniejsze
zdanie w tym miejscu („formuła budująca ze źródeł") było zbyt ostrożne —
poprawione.

**Kryterium fali:** na czystym koncie użytkownika, bez pomocy autora:
instalacja → nadanie dwóch uprawnień → ustawienie hasła → połączenie z Windowsa.

---

## Fala 6 — Komfort codziennej pracy  → specy F-102, F-003a

| Spec | Temat |
|---|---|
| F-102 | Clipboard (MS-RDPECLIP) — praktycznie warunek codziennego używania |
| F-003a | Klawiatura: układy, znaki narodowe, dead keys (wymaga ADR-0008) |
| F-105 | Wiele ekranów Maca na jednym ekranie klienta: tryby `single`/`switch`/`mosaic` |

**F-105 — sprostowanie założenia (2026-08-07).** RDP **obsługuje** wiele
monitorów po stronie klienta (`mstsc /multimon`, MS-RDPBCGR 2.2.1.3.6).
Blokadą jest silnik: `ironrdp-acceptor-0.10.0/src/connection.rs:786-796`
wysyła klientowi layout z **jednym** monitorem, na sztywno. Dlatego F-105 składa
N ekranów Maca na **jednym** pulpicie klienta, a prawdziwy multi-monitor zostaje
przy F-200 (własne MS-RDPBCGR) albo PR-ze upstream.

F-102 będzie **pierwszym kanałem wirtualnym** w projekcie, więc dokłada granicę
architektoniczną: logika schowka (NSPasteboard, formaty, negocjacja) nie wchodzi
do `session` — tam trafia wyłącznie adapter na `CliprdrServerFactory`.

---

## Fala 7 — Dźwięk  → spec `F-103-audio.md`

**Dodane 2026-08-25** na wniosek właściciela. Przeniesione z backlogu, gdzie
figurowało jako jedno zdanie bez miejsca w kolejności.

Przekierowanie **dźwięku systemowego Maca do klienta RDP** kanałem RDPSND
(MS-RDPEA): film odtwarzany na Macu słychać na Windowsie.

### Zbadany stan faktyczny (2026-08-25)

**Strona IronRDP — jest wszystko, czego potrzebujemy, bez zmian w `Cargo.toml`:**

| Element | Fakt | Źródło |
|---|---|---|
| Wpięcie | `RdpServerBuilder::with_sound_factory(Option<Box<dyn SoundServerFactory>>)` | `ironrdp-server-0.13.0/src/builder.rs:180` |
| **Brak feature gate** | W przeciwieństwie do `with_gfx_factory` (`#[cfg(feature = "egfx")]`, `builder.rs:186`) metoda dźwiękowa **nie jest** za flagą; `ironrdp-rdpsnd` jest zwykłą zależnością `ironrdp-server` | `ironrdp-server-0.13.0/Cargo.toml:120` |
| Traity | `SoundServerFactory::build_backend() -> Box<dyn RdpsndServerHandler>` | `ironrdp-server-0.13.0/src/sound.rs` |
| Kontrakt handlera | `get_formats()`, `choose_format(common: &[NegotiatedFormat])`, dane przez `RdpsndServer::wave(data, ts)` | `ironrdp-rdpsnd-0.9.0/src/server.rs:184` |
| Zdarzenia | `RdpsndServerMessage::{Wave, SetVolume, Close, Error}` | `ironrdp-rdpsnd-0.9.0/src/server.rs:17-31` |
| Formaty | `WaveFormat` to otwarty newtype `u16`; `PCM = 0x0001` | `ironrdp-rdpsnd-0.9.0/src/pdu/mod.rs:66,93` |

Warte odnotowania, bo oszczędza nam klasy błędów: `wFormatNo` — indeks formatu
na **liście klienta** — jest własnością crate'a, nie handlera. `NegotiatedFormat`
nie ma publicznego konstruktora, więc nie da się wysłać indeksu spoza zakresu.
Autorzy opisują to wprost jako naprawę starego API („the classic footgun of the
old index-returning API", `server.rs:36-47`).

**Strona macOS — też jest, też bez zmian w `Cargo.toml`:**

| Element | Fakt | Źródło |
|---|---|---|
| Włączenie | `SCStreamConfiguration::with_captures_audio(bool)` | `screencapturekit-8.0.1/src/stream/configuration/audio.rs:187` |
| Parametry | `AudioSampleRate` (domyślnie 48000 Hz), `AudioChannelCount` (mono/stereo) | `.../configuration/audio.rs:44,97` |
| Odbiór | Próbki przychodzą jako `SCStreamOutputType::Audio` | `.../stream/output_type.rs:31-32` |
| Feature | Gated przez `macos_13_0`, który nasz `macos_14_0` **już implikuje** | `screencapturekit-8.0.1/Cargo.toml:73-74` |

### Trzy rzeczy do rozstrzygnięcia, zanim powstanie spec

1. **Dźwięk jedzie tym samym `SCStream` co obraz.** To nie jest detal:
   watchdog z F-007 **przebudowuje strumień** przy zastoju (F-007.14), więc
   każda taka odbudowa przerwie także audio. Do decyzji w specu: osobny stream
   dla dźwięku (odporność za cenę drugiego strumienia) czy wspólny (prościej,
   ale zanik obrazu ucina dźwięk). Tego pytania nie było, gdy F-103 leżało
   w backlogu — pojawiło się dopiero razem z F-007.
2. **Format i opóźnienie.** PCM 48 kHz stereo to ~1,5 Mbit/s nieskompresowane.
   Do LAN wystarczy i jest najprostszą poprawną ścieżką na start; kompresja to
   osobna iteracja. Docelowy problem to nie pasmo, tylko **opóźnienie
   i synchronizacja z obrazem** — i to jest rzecz do zmierzenia, nie do
   założenia.
3. **TCC.** Hipoteza: przechwytywanie **dźwięku systemowego** mieści się
   w zgodzie na Screen Recording i nie wymaga osobnego uprawnienia. **Do
   potwierdzenia empirycznie w specu**, bo jeśli jest inaczej, zmienia to
   instrukcję instalacji z F-110 (trzecie uprawnienie do nadania).

### Dlaczego po Fali 6, a nie wcześniej

Bez schowka (F-102) serwer nie nadaje się do codziennej pracy; bez dźwięku
nadaje się, tylko jest uboższy. Kolejność idzie za tym, co blokuje używanie,
a nie za tym, co jest efektowniejsze w demo.

### Poza zakresem Fali 7

**Mikrofon klienta w stronę Maca.** SCK ma `SCStreamOutputType::Microphone`
(macOS 15+, `output_type.rs:33-37`), ale to jest przechwytywanie mikrofonu
**Maca**, czyli nie to samo. Przekierowanie mikrofonu *klienta* to osobne
rozszerzenie protokołu (MS-RDPEAI, kanał `AUDIO_INPUT`) i osobny spec — jeżeli
kiedykolwiek. Fala 7 to **wyłącznie kierunek Mac → klient.**

---

## Backlog — po jednym specu na temat

| ID | Temat | Uwagi ze zbadanego stanu IronRDP |
|---|---|---|
| F-104 | EGFX / nowsze kodeki | `ironrdp-server` ma feature `egfx` + `with_gfx_factory` (od 0.11). |
| F-107 | Wydajność: zero-copy / IOSurface | SCK oddaje IOSurface; `screencapturekit` ma `metal` i `is_backed_by_io_surface()`. |
| F-200 | **Własna implementacja MS-RDPBCGR** | Cel długoterminowy. Podmiana za granicą `session`. Plan i przegląd: `02-ms-rdpbcgr.md` §7 (etapy E0–E12 → F-201…F-212). **Pierwsza propozycja (2026-09-17): własny fork IronRDP rozwijany przez AI według speców — ADR-0011 (Proposed).** Start, gdy fale 0–3 będą Verified. |

**Przeniesione z backlogu do fal (2026-08-07):** F-101 → Fala 4,
F-102 i F-105 → Fala 6, F-106 → Fala 5. F-108, F-109 i F-110 to nowe ID nadane
w tej aktualizacji.

**Przeniesione z backlogu do fal (2026-08-25):** F-103 → Fala 7.

## Narzędzia (nie cel, ale trzymamy w zasięgu)

- `xfreerdp /v:127.0.0.1:3389 /cert:ignore -nla +glyph-cache /log-level:TRACE` —
  najgadatliwszy klient do debugowania.
- `SSLKEYLOGFILE=/tmp/rdp-keys.log` + Wireshark — deszyfrowanie TLS działa,
  bo `TlsIdentityCtx::make_acceptor()` włącza `rustls::KeyLogFile` (ADR-0004).
- `IRONRDP_LOG=trace` — logi wewnętrzne silnika protokołu.

---

## Uzupełnienie 2026-10-12 — co ustalił spec F-110

**Hipoteza „self-signed daje stabilny DR" jest teraz węższa, niż była.**
F-110 §3 zmierzył, że przy podpisie ad-hoc *designated requirement* to
**wyłącznie `cdhash`** — jawny `--identifier com.dawidadamski.mac-rdp-server`
nadany `codesign`iem do DR **nie trafia wcale**. Pierwsza wersja tego
uzupełnienia wskazywała na zmienny `Identifier` podpisu
(`mac_rdp_server-7139bc4f053fe608` vs `mac_rdp_server-b7ab3a30d18fac95`) jako
przyczynę — konkluzja była dobra, mechanizm zły, poprawione w F-110 §3.

Do tego doszła **druga, niezależna przyczyna**: wersjonowana ścieżka Cellara.
Eksperyment F-106 ma więc sens tylko razem ze stabilną ścieżką (bundle `.app`),
a nie na binarce instalowanej formułą — tam ponowne nadanie uprawnień jest
nieuniknione niezależnie od podpisu.

**Odpowiedź właściciela na pytanie o Apple Developer: nie.** Uzasadnienie
sprawdzone pomiarem (F-110 §3 „Jak robią to inni"): `yabai`, `skhd` i `borders`
to formuły podpisane ad-hoc, bez Developer ID i bez notaryzacji; koszt —
ponowne nadanie uprawnień po aktualizacji — dokumentują, a nie ukrywają.
Wpis „Podpisywanie | Brak Developer ID (na razie)" w §Cel Fal 4–6 przestaje
być „na razie" i staje się decyzją.

**Dokumentacja wyciągnięta przed kolejkę.** Repozytorium jest **publiczne**,
bez README, bez plików licencji i bez `SECURITY.md`, przy serwerze, który nie
ma uwierzytelniania. Właściciel zdecydował 2026-10-12 („Wypadało by"), że
F-110.8–F-110.10 idą **przed** resztą Fali 5. Startują po approve speca F-110.
