# F-101 — Uwierzytelnianie: hasło przed pulpitem

- **Fala:** 4
- **Status:** Approved (2026-10-21)
- **Data:** 2026-10-21
- **ADR:** ADR-0012 (NLA/CredSSP i hasło w Keychainie), ADR-0004 (TLS i certyfikat),
  ADR-0014 (silnik z forka)
- **Powiązane:** F-108 (hardening i model zagrożeń), F-106 (bundel `.app` —
  patrz §7 pkt 1), F-112 (plik konfiguracyjny), F-111 (pasek menu)

## 1. Cel

Klient musi podać poprawne hasło **zanim** zobaczy pulpit, a serwer bez
skonfigurowanego uwierzytelniania nie daje się wystawić poza loopback.

## 2. Zakres i granica

Ten spec obejmuje **jedno konto** i **jeden sekret**: nazwę użytkownika,
hasło w Keychainie, włączenie NLA w silniku, odmowę startu bez konfiguracji
i log audytowy. Konto RDP **nie jest kontem macOS** — nie ma tu PAM ani
sprawdzania hasła użytkownika Maca (ADR-0012 §Konsekwencje).

Ograniczanie liczby prób, blokady po nieudanych logowaniach i model zagrożeń
to **F-108**. Ten spec ma sprawić, że hasło jest w ogóle wymagane; F-108 ma
sprawić, że nie da się go zgadywać w nieskończoność.

Granica architektoniczna z CLAUDE.md zostaje: API silnika (`with_hybrid`,
`Credentials`) dotyka **wyłącznie** `src/session/`. Keychain i decyzja
„czy adres jest loopback" nie znają RDP.

## 3. Wymagania

### Tryb bezpieczeństwa połączenia

- **F-101.1** (MUST) Gdy uwierzytelnianie jest skonfigurowane, serwer MUSI
  być budowany przez `with_hybrid(acceptor, pub_key)`, a nie `with_tls`.
  Daje to `PROTOCOL_HYBRID | PROTOCOL_HYBRID_EX`, czyli CredSSP przed sesją.
- **F-101.2** (MUST) `pub_key` MUSI pochodzić z **tego samego**
  `TlsIdentityCtx`, z którego powstaje akceptor TLS (`src/session/mod.rs:79`).
  Nie wolno go wyliczać drugą drogą: rozjechanie klucza z certyfikatem daje
  błąd, który widać dopiero u klienta, w trakcie CredSSP.
- **F-101.3** (MUST) Poświadczenia MUSZĄ być podane silnikowi jako
  `Credentials { username, password, domain: None }`. `domain` zostaje `None`,
  dopóki nie ma AD (ADR-0012 §Odrzucone alternatywy pkt 6).
- **F-101.4** (MUST) Bez skonfigurowanego uwierzytelniania serwer MUSI
  zachować dzisiejsze zachowanie (`with_tls`) — żeby test na loopbacku
  i `--test-pattern` dalej działały bez sekretu.

### Skąd się bierze sekret

- **F-101.5** (MUST) Hasło MUSI być czytane z Keychaina: element typu
  *generic password*, usługa `mac-rdp-server`, konto = nazwa użytkownika.
- **F-101.6** (MUST) Hasło NIE MOŻE być przyjmowane ani z argumentów
  procesu, ani z pliku konfiguracyjnego. Argument wyląduje w historii powłoki
  i w `ps` u każdego użytkownika maszyny; plik wyląduje w backupie.
- **F-101.7** (MUST) Nazwa użytkownika MOŻE być w pliku konfiguracyjnym
  (F-112) albo we fladze — nie jest sekretem. Brak nazwy = uwierzytelnianie
  nieskonfigurowane.
- **F-101.8** (MUST) MUSI istnieć droga ustawienia hasła: tryb, który czyta
  je z terminala **bez echa** (a ze standardowego wejścia, gdy to nie
  terminal — żeby dało się zautomatyzować), zapisuje do Keychaina i kończy
  proces. Ten tryb NIE uruchamia serwera.
