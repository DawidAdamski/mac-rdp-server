# ADR-0012 — Uwierzytelnianie: NLA/CredSSP, a hasło w Keychainie

- **Status:** Accepted (2026-10-21). Źródło sekretu: Keychain, z jednorazową zgodą na zainstalowaną binarkę (§Pomiar 2026-10-21c)
- **Dotyczy:** F-101 (uwierzytelnianie), F-108 (hardening), F-106 (bundel `.app`)
- **Zastępuje:** nic. Pierwszy ADR o uwierzytelnianiu.

## Kontekst

Dziś `session::run` buduje serwer przez `with_tls(acceptor)` bez żadnego
uwierzytelniania: kto dojdzie do portu, ten steruje Makiem. Właściciel używa
`--bind-addr 0.0.0.0` w domowej sieci, więc to nie jest hipotetyczne.

Pierwszy przegląd API zrobiono 2026-08-07 na `ironrdp-server` 0.13.0
z crates.io. Od 2026-10-12 silnik pochodzi z **naszego forka mastera**
(ADR-0014), więc przed napisaniem F-101 cały stan przebadano ponownie.

### Zbadany stan faktyczny — 2026-10-21, fork rev `ac4cd7db`

| fakt | miejsce |
|---|---|
| NLA włącza się jednym wywołaniem buildera: `with_hybrid(acceptor, pub_key)` | `crates/ironrdp-server/src/builder.rs:90` |
| …co daje `SecurityProtocol::HYBRID \| HYBRID_EX` | `crates/ironrdp-server/src/server.rs:298` |
| `pub_key` **już mamy** — `TlsIdentityCtx`, którego używamy w `src/session/mod.rs:79`, wystawia `pub_key: Vec<u8>` obok `make_acceptor()` | `crates/ironrdp-server/src/helper.rs:15` |
| Silnik woła `accept_credssp(..., public_key, kerberos_config)` z `kerberos_config = None` — czyli **NTLM, bez Kerberosa i bez AD** | `crates/ironrdp-server/src/server.rs:883-891` |
| CredSSP po stronie serwera potrzebuje **odwracalnego hasła**: z `acceptor.creds` składany jest `AuthIdentity { username, password }` i podawany `CredsspSequence` | `crates/ironrdp-acceptor/src/lib.rs:170-181` |
| `Credentials { username, password, domain: Option<String> }`, a jego `Debug` **celowo nie pokazuje hasła** | `crates/ironrdp-pdu/src/rdp/client_info.rs:195-207` |
| Hasło można zmienić w działającym serwerze: `ServerEvent::SetCredentials` → `set_credentials` | `server.rs:481`, `server.rs:1760` |
| Serwerowy NTLM pochodzi z craty `sspi` **0.21.3** | `Cargo.lock` |

Dlaczego to NTLM, a nie hash: NTLM jest challenge-response. Serwer musi
policzyć odpowiedź z tego samego sekretu co klient, więc hash jednokierunkowy
(bcrypt, argon2) **nie wystarczy** — niezależnie od tego, że byłby
bezpieczniejszy w spoczynku. To ograniczenie protokołu, nie biblioteki.

## Decyzja

1. **NLA przez `with_hybrid`**, gdy uwierzytelnianie jest skonfigurowane.
   `with_tls` zostaje wyłącznie dla adresu loopback bez konfiguracji.
2. **Hasło trzyma Keychain**, nie plik konfiguracyjny i nie flaga. Dostęp
   przez cratę **`security-framework` 3.7.0** (dziś nieobecną w drzewie),
   element typu *generic password*, usługa `mac-rdp-server`, konto = nazwa
   użytkownika.
3. **Nazwa użytkownika** może być w pliku konfiguracyjnym (F-112) albo we
   fladze — to nie sekret. **Hasło nigdy nie przechodzi przez argumenty
   procesu**: wylądowałoby w historii powłoki i w `ps` u każdego użytkownika
   maszyny.
4. **Serwer odmawia startu na adresie nie-loopback**, jeśli uwierzytelnianie
   nie jest skonfigurowane. To kryterium Fali 4, zapisane w `waves.md`.
5. **Każda próba uwierzytelnienia zostawia jedną linię w logu** — przyjęta
   i odrzucona. Hasła nie logujemy nigdy; `Debug` dla `Credentials` już to
   gwarantuje po stronie silnika.

## Konsekwencje

- **Klient musi umieć NLA.** `mstsc` umie. FreeRDP i tak nie kończy dziś
  handshake'u TLS z tym serwerem (`waves.md` §Pomiar 2026-10-19b), więc nie
  jest to regresja, ale po F-101 trzeba to sprawdzić ponownie, bo diagnoza
  „FreeRDP nie działa" dostanie drugą możliwą przyczynę.
