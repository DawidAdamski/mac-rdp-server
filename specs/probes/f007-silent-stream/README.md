# Sonda: strumień przechwytywania milknie bez błędu (F-007.34, AC-10)

## Co mierzy

Awarię z **F-007 §8g**: `SCStream` żyje, ale ScreenCaptureKit przestaje wołać
delegata — nie przychodzi ani klatka, ani klatka `Idle`, ani odrzut. Przed
F-007.30 watchdog nie miał czego zliczać, więc odzyskiwanie nie startowało
i obraz zostawał zamrożony do restartu procesu. Reconnect nie pomagał, bo
źródło klatek jest wspólne dla całego procesu.

## Dlaczego w ogóle istnieje

Bo tej awarii **nie da się wyczekać**. W boju pojawiła się raz (2026-10-19,
13 minut zamrożonego obrazu), a następnego dnia nie wystąpiła ani razu przez
12 godzin 57 minut ciągłej pracy (§8h). Bez sondy weryfikacja F-007.30
czekałaby tygodniami na kolejny przypadek.

## Jak wywołuje awarię

Z osi czasu z §8g: wyzwalaczem był **drugi proces przechwytujący ten sam
ekran**, który się zakończył. Osiem sekund po jego zamknięciu pierwszy
strumień zamilkł na zawsze.

```
zsh run.sh ../../../target/release/mac-rdp-server
```

1. uruchamia serwer A, czeka na pierwszą klatkę, mierzy `received`,
2. uruchamia serwer B na innym porcie, czeka na jego pierwszą klatkę,
3. zatrzymuje B czysto (`SIGINT`) — tak, jak zakończył się proces w §8g,
4. obserwuje A przez 90 sekund i czyta jego log.

Trzyma przy tym ekran obudzony (`caffeinate -d`), żeby nie pomieszać tej
awarii z uśpieniem ekranu z §8e.

## Jak czytać wynik

| Wynik | Znaczenie |
|---|---|
| **PASS** | strumień zamilkł, watchdog to zauważył (`stopped calling us entirely`), `capture recovered`, `received` znowu rośnie |
| **FAIL** (cisza bez wznowienia) | wykrycie działa, odzyskiwanie nie |
| **FAIL** (cisza niezauważona) | dokładnie luka z §8g — `received` stoi, a w logu nic |
| **INCONCLUSIVE** | awaria się nie wywołała; to wyścig w ScreenCaptureKit, nie pewnik. Powtórzyć albo zabić drugi proces przez `-9` |
| **SKIP** | drugi proces nie przechwycił klatki, czyli wyzwalacz nie został w ogóle zaaplikowany |

`INCONCLUSIVE` **nie jest** zaliczeniem AC-10 — to brak pomiaru.

## Czego sonda nie sprawdza

Druga połowa AC-10, „obraz wraca u podłączonego klienta **bez rozłączania**",
wymaga prawdziwego klienta RDP z widocznym oknem. Sonda nie ma klienta, więc
dowodzi tylko, że serwer sam z siebie wraca do produkowania klatek. To samo
ograniczenie dotyczy F-007.25 i żywej publikacji placeholdera z F-007.27.

## Wymagania

- uprawnienie **Screen Recording** dla terminala, z którego uruchamiasz sondę
  (inaczej serwer poda placeholder i sonda nie mierzy niczego),
- wolne porty 33899 i 33900,
- binarka serwera: `cargo build --release`.
