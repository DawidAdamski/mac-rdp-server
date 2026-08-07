# Roadmapa fal

**Status:** Approved (2026-07-22) — dokument żywy, aktualizowany po każdej fali
**Data ostatniej aktualizacji:** 2026-08-07

## Reguła

Fala jest **Verified** dopiero, gdy: (a) wszystkie kryteria akceptacji jej speców
są spełnione, (b) właściciel projektu ręcznie przetestował prawdziwym klientem RDP.
Dopiero wtedy zaczyna się następna fala.

| Fala | Nazwa | Specy | Status |
|---|---|---|---|
| 0 | Szkielet | F-001 | **Verified** (2026-07-22) |
| 1 | Żywy ekran | F-002 | **Verified** (2026-07-24) |
| 2 | Input | F-003, F-003a | **F-003 Verified** (2026-07-27); F-003a — do decyzji |
| 3 | Jakość MVP | F-004…F-007 | In progress (F-007 Implemented; F-004/005/006 do napisania) |
| 4 | Bezpieczeństwo | F-101, F-108 | Planned |
| 5 | Produkt | F-106, F-109, F-110 | Planned |
| 6 | Komfort | F-102, F-003a, F-105 | Planned |
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
| Podpisywanie | **Brak Developer ID** (na razie) | Ścieżka ad-hoc/self-signed; dystrybucja przez tap Homebrew, nie notaryzowany `.dmg` |
| Model wydania | Darmowy OSS + opcjonalne donacje | F-110: README, licencje, `SECURITY.md`, `FUNDING.yml` |

**Zasada porządkująca te trzy fale:** *najpierw serwer wolno wystawić, potem
łatwo zainstalować.* Podpisany LaunchAgent nasłuchujący bez uwierzytelniania
byłby gorszy od binarki odpalanej ręcznie — łatwiejszy w instalacji znaczy
wtedy tylko „łatwiejszy do nieświadomego wystawienia". Dlatego Fala 4 idzie
przed Falą 5 i nie ma od tego wyjątku.

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
| F-004 | Damage tracking — wysyłanie tylko zmienionych regionów | Spec do napisania |
| F-005 | Kursor — `DisplayUpdate::{ColorPointer, RGBAPointer, PointerPosition, HidePointer}` | Spec do napisania |
| F-006 | Resize sesji — `RdpServerDisplay::request_layout` + MS-RDPEDISP | Spec do napisania |
| F-007 (iter. 2) | Przełączanie ekranów: F-007.4/.4a/.6/.6a/.7 | Odblokowane przez F-006 |

**Kolejność w tej fali jest wymuszona zależnościami, nie preferencją:**

```
F-007 iter.1b + 1c                       ← ZROBIONE (watchdog zastoju,
                                            odświeżanie geometrii wejścia)
        ↓
F-004 damage tracking                    ← największy zysk dla używalności
        ↓
F-005 kursor sprzętowy                   ← MUSI iść zaraz po F-004 (patrz niżej)
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

## Fala 5 — Produkt  → specy F-106, F-109, F-110

Fala, po której instalacja nie wymaga toolchaina Rusta w głowie użytkownika.

| Spec | Temat |
|---|---|
| F-106 | Bundle `.app`, `Info.plist`, własna tożsamość TCC, podpis |
| F-109 | Usługa launchd (LaunchAgent) + podkomendy CLI: `install`, `uninstall`, `status` |
| F-110 | Dystrybucja i dokumentacja: README, licencje, `SECURITY.md`, tap Homebrew, donacje |

**Dwa fakty o macOS, które kształtują tę falę:**

1. **LaunchAgent, nie LaunchDaemon.** Capture i wstrzykiwanie inputu muszą się
   dziać w sesji graficznej zalogowanego użytkownika (`gui/$UID`). Demon roota
   tego nie zrobi — to nie jest kwestia uprawnień do naprawienia, tylko tego,
   czym jest sesja okienkowa w macOS.
2. **TCC kluczuje granty po *designated requirement* podpisu.** Podpis ad-hoc
   zmienia cdhash przy każdym buildzie, więc bez stabilnej tożsamości
   użytkownik nadaje Screen Recording i Accessibility po każdej aktualizacji.
   Hipoteza do zweryfikowania empirycznie w F-106: **lokalnie wygenerowany
   certyfikat self-signed** daje stabilny DR i granty przeżywają aktualizację.
   To jest główna różnica w komforcie użytkownika przy braku Developer ID —
   albo się potwierdzi, albo dokumentujemy „nadaj uprawnienia ponownie".

**Kanał dystrybucji przy braku notaryzacji.** `.dmg` pobrany przeglądarką
dostaje atrybut kwarantanny i Gatekeeper go zablokuje. **Tap Homebrew z formułą
budującą ze źródeł** omija to czysto — artefakt zbudowany lokalnie kwarantanny
nie dostaje. To ścieżka główna; `.dmg` z instrukcją „prawy klik → Otwórz"
najwyżej jako dodatek.

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

## Backlog — po jednym specu na temat

| ID | Temat | Uwagi ze zbadanego stanu IronRDP |
|---|---|---|
| F-103 | Audio (MS-RDPEA) | IronRDP ma `SoundServerFactory` + `RdpsndServerHandler` (formaty OPUS/PCM). Po naszej stronie: przechwycenie audio systemowego (SCK `macos_13_0`). |
| F-104 | EGFX / nowsze kodeki | `ironrdp-server` ma feature `egfx` + `with_gfx_factory` (od 0.11). |
| F-107 | Wydajność: zero-copy / IOSurface | SCK oddaje IOSurface; `screencapturekit` ma `metal` i `is_backed_by_io_surface()`. |
| F-200 | **Własna implementacja MS-RDPBCGR** | Cel długoterminowy. Podmiana za granicą `session`. Rozbić na PDU-po-PDU, gdy fale 0–3 będą Verified. |

**Przeniesione z backlogu do fal (2026-08-07):** F-101 → Fala 4,
F-102 i F-105 → Fala 6, F-106 → Fala 5. F-108, F-109 i F-110 to nowe ID nadane
w tej aktualizacji.

## Narzędzia (nie cel, ale trzymamy w zasięgu)

- `xfreerdp /v:127.0.0.1:3389 /cert:ignore -nla +glyph-cache /log-level:TRACE` —
  najgadatliwszy klient do debugowania.
- `SSLKEYLOGFILE=/tmp/rdp-keys.log` + Wireshark — deszyfrowanie TLS działa,
  bo `TlsIdentityCtx::make_acceptor()` włącza `rustls::KeyLogFile` (ADR-0004).
- `IRONRDP_LOG=trace` — logi wewnętrzne silnika protokołu.