- **F-101.9** (SHOULD) Ustawianie pustego hasła POWINNO być odrzucone
  z komunikatem, a nie zapisane.

### Odmowa startu

- **F-101.10** (MUST) Adres nasłuchu **nie-loopback** bez skonfigurowanego
  uwierzytelniania MUSI być błędem: serwer nie startuje, kod wyjścia jest
  niezerowy, a komunikat mówi, co zrobić (ustawić hasło albo zbindować na
  loopback). To kryterium Fali 4 z `waves.md`.
- **F-101.11** (MUST) Nazwa użytkownika skonfigurowana, ale hasła nie ma
  w Keychainie → również odmowa startu, z **innym** komunikatem. Różnica
  „nie skonfigurowałeś" vs „skonfigurowałeś, ale sekretu nie ma" jest tym,
  co odróżnia pomyłkę od świadomej decyzji.
- **F-101.12** (MUST) Decyzja „czy ten adres jest loopback" MUSI być czystą
  funkcją pokrytą testami jednostkowymi. W szczególności `0.0.0.0` i `::`
  **nie są** loopbackiem — to adresy nasłuchu na wszystkim, czyli dokładnie
  ten przypadek, którego to wymaganie dotyczy.

### Log audytowy

- **F-101.13** (MUST) Każde **przyjęte** uwierzytelnienie MUSI dać jedną
  linię na poziomie `info` z adresem klienta i nazwą użytkownika.
- **F-101.14** (MUST) Każde **odrzucone** MUSI dać jedną linię na poziomie
  `warn` z adresem klienta, nazwą użytkownika (jeśli znana) i powodem.
- **F-101.15** (MUST) Hasło NIE MOŻE trafić do logu na żadnym poziomie,
  **w tym `trace`**. `Debug` dla `Credentials` już to gwarantuje po stronie
  silnika (ADR-0012 §Zbadany stan), ale nasz kod nie ma prawa na tym
  poprzestać — F-003a.19 pokazało, że logowanie na poziomie trace potrafi
  zapisać hasło wpisywane z klawiatury, i to jest osobna, znana sprawa.
- **F-101.16** (MUST) Linia startowa MUSI mówić, czy uwierzytelnianie jest
  włączone i dla jakiego użytkownika (`auth=nla user=…` albo `auth=none`),
  tak jak F-112.7 zrobiło z pochodzeniem konfiguracji.

### Pasek menu

- **F-101.17** (SHOULD) Menu POWINNO pokazywać, czy uwierzytelnianie jest
  włączone. Próba włączenia usługi na adresie nie-loopback bez hasła POWINNA
  dać stan „nie udało się" z powodem — maszyneria z F-111.11 już to umie,
  więc jest to kwestia komunikatu, nie nowego mechanizmu.
- **F-101.18** (MAY) Hasło MOŻE dać się zmienić bez restartu serwera przez
  `ServerEvent::SetCredentials` (ADR-0012 §Zbadany stan). Nie jest to
  potrzebne do spełnienia celu i nie wchodzi w kryteria akceptacji.

## 4. Odniesienia do dokumentacji

| Wymaganie | Źródło |
|---|---|
| F-101.1 | MS-RDPBCGR 2.2.1.1.1 `PROTOCOL_HYBRID` / `PROTOCOL_HYBRID_EX` (str. ~2089 w `specs/ref/`); 5.4.5.2 CredSSP; `builder.rs:90`, `server.rs:298` |
| F-101.1, F-101.14 | MS-RDPBCGR 5.4.5.2.1 User Authorization Failures → 3.3.5.7.1.1; 2.2.10.2 Early User Authorization Result PDU (wymagane przez HYBRID_EX) |
| F-101.2 | `ironrdp-server` `TlsIdentityCtx` (`helper.rs:15`), używany w `src/session/mod.rs:79` |
| F-101.3 | `Credentials` (`ironrdp-pdu/src/rdp/client_info.rs:195`) |
| F-101.5 | Apple, Keychain Services: `SecItemAdd`, `SecItemCopyMatching`, `kSecClassGenericPassword`; crate `security-framework` 3.7.0 |
| F-101.10 | `waves.md` §Fala 4, kryterium fali |
| ADR-0012 | pełny zbadany stan API z 2026-10-21, fork rev `ac4cd7db` |

