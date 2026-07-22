# CLAUDE.md — zasady pracy w tym repo

Projekt: serwer RDP dla macOS w Rust. Metoda: **spec-driven development**.

## Zasada zero

**Żadna funkcjonalność nie powstaje bez wpisu w specyfikacji.**
Kolejność jest sztywna:

```
spec (Draft) → approve właściciela → kod → weryfikacja wg kryteriów akceptacji → Verified
```

Kod może powstać **wyłącznie** dla wymagań ze speca w statusie **Approved**.
Specy approwuje właściciel projektu, nie asystent.

Jeżeli w trakcie implementacji okaże się, że wymaganie jest złe, niepełne albo
niewykonalne: **zatrzymaj się, zgłoś, zaproponuj zmianę speca lub ADR.**
Nie „napraw po cichu w kodzie".

## Struktura repo

```
specs/
  00-vision.md        wizja, non-goals, słownik
  01-architecture.md  moduły, granice, przepływ danych
  waves.md            roadmapa fal (dokument żywy)
  adr/                ADR-XXXX-tytul.md — decyzje techniczne
  features/           F-XXX-nazwa.md — jedna funkcjonalność = jeden spec
src/                  kod
CLAUDE.md             ten plik
```

## Format speca funkcjonalności

Każdy `specs/features/F-XXX-*.md` ma dokładnie te sekcje:

1. **Cel** — jedno zdanie.
2. **Wymagania** — numerowane `F-XXX.N`, każde testowalne, z MUST/SHOULD/MAY.
3. **Odniesienia do dokumentacji** — sekcje MS-RDPBCGR / MS-RDPEGDI / docs Apple.
4. **Kryteria akceptacji** — obserwowalne zachowania (co widzę w kliencie /
   w logach / w teście).
5. **Poza zakresem** — jawnie, żeby nie pełzł scope.
6. **Status** — Draft → Approved → Implemented → Verified.

## ADR

Każda istotna decyzja techniczna to osobny plik `specs/adr/ADR-XXXX-tytul.md`
z sekcjami: Kontekst, Decyzja, Konsekwencje, Odrzucone alternatywy.

**Zmiana zdania = aktualizacja speca + nowy ADR, nigdy cichy refactor.**
Stary ADR dostaje status Superseded z linkiem do nowego — nie kasujemy historii.

ADR powinien zawierać **zbadany stan faktyczny z datą** (wersje cratów, sygnatury
API, feature flags), nie tylko konkluzję. Za pół roku to jest cała wartość dokumentu.

## Commity

Każdy commit odwołuje się do ID wymagania:

```
F-002.3: ScreenCaptureKitSource emituje klatki BGRA z poprawnym stride
```

Commit bez ID wymagania jest dopuszczalny tylko dla: aktualizacji speców,
CI, formatowania.

## Granice architektoniczne — nienegocjowalne bez ADR

1. **Tylko `src/session/` importuje `ironrdp_server::*`.** Reszta kodu operuje
   na naszych typach (`Frame`, `FrameUpdate`, `PointerEvent`, `KeyEvent`).
   To jest warunek celu długoterminowego: podmiany IronRDP na własną
   implementację MS-RDPBCGR. Egzekwowane testem w CI (F-001.12).
2. **`capture` nie zna RDP. `session` nie zna macOS.**
3. **`RdpServerInputHandler::keyboard/mouse` są synchroniczne** — nic w nich
   nie blokuje. Konwersja + `try_send` na bounded kanał i tyle.
4. **Klatki się koalescują** (`tokio::sync::watch`), nie kolejkują. Wolny klient
   ma dostać mniej klatek, ale aktualnych.

## Testy

- **Jednostkowe** — dla całej logiki czystej: konwersje klawiszy, geometria,
  arytmetyka stride, diff ekranu, precedencja konfiguracji. To ma być domyślne.
- **Integracyjne** — wszędzie, gdzie da się bez GUI (np. TCP+TLS handshake).
- **Ręczne** — część macOS-owa (capture, input, uprawnienia TCC): checklista
  w sekcji „Kryteria akceptacji" odpowiedniego speca. Każdy spec dotykający
  macOS MUSI mieć taką checklistę.

Arytmetyka na buforze pikseli **zawsze** przez `y * stride + x * bpp`,
nigdy `y * width * bpp`. Patrz ADR-0005.

## Weryfikacja fali

Fala dostaje status **Verified** dopiero, gdy:
1. wszystkie kryteria akceptacji jej speców są spełnione,
2. CI jest zielone (fmt, clippy `-D warnings`, test),
3. właściciel ręcznie przetestował prawdziwym klientem RDP.

Dopiero wtedy zaczyna się następna fala. Nie robimy dwóch fal naraz.

## Zależności

Craty kluczowe dla projektu (`ironrdp-server`, `screencapturekit`) są **pinowane
dokładną wersją** (`=x.y.z`) — oba mają bardzo szybkie tempo breaking changes
(ADR-0001 §Ryzyko, ADR-0002 §Ryzyko). `Cargo.lock` jest w repo.
Bump wersji = świadoma decyzja + przejrzenie CHANGELOG-a + wpis w ADR, jeśli
zmienia API, którego używamy.

## Debugowanie

| Problem | Narzędzie |
|---|---|
| Klient się nie łączy | `xfreerdp /v:127.0.0.1:3389 /cert:ignore -nla /log-level:TRACE` |
| Coś w protokole | `IRONRDP_LOG=trace` |
| Trzeba zobaczyć ruch | `SSLKEYLOGFILE=/tmp/rdp-keys.log` + Wireshark (`make_acceptor()` włącza `rustls::KeyLogFile`) |
| Nasze logi | `MAC_RDP_LOG=debug` |

## Język

Specy, ADR-y i ten plik: **po polsku**. Kod, nazwy, komentarze w kodzie,
komunikaty logów i commit messages: **po angielsku**.
