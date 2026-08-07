# 00 — Wizja

**Status:** Approved (2026-07-22); §5 i §9 zrewidowane 2026-08-07 (Draft — do approve'u)
**Data:** 2026-07-22
**Ostatnia zmiana:** 2026-08-07 — rewizja non-goals po domknięciu MVP, nowa §9 (dystrybucja)

## 1. Po co to jest

Pełnoprawny serwer RDP dla macOS, napisany w Rust — realna alternatywa dla
VNC / Screen Sharing dla ludzi, którzy chcą dostać się do Maca z klienta RDP
(mstsc.exe na Windows, Windows App na macOS/iOS, FreeRDP na Linuksie).

Dziś na macOS nie ma dobrego serwera RDP. Screen Sharing mówi po VNC —
protokole bez porządnej kompresji, bez dynamicznego resize, bez kanałów
wirtualnych (clipboard, audio, drukarki, przekierowanie urządzeń).

## 2. Cel długoterminowy

Serwer, który dla klienta RDP wygląda jak zwykły host RDP:
żywy ekran z sensowną wydajnością, pełny input, clipboard, audio,
wiele monitorów, dynamiczny resize sesji, uwierzytelnianie na poziomie sieci.

Docelowo warstwa protokołu ma być **nasza** — implementowana wprost wg
MS-RDPBCGR i pokrewnych — a nie zapożyczona. Do tego dochodzimy falami,
podmieniając moduły za stabilnymi granicami (patrz `01-architecture.md`).

## 3. Zasada nadrzędna

**Najpierw coś, co DZIAŁA. Potem coraz więcej specyfikacji Microsoftu.**

Każda fala kończy się stanem, który da się ręcznie sprawdzić prawdziwym
klientem RDP. Nie budujemy warstw „na zapas".

## 4. MVP — definicja „działa"

Klient (Windows App na Macu **lub** mstsc.exe na Windows):
1. łączy się po TLS z self-signed certem (ostrzeżenie o certyfikacie jest OK),
2. widzi żywy pulpit Maca,
3. steruje myszą i klawiaturą.

**W MVP nie ma:** NLA, clipboardu, audio, wielu monitorów, przekierowań.
Jeden monitor, kodowanie bitmap (RDP 6.0 compression).

## 5. Non-goals (świadomie poza zakresem, także długoterminowo)

- **Klient RDP.** Piszemy tylko serwer.
- **Inne platformy.** Tylko macOS. Zero warstw abstrakcji „na wszelki wypadek"
  pod Windows/Linux.
- **Sesje wielu użytkowników / RDS.** Serwujemy konsolę zalogowanego użytkownika,
  nie tworzymy osobnych sesji jak Windows Server. **To jest fakt istotny dla
  bezpieczeństwa i prywatności, nie tylko techniczny:** osoba stojąca przy Macu
  widzi dokładnie to samo, co widzi klient zdalny. Musi to być powiedziane
  wprost w dokumentacji użytkownika (F-110), a nie odkrywane w praktyce.
- **Własna implementacja TLS ani kryptografii.** rustls i już.
- **GUI aplikacji.** Binarka CLI + plik konfiguracyjny. GUI ewentualnie kiedyś,
  jako osobny projekt na tym samym rdzeniu.
  **Doprecyzowanie (2026-08-07):** bundle `.app` z `LSUIElement = true` **nie
  łamie** tego non-goala. To bundle bez interfejsu — bez okna, bez ikony
  w Docku, bez menu. Jest potrzebny wyłącznie dlatego, że macOS przypisuje
  uprawnienia TCC do tożsamości aplikacji; bez własnego bundle'a serwer
  dziedziczy uprawnienia Terminala, a to znaczy, że instrukcja instalacji
  brzmi „nadaj iTermowi prawo do nagrywania ekranu". Bundle jest nośnikiem
  tożsamości, nie interfejsem.

### Non-goal wygasły

- ~~**Produkcyjny model uprawnień / hardening** przed MVP. Najpierw ma
  działać.~~ — **wygasło 2026-08-07.** MVP z §4 jest osiągnięte (Fale 0–2
  Verified), więc powód odłożenia zniknął. Hardening i uwierzytelnianie
  stają się **Falą 4** i są warunkiem koniecznym §9. Do czasu ich wdrożenia
  obowiązuje reguła: serwer nasłuchuje wyłącznie na loopbacku.

## 6. Słownik pojęć

| Pojęcie | Znaczenie w tym projekcie |
|---|---|
| **Fala (wave)** | Zamknięty etap prac kończący się ręcznie weryfikowalnym stanem. Patrz `waves.md`. |
| **Spec (F-XXX)** | Jeden dokument w `specs/features/` = jedna funkcjonalność. Kod powstaje wyłącznie dla speców w statusie Approved. |
| **ADR** | Architecture Decision Record — jedna istotna decyzja techniczna, z kontekstem i konsekwencjami. Nieodwracalna po cichu. |
| **Frame** | Jedna klatka obrazu z `capture`: bufor pikseli + wymiary + stride + format. |
| **Stride** | Liczba bajtów na wiersz w buforze. **Zwykle większa niż `width * 4`.** Patrz ADR-0005. |
| **Damage / dirty region** | Prostokąt, który zmienił się od ostatniej wysłanej klatki. Fala 3. |
| **Scancode** | Kod klawisza wysyłany przez RDP (PC set 1, make/break + prefiks `E0`). |
| **CGKeyCode** | Wirtualny kod klawisza macOS, zależny od układu klawiatury. Nie to samo co scancode. Patrz ADR-0003. |
| **TCC** | *Transparency, Consent and Control* — system uprawnień macOS. Potrzebujemy **Screen Recording** (capture) i **Accessibility** (input). |
| **NLA / CredSSP** | Network Level Authentication — uwierzytelnienie przed nawiązaniem sesji graficznej. Poza MVP, patrz ADR-0004. |
| **FastPath** | Zoptymalizowany format PDU w RDP (MS-RDPBCGR 2.2.8.1.2 / 2.2.9.1.2). Obsługiwany przez IronRDP. |
| **MS-RDPBCGR** | Podstawowa specyfikacja RDP: connection, graphics, input. Główne źródło wymagań. |

## 7. Kluczowe dokumenty źródłowe

| Skrót | Tytuł | Do czego |
|---|---|---|
| MS-RDPBCGR | Remote Desktop Protocol: Basic Connectivity and Graphics Remoting | handshake, bitmapy, input |
| MS-RDPEGDI | Graphics Device Interface Acceleration Extensions | rozkazy rysowania |
| MS-RDPECLIP | Clipboard Virtual Channel Extension | clipboard (backlog) |
| MS-RDPEA | Audio Output Virtual Channel Extension | audio (backlog) |
| MS-RDPEDISP | Display Control Virtual Channel Extension | resize sesji (Fala 3) |
| Apple: ScreenCaptureKit | <https://developer.apple.com/documentation/screencapturekit> | capture |
| Apple: Quartz Event Services | <https://developer.apple.com/documentation/coregraphics/quartz_event_services> | input |

### Źródła kanoniczne dla protokołu

Przy wątpliwościach dotyczących RDP sięgamy najpierw tutaj:

- **MS-RDPBCGR (HTML)** — <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpbcgr/5073f4ed-1e93-45e1-b039-6e30c385867c>
  Nawigowalne sekcje; to jest źródło do cytowania w polu „Odniesienia do dokumentacji" speców.
- **MS-RDPBCGR (PDF)** — <https://winprotocoldocs-bhdugrdyduf5h2e4.b02.azurefd.net/MS-RDPBCGR/%5bMS-RDPBCGR%5d.pdf>
  Pełny dokument, wygodny do przeszukiwania offline.
- **Understanding RDP (Microsoft Learn)** — <https://learn.microsoft.com/en-us/troubleshoot/windows-server/remote/understanding-remote-desktop-protocol>
  Przystępny opis stosu i kolejności nawiązywania połączenia.
- **Wireshark wiki: RDP** — <https://wiki.wireshark.org/RDP>
  Warstwy i typy PDU „z drutu"; główne źródło przy debugowaniu z `SSLKEYLOGFILE`.

## 8. Metoda pracy

Spec-driven. Kolejność jest sztywna: **spec → approve → plan → kod → weryfikacja
względem kryteriów akceptacji**. Szczegóły w `CLAUDE.md`.

## 9. Model wydania i dystrybucji

**Dodane 2026-08-07.** Do tej pory wizja opisywała, *co* budujemy. Ta sekcja
mówi, *komu to oddajemy i na jakich warunkach* — bo od tego zależą wymagania
Fal 4 i 5.

### Czym to ma być dla użytkownika

**Darmowe oprogramowanie open source**, z możliwością dobrowolnego wsparcia
autora. Nie ma wersji płatnej, nie ma funkcji za paywallem, nie ma telemetrii.
Donacja jest podziękowaniem, nie zakupem — nie kupuje priorytetu w zgłoszeniach
ani obietnicy wsparcia.

Licencja: **MIT OR Apache-2.0** (jak już deklaruje `Cargo.toml`; pliki licencji
dochodzą w F-110).

### Definicja „da się zainstalować"

Użytkownik, który nie zna Rusta i nie chce znać, wykonuje:

1. jedno polecenie instalacji,
2. nadanie dwóch uprawnień w System Settings (Screen Recording, Accessibility),
3. ustawienie hasła dostępu,
4. połączenie klientem RDP.

Każdy z tych kroków musi mieć czytelny komunikat, gdy się nie uda. Kontrakt
z `01-architecture.md` — brak uprawnienia nigdy nie panikuje, zawsze mówi, co
kliknąć — rozciąga się na instalację.

### Definicja „da się bezpiecznie używać"

Serwer zdalnego dostępu to cel ataku. Trzy niezmienniki, których nie negocjujemy
po Fali 4:

1. **Nasłuch poza loopbackiem wymaga skonfigurowanego uwierzytelniania.**
   Bez niego serwer odmawia startu — z instrukcją, nie z błędem.
2. **Uwierzytelnienie poprzedza sesję graficzną** (NLA/CredSSP). Klient nie
   widzi ani jednego piksela przed zalogowaniem.
3. **Hasło dostępu jest osobne od hasła konta macOS.** Serwer nigdy nie prosi
   o poświadczenia systemowe i nigdy ich nie przechowuje. Kompromitacja serwera
   nie oddaje konta.

### Ograniczenie przyjęte świadomie: brak Developer ID

Właściciel nie ma konta Apple Developer Program (decyzja 2026-08-07: „nie
teraz"). Konsekwencje, żeby nie były odkrywane w trakcie:

- Brak notaryzacji ⇒ artefakt pobrany przeglądarką zostanie zablokowany przez
  Gatekeeper. Dlatego kanałem głównym jest **tap Homebrew budujący ze źródeł**,
  a nie `.dmg` do pobrania.
- Podpis ad-hoc zmienia się przy każdym buildzie ⇒ granty TCC mogą wymagać
  ponownego nadania po aktualizacji. F-106 sprawdza, czy lokalny certyfikat
  self-signed daje stabilną tożsamość i rozwiązuje to.

Projekt ma być zbudowany tak, żeby **dołożenie Developer ID było zmianą
konfiguracji wydania, nie zmianą architektury.**