## 5. Kryteria akceptacji

1. **AC-1** CI zielone: fmt, clippy `-D warnings`, testy, `cargo check --locked`.
2. **AC-2** **`mstsc` pyta o hasło, zanim pokaże pulpit.** Nie po pokazaniu,
   nie zamiast — przed. To jest całe wymaganie w jednym zdaniu.
3. **AC-3** **Złe hasło nie wpuszcza.** Klient dostaje odmowę, pulpit się nie
   pokazuje, a w logu jest linia `warn` z adresem klienta i nazwą użytkownika.
4. **AC-4** **Dobre hasło wpuszcza i nic nie psuje.** Obraz, mysz, klawiatura
   i dwuklik działają jak przed F-101.
5. **AC-5** **`--bind-addr 0.0.0.0` bez hasła nie wstaje.** Kod wyjścia
   niezerowy, komunikat mówi, co zrobić.
6. **AC-6** **`--bind-addr 127.0.0.1` bez hasła wstaje** i działa jak dotąd —
   inaczej każdy test diagnostyczny wymagałby sekretu.
7. **AC-7** **Ustawienie hasła działa i jest widoczne w systemie.** Po
   ustawieniu element `mac-rdp-server` jest w Dostępie do pęku kluczy,
   a hasła nie widać w historii powłoki (`history | grep` nie znajduje).
8. **AC-8** **Keychain nie pyta o pozwolenie przy każdym starcie.** Start,
   `cargo build --release`, start ponownie — bez okienka „chce użyć informacji
   poufnych". Przedtem było to pytanie otwarte mogące wymusić zmianę projektu;
   pomiar z 2026-10-21 (ADR-0012 §Pomiar) pokazał, że przebudowanie nie wywołuje
   okienka, więc kryterium zostaje jako regresja, nie jako niewiadoma.
9. **AC-8a** **To samo na binarce z wydania, ręcznie.** Ustaw hasło binarką
   z `target/release`, połącz się, potem `brew upgrade` do nowszej wersji
   i połącz się ponownie **bez ponownego ustawiania hasła**. Bez okienka.
   To jest właściwy test AC-8, bo kopia z brew i kopia lokalna mają różne
   tożsamości podpisu, a wydaniowa niesie identyfikator wyprowadzony z LC_UUID,
   zmienny przy każdym linkowaniu. Pomiar sondą tego **nie** obejmował
   (§7 pkt 1), więc jeśli okienko wyskoczy, to wyskoczy tutaj.
10. **AC-9** **Hasła nie ma w logu.** Pełna sesja z `MAC_RDP_LOG=trace`
    i poprawnym hasłem; `grep` po haśle w całym logu nie znajduje nic.
11. **AC-10** **Jedno konto, jedna nazwa.** Poprawne hasło z **inną** nazwą
    użytkownika niż skonfigurowana jest odrzucane (dowód, że sprawdzana jest
    para, a nie samo hasło).

## 6. Poza zakresem

- **Ograniczanie liczby prób, blokady, opóźnienia po nieudanym logowaniu** —
  F-108. Ten spec wymaga hasła; F-108 pilnuje, żeby nie dało się go zgadywać.
- **Kerberos i Active Directory** — silnik przekazuje `kerberos_config = None`,
  a w domowej sieci nie ma AD (ADR-0012).
- **Uwierzytelnianie kontem macOS** (PAM, hasło użytkownika systemu) —
  świadomie nie; konto RDP to osobny sekret do jednego celu.
- **Wiele kont** — jedno konto, jedno hasło. Więcej to nowy spec.
- **Szyfrowanie samego hasła naszym kodem** — to robi Keychain.
- **RDSTLS, RDS-AAD-Auth** (MS-RDPBCGR 5.4.5.3, 5.4.5.4) — poza MVP.