- **Dotychczasowy sposób pracy właściciela przestanie działać**:
  `--bind-addr 0.0.0.0` bez hasła nie wstanie. Koszt jednorazowy — ustawienie
  hasła raz — i to jest cel tej zmiany, nie skutek uboczny.
- **Hasło musi być w Keychainie, zanim serwer wstanie.** Czyli potrzebna jest
  droga do jego ustawienia; F-101 ją definiuje.
- **Nie dotykamy uwierzytelniania systemowego.** Konto RDP nie jest kontem
  macOS: nie ma PAM, nie ma `/etc/passwd`, nie ma sprawdzania hasła
  użytkownika Maca. To osobny sekret do jednego celu. Gdyby kiedyś miało być
  inaczej, to nowy ADR.
- **`unsafe_code = "deny"` zostaje.** `security-framework` zamyka `unsafe`
  w sobie, więc nasz kod pozostaje bezpieczny — inaczej niż przy `objc2`
  w F-111, gdzie trzeba było wyjątku.

## Pomiar (2026-10-21) — pierwsze podejście, **wniosek błędny**

Pierwotnie ta sekcja brzmiała „Otwarta kwestia wymagająca pomiaru przed kodem"
i zawierała prognozę, że Keychain **będzie** pytał po każdym przebudowaniu,
bo element ma ACL powiązane z aplikacją, która go stworzyła, a nasza binarka
jest podpisana ad-hoc (`codesign --sign -`, F-110 §3) pod zmienną ścieżką.
**Prognoza była błędna.** Zapisana tu wraz z pomiarem, bo była powodem, dla
którego F-106 miał wyprzedzić F-101.

Narzędzie: osobny projekt cargo w scratchpadzie (`security-framework` 3.7.0,
`passwords::{set,get,delete}_generic_password`), usługa `mac-rdp-server-probe`,
żeby prawdziwy element nigdy nie był ruszany. Pęk: `login.keychain-db`.

### Co zmierzono o tożsamości podpisu

Designated requirement binarki podpisanej ad-hoc to **dosłownie hash tego
jednego pliku** — nie „ten program", nie „ten wydawca":

```
# designated => cdhash H"d0b5486c81d1a420f9b7ba3489a3dc99863f9cec"
```

Build **nie jest reprodukowalny**: `cargo build`, dopisanie linii komentarza
i przywrócenie źródła do stanu wyjściowego dały trzy różne cdhashe
(`ee501cf0…`, `0fde73b7…`, `d0b5486c…`). Czyli nie trzeba zmieniać kodu, żeby
powstała binarka o nowym cdhashu.

Po `codesign --force --sign -` (to robi `.github/workflows/release.yml:55`)
identyfikator ma postać `nazwa-pliku` + hex napisu `UUID` + **LC_UUID Mach-O**:

```
Identifier = bin-B-resigned-55554944b0b3aadf093f354f8226b39e9a13f14b
LC_UUID    =                B0B3AADF-093F-354F-8226-B39E9A13F14B
```

LC_UUID zmienia się przy każdym linkowaniu. Bump wersji crate'a zmienia też
identyfikator linker-signed nadawany przez cargo (`keychain_probe-f47c8586…`
→ `keychain_probe-f69f06bd…`). Dziś `target/release/mac-rdp-server`
(`mac_rdp_server-3f737d0f…`) i `/opt/homebrew/bin/mac-rdp-server`
(`mac-rdp-server-5555494412c0…`) mają **różne tożsamości**.

### Co zmierzono o dostępie do elementu

Element utworzony sondą, a potem czytany:

| czytający | tożsamość | wynik |
|---|---|---|
| ta sama binarka | — | cicho |
| kopia bajt w bajt, inna nazwa pliku | ten sam cdhash | cicho |
| kopia w innym katalogu, i w katalogu domowym | ten sam cdhash | cicho |
| **przebudowana binarka** | nowy cdhash | **cicho** |
| binarka po bumpie wersji crate'a | nowy cdhash i nowy identifier | **cicho** |
| binarka po `codesign -f -s -` | identifier z LC_UUID | **cicho** |
| **zupełnie obcy program ad-hoc** | inny crate, inna nazwa | **okienko, odmowa** (37,8 s do anulowania) |
| `/usr/bin/security -w` | `com.apple.security` | **okienko** (zawisło na limicie 20 s) |

Kontrola, że okienko `security` nie jest artefaktem samego narzędzia:
`security add-generic-password` + `security find-generic-password -w` na
własnym elemencie przechodzi **cicho**, zarówno z ACL domyślnym, jak i z `-A`.
Czyli odmowa z wiersza przedostatniego jest prawdziwą odmową ACL.

