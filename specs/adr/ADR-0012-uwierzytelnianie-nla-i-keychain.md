# ADR-0012 — Uwierzytelnianie: NLA/CredSSP, a hasło w Keychainie

- **Status:** Proposed (2026-10-21)
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

## Otwarta kwestia wymagająca pomiaru przed kodem

**Czy Keychain nie będzie pytał o pozwolenie przy każdym przebudowaniu
binarki.** Element Keychaina ma ACL powiązane z aplikacją, która go stworzyła,
a nasza binarka jest podpisana **ad-hoc** (`codesign --sign -`, F-110 §3) i leży
pod ścieżką, która zmienia się między `target/debug` a `/opt/homebrew`. Jeśli
tożsamość będzie się zmieniać, użytkownik dostanie okienko „mac-rdp-server
chce użyć informacji poufnych" przy każdym starcie po przebudowaniu — co jest
nie do przyjęcia dla usługi uruchamianej z `launchd` (F-109).

To ta sama rodzina problemów co uprawnienia TCC należące do terminala i jest
to **drugi powód**, dla którego F-106 (bundel `.app` z własną tożsamością)
przestaje być kosmetyką. Pomiar musi odpowiedzieć: czy zapis i odczyt
elementu z tej samej binarki przechodzi bez okienka, i co się dzieje po
`cargo build`. Dopóki nie ma odpowiedzi, F-101 nie powinien wejść w kod.

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
