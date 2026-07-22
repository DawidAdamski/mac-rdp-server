# 00 — Wizja

**Status:** Approved (2026-07-22)
**Data:** 2026-07-22

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
  nie tworzymy osobnych sesji jak Windows Server.
- **Własna implementacja TLS ani kryptografii.** rustls i już.
- **GUI aplikacji.** Binarka CLI + plik konfiguracyjny. GUI ewentualnie kiedyś,
  jako osobny projekt na tym samym rdzeniu.
- **Produkcyjny model uprawnień / hardening** przed MVP. Najpierw ma działać.

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
