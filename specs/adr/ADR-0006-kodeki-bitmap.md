# ADR-0006: Nie reklamujemy kodeków bitmap — zostajemy przy RDP 6.0

- **Status:** Accepted
- **Data:** 2026-07-22
- **Dotyczy:** F-001, F-002, w przyszłości F-004 i F-104
- **Powód powstania:** czarny ekran u klienta w Fali 1

## Kontekst

Fala 1 dawała u klienta **czarny ekran**, mimo że:
- sesja RDP nawiązywała się poprawnie (zdarzenia myszy dochodziły i były logowane),
- ScreenCaptureKit oddawał poprawne klatki (zrzut do pliku: 6 467 329 z 7 372 800
  pikseli miało niezerowe RGB),
- wszystkie 56 testów było zielonych.

Uszkodzenie było **między enkoderem a drutem** — w miejscu, którego żaden
z dotychczasowych testów nie dotykał.

## Zbadany stan faktyczny

`RdpServerBuilder` ma metodę `with_bitmap_codecs(BitmapCodecs)`. **Nie wywoływaliśmy
jej.** Domyślna wartość ustawiana w builderze to `server_codecs_capabilities(&[])`
— i to jest pułapka: **pusty wycinek nie znaczy „żadnych kodeków", tylko
„domyślne"**, czyli RemoteFX + ImageRemoteFX + QOI + QOIZ (features `qoi`, `qoiz`
są domyślnie włączone w `ironrdp-server`).

Wybór kodeka w `UpdateEncoder::new` (`encoder/mod.rs:132-159`) ma priorytet:

```
qoiz → qoi → remotefx → BitmapUpdater::None (gołe bitmapy RDP 6.0)
```

Kodek trafia do użycia, gdy **obie** strony go reklamują. QOI i QOIZ są własnymi
kodekami IronRDP, spoza MS-RDPBCGR, więc prawdziwy klient (mstsc, Windows App,
FreeRDP) ich nie zna — ale **RemoteFX reklamuje praktycznie każdy**. Efekt:
zamiast Bitmap Update szły Surface Bits.

### Pomiar

Napisana przy tej okazji sonda (`examples/probe_client.rs`) łączy się z naszym
serwerem jako prawdziwy klient RDP i liczy niepuste piksele. Wzorzec testowy
1280×720, ten sam serwer, zmieniana tylko lista kodeków reklamowanych przez klienta:

| klient reklamuje | niepuste piksele |
|---|---|
| nic | **917 504 / 921 600 (99,6%)** |
| `remotefx` | 0 / 921 600 (0%) |
| `qoi` | 0 / 921 600 (0%) |
| `qoiz` | 0 / 921 600 (0%) |

Po poprawce, prawdziwy ekran 5120×1440, niezależnie od tego, co reklamuje klient:
**6 467 329 / 7 372 800 (87,7%)** — dokładnie tyle, ile ma klatka źródłowa.

## Decyzja

`session::run` **jawnie** ustawia pustą listę kodeków:

```rust
.with_bitmap_codecs(BitmapCodecs(Vec::new()))
```

To nie jest opcjonalne i nie wolno tego usunąć „bo przecież domyślne".
Dopóki nie powstanie spec na EGFX/RemoteFX (F-104), serwer ma zostać na ścieżce
Bitmap Update z kompresją RDP 6.0 — dokładnie to, co deklarują ADR-0001 i F-002.

## Konsekwencje

- Implementacja wreszcie **robi to, co mówi zatwierdzony spec**. Wcześniej
  rozjazd był cichy: spec mówił „bitmap RDP 6.0", kod wysyłał Surface Bits.
- Wydajność zostaje na poziomie gołych bitmap. Świadomie: najpierw ma być
  poprawnie, potem szybko. RemoteFX/EGFX wracają jako F-104, z własnym specem
  i z tą sondą jako narzędziem weryfikacji.
- Powstał test regresyjny `tests/rdp_pixels.rs`, który podnosi serwer, łączy się
  prawdziwym klientem **reklamującym RemoteFX** i sprawdza piksele. Zweryfikowane:
  po cofnięciu poprawki test pada z komunikatem
  `only 0.0% of pixels were non-black`.

## Czego ten incydent nauczył o testach

Zielone były: testy jednostkowe geometrii, konwersji, certyfikatu, oraz test
integracyjny X.224 + TLS. Żaden nie patrzył na **piksele po stronie klienta**.
Granica, na której nikt nie asercjonował, to dokładnie ta, na której projekt padł.

Stąd zasada na kolejne fale: **każda fala, która zmienia to, co widzi użytkownik,
dostaje asercję na pikselach u klienta**, a nie tylko na danych wejściowych.

## Otwarta kwestia (nie blokuje Fali 1)

Nie ustalono, **dlaczego** ścieżka kodekowa daje czerń. Analiza źródeł
`ironrdp-server` 0.13.0 wykluczyła kanał alfa (żaden z enkoderów go nie czyta
dla `BgrA32`) i przepełnienia arytmetyczne (cała arytmetyka kafelkowania jest
na `usize`). Najlepszy niepotwierdzony trop: przy klatce większej niż
`max_request_size` (domyślnie 8 MiB) włącza się `split_diff`
(`encoder/mod.rs:277-316`), a przy RemoteFX każdy pasek dostaje własną ramkę
`FrameBeginPdu { index: 0 }` (`rfx.rs:60-92`). Przy 5120×1440 klatka ma 29,5 MB,
więc ta ścieżka jest zawsze aktywna; przy 1280×720 (3,7 MB) nie była.

To trzeba rozstrzygnąć **przed** F-104 — nie wcześniej. Wtedy też należy
sprawdzić, czy to błąd IronRDP wart zgłoszenia upstream.
