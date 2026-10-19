# Sonda — ile kosztuje jedna klatka w enkoderze

Powstała ze zgłoszenia właściciela 2026-10-19: „miałem parę razy taką
sytuację, że się zblokowało, ale jak odczekałem, to wróciło — tak jakby
czasami nie nadążał stream."

To jest **inny objaw** niż zamarzanie z F-007 §8c: tam nie wracało nigdy, tu
wraca samo. Sonda sprawdza jedno ogniwo potoku — enkoder — bez capture
i bez klienta, żeby liczba należała do enkodera i do nikogo innego.

```
cargo run --release          # w tym katalogu
```

Rozmiar pulpitu jest zaszyty na 5120×1440, bo tam zgłoszono objaw.
Własny `build.rs` dokłada `-Wl,-rpath,/usr/lib/swift`: argumenty linkera
z `build.rs` **zależności** nie docierają do binarki, która jej używa, więc
bez tego każde uruchomienie kończy się
`Library not loaded: @rpath/libswift_Concurrency.dylib`.

## Wynik 2026-10-19

Budżet: **33,3 ms** na klatkę przy 30 fps.

| Przypadek | średnio | najgorzej | sufit | aktualizacje |
|---|---|---|---|---|
| nic się nie zmieniło | 0,63 ms | 0,73 ms | ~1600 fps | 0 |
| zmiana 32×32 (kursor) | 0,61 ms | 0,63 ms | ~1640 fps | 1 |
| zmiana 1280×720 (okno) | 0,82 ms | 1,05 ms | ~1220 fps | 1 |
| cały ekran | 0,01 ms | 0,02 ms | — | 1 |
| enkoder pełnoklatkowy | ~0 ms | ~0 ms | — | 1 |

**Enkoder nie jest wąskim gardłem.** Najgorszy przypadek to 1,05 ms przeciw
budżetowi 33,3 ms — zapas rzędu trzydziestu razy.

Dwie rzeczy, które na pierwszy rzut oka wyglądają na błąd, a nie są:

1. **„Cały ekran" jest szybszy niż „nic się nie zmieniło"** (0,01 ms wobec
   0,63 ms). To nie pomyłka: `dirty_tiles` ma wczesne wyjście
   (`remaining`, `src/encoder/damage.rs:370`) — gdy wszystkie kafle w rzędzie
   są już brudne, przestaje skanować kolejne wiersze tego rzędu. Przy
   identycznych klatkach trzeba przeczytać oba bufory w całości, czyli 56 MB.
2. **Enkoder pełnoklatkowy ma koszt ~0.** Niczego nie porównuje i nie kopiuje —
   owija `Frame` w `FrameUpdate::Full`. Prawdziwy koszt tej ścieżki płaci
   silnik protokołu, nie enkoder.

### Błąd w pierwszej wersji tej sondy, zapisany dla potomności

Wypełnienie liczyłem jako `n % 251 | 1`, co zlepia kolejne pary na jedną
wartość (`10|1` i `11|1` to oba 11). Połowa „zmienionych" klatek była więc
identyczna, w tym ostatnia — i kolumna „aktualizacje" pokazywała wszędzie 0,
co wyglądało na błąd enkodera. Sonda, która myli się w modelu, mierzy nie to,
co trzeba. Teraz krok jest nieparzysty (`n * 37 + 1`) i nie powtarza się
w obrębie przebiegu.

## Co z tego wynika dla zgłoszonego objawu

Zmierzone ogniwa potoku:

| Ogniwo | Wydajność | Wąskie gardło? |
|---|---|---|
| Przechwytywanie (SCK → nasz proces) | **29,6 klatki/s** stabilnie przy 5120×1440 | nie |
| Enkoder (diff + regiony) | najgorzej 1,05 ms na klatkę | nie |
| Wysyłka (silnik protokołu → klient) | **niezmierzone** | kandydat |

Zostaje wysyłka, i jest po temu konkretny powód: pełna klatka to **28 MB**
nieskompresowanych. Enkoder przechodzi na pełną klatkę, kiedy zmieni się
większość ekranu (Cmd+Tab, przewijanie, wideo). Jedna taka klatka w sieci to
rzędy wielkości więcej czasu niż 33 ms, a `watch` nie kolejkuje — nadmiarowe
klatki się **gubią** (licznik `coalesced`). Objaw „zablokowało się i po chwili
wróciło" pasuje do tego dokładnie.

Tego nie dało się tu domknąć, bo **FreeRDP nie łączy się z tym serwerem**
(patrz `specs/waves.md` §Pomiar 2026-10-19b). Potrzebny jest pomiar z `mstsc`
i `MAC_RDP_LOG=debug`: `delivered` kontra `coalesced` oraz linie
`sending a full frame`.