### Wnioski, które z tego wyciągnięto — i dlaczego były błędne

Wyciągnięto wtedy trzy wnioski: że AC-8 jest osiągalne, że element nie jest
przy tym otwarty dla wszystkich, i że żadna z trzech ucieczek nie jest
potrzebna. **Wszystkie trzy są nieważne**, bo kolumna „wynik" w tabeli powyżej
mierzyła co innego, niż się wydawało.

Błąd metody, nazwany wprost, bo to on jest tu wart zapamiętania: mierzono
**czy odczyt się udał**, a nie **czy pojawiło się okienko**. Okienko Keychaina
pojawia się na ekranie właściciela, a nie w wyjściu procesu; każde „cicho"
w tabeli znaczyło w rzeczywistości „właściciel zobaczył okienko i wpisał
hasło". Sygnał, który by to złapał, był pod ręką i nie został użyty: **czas**.
Jedyne dwa wiersze, przy których czas zmierzono (37,8 s i 20 s), opisano jako
„odmowa ACL" — a to również był właściciel, tylko klikający „Odmów".

Patrz §Pomiar 2026-10-21b poniżej.

## Pomiar 2026-10-21b — Keychain pyta przy **każdym** odczycie

Zgłoszenie właściciela w trakcie implementacji F-101: „te wszystkie keychainy
to mnie pyta o hasło i ja muszę manualnie wprowadzać". To unieważniło pomiar
powyżej i zostało powtórzone na prawdziwej binarce serwera, z czasem jako
miarą.

### Zmierzone

Trzy kolejne starty `target/debug/mac-rdp-server --auth-user … --bind-addr
0.0.0.0`, **tą samą binarką**, bez żadnego przebudowania pomiędzy. Mierzony
odstęp między linią `starting mac-rdp-server` i pierwszą linią po odczycie
Keychaina:

| start | odstęp |
|---|---|
| 1 | 11,65 s |
| 2 | 7,94 s |
| 3 | 9,55 s |

To nie koszt kryptografii. To czas, w którym człowiek czyta okienko i wpisuje
hasło. Czwarty start, po przebudowaniu, zawisł na 45-sekundowym limicie i nie
wstał, bo właściciel zaczął klikać „Odmów".

Dalej, i to jest wniosek najważniejszy: element utworzony **z ACL „dostęp dla
dowolnej aplikacji"** (`security add-generic-password -A`) też wywołał okienko,
gdy czytał go nasz kod:

| element | ACL | czytający | wynik |
|---|---|---|---|
| `f101-anyapp-test` | `-A`, dowolna aplikacja | nasza binarka ad-hoc | **okienko**, odmowa (44,9 s) |
| `f101-noacl-test` | domyślny | nasza binarka ad-hoc | **okienko**, odmowa (2,97 s) |
| oba powyższe | — | `/usr/bin/security` | cicho |

### Wnioski (ten raz oparte na obserwacji okienka, nie na kodzie wyjścia)

1. **AC-8 nie przechodzi**, i to szerzej niż mówiło samo kryterium: Keychain
   pyta nie „po przebudowaniu binarki", ale **przy każdym starcie**.
2. **Pierwotna prognoza z tego ADR-a była słuszna.** Binarka podpisana ad-hoc
   nie ma stabilnej tożsamości, którą ACL mógłby zapamiętać, więc nie da się
   jej na tę listę wpisać trwale.
3. **Ucieczka „element dla dowolnej aplikacji" nie działa.** To nie ACL
   elementu decyduje, tylko brak tożsamości czytającego. Odrzucona nie
   dlatego, że jest niebezpieczna — dlatego, że jest nieskuteczna.
4. `/usr/bin/security` czyta swoje elementy cicho, bo jest podpisany przez
   Apple i trafia na listę zaufanych aplikacji jako konkretna tożsamość. To
   kontrprzykład pokazujący, że mechanizm działa — tylko nie dla nas.

### Pomiar 2026-10-21c — „Zawsze zezwalaj" działa i ginie z cdhashem

Wykonany tego samego dnia, na wniosek właściciela, z jego świadomym
kliknięciem. **To domyka mechanizm.**

Jedno kliknięcie „Zawsze zezwalaj" w okienku, a potem trzy kolejne starty tą
samą binarką (cdhash `389669d1`), bez przebudowania:

| start | odczyt Keychaina |
|---|---|
| 1 | 0,010 s |
| 2 | 0,007 s |
| 3 | 0,008 s |