## 7. Ryzyka i otwarte kwestie

1. **Czy Keychain pyta o pozwolenie po przebudowaniu binarki — ZMIERZONE
   2026-10-21: nie pyta.** Pomiar opisany w ADR-0012 §Pomiar. Przebudowanie,
   kopia pod inną nazwą i w innym katalogu, bump wersji crate'a i ponowne
   podpisanie ad-hoc — żadne z tego nie wywołało okienka. Jednocześnie element
   **nie** jest otwarty dla wszystkich: obcy program ad-hoc i `/usr/bin/security`
   dostają okienko i odmowę. Czyli AC-8 jest osiągalne bez F-106, bez elementu
   „dla dowolnej aplikacji" i bez pliku `0600`.

   Co **nie** zostało rozdzielone pomiarem: wszystkie dopuszczone warianty
   pochodziły z jednego projektu cargo. Mechanizmu dopasowania nie udało się
   ustalić — cdhash, signing identifier, nazwa pliku, ścieżka i katalog zostały
   **wykluczone** jako kryterium (zmieniały się, a dostęp pozostawał cichy).
   Dlatego weryfikacja AC-8 na prawdziwej binarce zostaje **ręczna** (AC-8a
   poniżej): `target/release` kontra `/opt/homebrew/bin` po `brew upgrade`, bo
   te dwie kopie mają dziś różne tożsamości, a wydaniowa niesie identyfikator
   wyprowadzony z LC_UUID, zmienny przy każdym linkowaniu.
2. **NTLM jest kryptograficznie słaby.** To, co wybieramy, jest wymuszone
   przez protokół: RDP NLA to CredSSP, a CredSSP w praktyce to NTLM, gdy nie
   ma Kerberosa. Łagodzi to fakt, że cała wymiana idzie w środku TLS
   (ADR-0004), więc podsłuch w sieci lokalnej nie wystarcza. Zapisane, żeby
   nikt nie uznał tego za przeoczenie.
3. **FreeRDP.** Dziś nie kończy handshake'u TLS z tym serwerem
   (`waves.md` §Pomiar 2026-10-19b), więc diagnoza „FreeRDP nie działa"
   dostanie po F-101 **drugą możliwą przyczynę**. Do sprawdzenia po
   implementacji, jako diagnostyka — nie jest to kryterium akceptacji.
4. **Zmiana dotychczasowego sposobu pracy właściciela.** `--bind-addr 0.0.0.0`
   bez hasła przestanie wstawać. Jest to cel, nie skutek uboczny, ale warto
   wiedzieć przed implementacją, że pierwszy start po F-101 **wymaga
   ustawienia hasła**. Alternatywa „jawna flaga wyłączająca kontrolę" nie
   jest w wymaganiach **świadomie** — do decyzji właściciela, bo furtka na
   stałe w kodzie zwykle zostaje na stałe.
5. **Hasło w pamięci procesu.** Trzymamy je jako `String` w `Credentials`,
   bo tego wymaga API silnika. Zerowanie pamięci (`zeroize`) i ochrona przed
   zrzutem rdzenia to kandydaci do F-108, nie do tego speca.

## 8. Status

**Approved (2026-10-21).** Zaapprowane przez właściciela 2026-10-21.

Pomiar blokujący implementację **wykonany tego samego dnia** (ADR-0012
§Pomiar): Keychain nie pyta o pozwolenie po przebudowaniu binarki, a element
nie jest przy tym otwarty dla wszystkich aplikacji. Żadna z trzech rozważanych
ucieczek nie jest potrzebna — ani F-106 przed F-101, ani element „dla dowolnej
aplikacji", ani plik `0600`. **Wymagania zostają w tej formie i kod jest
odblokowany.**

Jedno zastrzeżenie przechodzi do kryteriów: AC-8a weryfikuje to samo na
binarce z wydania (`brew upgrade`), czego sonda nie objęła.
