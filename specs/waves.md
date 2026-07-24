# Roadmapa fal

**Status:** Approved (2026-07-22) — dokument żywy, aktualizowany po każdej fali
**Data ostatniej aktualizacji:** 2026-07-22

## Reguła

Fala jest **Verified** dopiero, gdy: (a) wszystkie kryteria akceptacji jej speców
są spełnione, (b) właściciel projektu ręcznie przetestował prawdziwym klientem RDP.
Dopiero wtedy zaczyna się następna fala.

| Fala | Nazwa | Specy | Status |
|---|---|---|---|
| 0 | Szkielet | F-001 | **Verified** (2026-07-22) |
| 1 | Żywy ekran | F-002 | **Verified** (2026-07-24) |
| 2 | Input | F-003, F-003a | Następna |
| 3 | Jakość MVP | F-004…F-007 | Planned |
| — | Backlog | F-1xx | Planned |

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

| Spec | Temat |
|---|---|
| F-004 | Damage tracking — wysyłanie tylko zmienionych regionów |
| F-005 | Kursor — `DisplayUpdate::{ColorPointer, RGBAPointer, PointerPosition, HidePointer}` |
| F-006 | Resize sesji — `RdpServerDisplay::request_layout` + MS-RDPEDISP |
| F-007 | Stabilność — reconnect, sleep/wake Maca, zmiana rozdzielczości, wiele kolejnych sesji |

**Kryterium fali:** sesja nadaje się do pracy przez kilkadziesiąt minut;
płynność przy typowej pracy (przewijanie tekstu, przeglądarka) jest znośna;
uśpienie i wybudzenie Maca nie zabija serwera.

**Kwestia zamknięta w Fali 1 (ADR-0002):** `screencapturekit` **wystawia**
`dirty_rects()` przez `CMSampleBufferSCExt`. F-004 bierze damage od systemu,
a własny diff klatek zostaje najwyżej fallbackiem, gdy atrybut jest pusty.
Zmiana bindingu niepotrzebna.

---

## Backlog — po jednym specu na temat

| ID | Temat | Uwagi ze zbadanego stanu IronRDP |
|---|---|---|
| F-101 | NLA / CredSSP | `with_hybrid(acceptor, pub_key)` + `CredentialValidator` **już są** w cracie (ADR-0004). Potrzebny model zagrożeń i decyzja o źródle poświadczeń. |
| F-102 | Clipboard (MS-RDPECLIP) | IronRDP ma `CliprdrServerFactory`; od 0.11 także transfer plików. Po naszej stronie: NSPasteboard. |
| F-103 | Audio (MS-RDPEA) | IronRDP ma `SoundServerFactory` + `RdpsndServerHandler` (formaty OPUS/PCM). Po naszej stronie: przechwycenie audio systemowego (SCK `macos_13_0`). |
| F-104 | EGFX / nowsze kodeki | `ironrdp-server` ma feature `egfx` + `with_gfx_factory` (od 0.11). |
| F-105 | Multi-monitor | Wymaga `FrameSource` z wieloma `DisplayInfo` i MS-RDPEDISP. |
| F-106 | Bundle .app + Info.plist | `NSScreenCaptureUsageDescription`, podpisywanie, własna tożsamość TCC zamiast dziedziczenia po Terminalu. |
| F-107 | Wydajność: zero-copy / IOSurface | SCK oddaje IOSurface; `screencapturekit` ma `metal` i `is_backed_by_io_surface()`. |
| F-200 | **Własna implementacja MS-RDPBCGR** | Cel długoterminowy. Podmiana za granicą `session`. Rozbić na PDU-po-PDU, gdy fale 0–3 będą Verified. |

## Narzędzia (nie cel, ale trzymamy w zasięgu)

- `xfreerdp /v:127.0.0.1:3389 /cert:ignore -nla +glyph-cache /log-level:TRACE` —
  najgadatliwszy klient do debugowania.
- `SSLKEYLOGFILE=/tmp/rdp-keys.log` + Wireshark — deszyfrowanie TLS działa,
  bo `TlsIdentityCtx::make_acceptor()` włącza `rustls::KeyLogFile` (ADR-0004).
- `IRONRDP_LOG=trace` — logi wewnętrzne silnika protokołu.