Milisekundy wobec 8–12 sekund przed kliknięciem, i zero okienek. Potem
przebudowanie — cdhash `389669d1` → `b1275899` — i pierwszy start **znów
wywołał okienko**.

**Mechanizm, wreszcie zmierzony, nie zgadnięty:** zgoda jest wiązana
z **cdhashem** binarki i przestaje obowiązywać dokładnie wtedy, gdy cdhash się
zmienia. To jest to, co pierwotna prognoza tego ADR-a mówiła od początku
(§Pomiar 2026-10-21b wniosek 2), a czego dwa poprzednie pomiary nie potrafiły
potwierdzić — pierwszy, bo mierzył kod wyjścia, drugi, bo właściciel wszystko
odrzucał.

### Co to znaczy w praktyce

| sytuacja | koszt |
|---|---|
| codzienne przebudowywanie w trakcie pracy nad kodem | jedno okienko **na każdy build**, który startuje z hasłem |
| binarka z brew, raz zainstalowana | jedno okienko **raz**, potem cicho |
| `brew upgrade` do nowej wersji | jedno okienko **raz po każdym upgrade** |
| usługa z `launchd` (F-109) bez zalogowanej sesji | **nie do przyjęcia** — nie ma komu kliknąć |

Czyli AC-8 w literalnym brzmieniu („start, `cargo build --release`, start
ponownie — bez okienka") **nadal nie przechodzi**, ale cel użytkowy jest
osiągalny bez F-106: na zainstalowanej binarce to jedno kliknięcie po
instalacji. F-106 pozostaje konieczne dla **F-109**, bo usługa w tle nie ma
komu pokazać okienka — i to jest teraz jego najmocniejsze uzasadnienie, obok
uprawnień TCC należących do terminala.

### Decyzja (2026-10-21)

Właściciel wybrał najpierw zmierzyć „Zawsze zezwalaj", i wynik rozstrzygnął:
**Keychain zostaje, a kosztem jest jedno kliknięcie na zainstalowaną binarkę.**

Odrzucone w konsekwencji:

1. **Plik `0600` zamiast Keychaina** — niepotrzebny. Keychain z jednorazową
   zgodą daje to samo wygodnie i nie trafia do backupu jawnym tekstem.
2. **Element „dla dowolnej aplikacji"** — nie tyle niebezpieczny, co
   nieistniejący: zmierzono, że też wywołuje okienko (§Pomiar 2026-10-21b).

**F-106 nie blokuje F-101** i uwierzytelnianie wchodzi teraz. F-106 blokuje
natomiast **F-109** (launchd), bo usługa w tle nie ma komu pokazać okienka;
dla F-109 prawdziwa tożsamość podpisu przestaje być kosmetyką i staje się
warunkiem.

## Odrzucone alternatywy

1. **Hasło w pliku konfiguracyjnym.** Jawnym tekstem w TOML-u, który użytkownik
   trzyma w `~/.config`, kopiuje w backupach i czasem wrzuca do repozytorium
   z dotfile'ami. Keychain jest szyfrowany i zamykany razem z sesją
   użytkownika.
2. **Hash hasła zamiast hasła.** Niewykonalne: NTLM to challenge-response
   (patrz Kontekst). Zapisane tutaj, żeby za pół roku nikt tego nie
   „naprawił" jako oczywistego ulepszenia.
3. **`CredentialValidator` zamiast NLA.** Trait istnieje
   (`server.rs:190-215`) i ma gotową implementację `ExactMatchCredentialValidator`,
   ale jest wołany **wyłącznie** na ścieżce bez HYBRID
   (`server.rs:1350`, a `result.credentials` wypełnia się tylko gdy protokół
   nie przecina HYBRID — `connection.rs:694`). Czyli: poświadczenia czytane
   z `ClientInfoPdu`, już po pełnej sekwencji połączenia, hasło jawnym tekstem
   w środku TLS. To kontrola dostępu, nie uwierzytelnianie przed sesją.
4. **Porównanie poświadczeń w akceptorze na ścieżce standard RDP security.**
   Istnieje (`connection.rs:694-713`) i odmawia przez `ServerDeniedConnection`,
   ale sam upstream oznaczył to komentarzem `FIXME: How authorization should be
   denied with standard RDP security?`. Ta sama wada co wyżej: po sekwencji.
5. **Crate `keyring` 4.2.0** zamiast `security-framework`. Ładne API, ale to
   warstwa abstrakcji nad wieloma systemami w projekcie, który jest wyłącznie
   macOS-owy i w którym do innych frameworków Apple sięgamy wprost.
6. **Kerberos / Active Directory.** `kerberos_config = None` jest tym, co
   silnik przekazuje, a w domowej sieci nie ma AD. Poza zakresem.
