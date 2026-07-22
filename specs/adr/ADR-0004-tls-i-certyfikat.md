# ADR-0004: Bezpieczeństwo w MVP — TLS-only + self-signed cert generowany przez `rcgen`

- **Status:** Accepted
- **Data:** 2026-07-22
- **Dotyczy:** F-001 (Fala 0)

## Kontekst

MVP ma się połączyć z Windows App (macOS) i mstsc.exe (Windows) po TLS, bez NLA.
`ironrdp-server` wystawia trzy tryby: `with_no_security()`, `with_tls(acceptor)`,
`with_hybrid(acceptor, pub_key)`.

## Decyzja

1. **Fala 0–3: `with_tls(acceptor)`** — Enhanced RDP Security z TLS 1.2/1.3.
   Bez NLA/CredSSP, zgodnie z definicją MVP.
2. **Certyfikat:** jeżeli w konfiguracji nie podano `cert`/`key`, binarka
   **generuje self-signed cert przy pierwszym starcie** (`rcgen` 0.14.8),
   zapisuje jako PEM do katalogu danych aplikacji i przy kolejnych startach
   go wczytuje. Ścieżki są konfigurowalne.
3. **Wczytanie:** `TlsIdentityCtx::init_from_paths(cert, key)` →
   `make_acceptor()` (feature `helper` w `ironrdp-server`).
   Rozpoznaje PEM po rozszerzeniu `.pem`, inaczej próbuje DER — piszemy `.pem`.
4. Cert zawiera CN + SAN: `localhost`, `127.0.0.1`, `::1` oraz hostname Maca
   (`.local`), żeby dało się łączyć i lokalnie, i z Windowsa po nazwie.

## Konsekwencje

- Klient **pokaże ostrzeżenie o tożsamości serwera** (nieznany CA + brak NLA).
  To jest oczekiwane zachowanie MVP i **musi być opisane w kryteriach akceptacji**
  F-001, żeby nie było traktowane jak błąd.
- `TlsIdentityCtx::make_acceptor()` instaluje `rustls::KeyLogFile` — czyli
  ustawienie `SSLKEYLOGFILE` daje deszyfrowanie w Wiresharku za darmo.
  **To jest nasze główne narzędzie debugowania protokołu** — do opisania w F-001.
- rustls wymaga zainstalowanego crypto providera. Jeśli w drzewie zależności
  znajdą się dwa (ring + aws-lc-rs), trzeba jawnie wywołać
  `CryptoProvider::install_default()` przed `make_acceptor()`. Do sprawdzenia
  przy pierwszym buildzie; jeśli konieczne — jawne wywołanie w `main`.

## Uwaga do przyszłej fali: NLA jest bliżej, niż się wydaje

Referencyjny przykład Devolutions (`crates/ironrdp/examples/server.rs`) ma
**`--sec hybrid` jako domyślne** i robi:
```rust
server_builder.with_hybrid(acceptor, identity.pub_key)   // CredSSP/NLA
...
server.set_credentials(Some(Credentials { username, password, domain: None }));
```
Od 0.12.0 jest też trait `CredentialValidator` + `ExactMatchCredentialValidator`.
Czyli **NLA po stronie serwera jest już w cracie** — to jedno wywołanie buildera
plus decyzja, skąd bierzemy poświadczenia (i czy w ogóle chcemy je trzymać).

Świadomie **nie** włączamy tego w MVP: NLA zmienia model bezpieczeństwa
(serwer musi weryfikować hasło zanim cokolwiek pokaże) i zasługuje na własny
spec z rozpisanym modelem zagrożeń. Trafia do backlogu jako F-1xx.

## Odrzucone alternatywy

- **`with_no_security()`** — odrzucone: mstsc i Windows App w praktyce wymagają
  co najmniej TLS; „gołe" RDP to ślepa uliczka nawet do testów.
- **Ręczne składanie `rustls::ServerConfig`** — odrzucone: `helper` daje to samo
  plus wyliczenie `pub_key` z certyfikatu (potrzebne później do `with_hybrid`).
- **Cert generowany w skrypcie / openssl w README** — odrzucone: „binarka startuje
  i działa" jest kryterium Fali 0; ręczny krok z openssl to tarcie na starcie.
