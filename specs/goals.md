# Cele (`/goal`) — łańcuch prac Fali 3: poprawki zgodności z MS-RDPBCGR

**Status:** dokument żywy
**Data:** 2026-09-17
**Dotyczy:** `waves.md` Fala 3, `02-ms-rdpbcgr.md` §6 pkt 1–3 (P-01, P-04/P-05, P-06)

## Jak to działa

`/goal <warunek>` (Claude Code ≥ 2.1.198, dokumentacja:
<https://code.claude.com/docs/en/goal.md>) pracuje turami, aż warunek zostanie
spełniony. **Po każdej turze mały model ocenia warunek wyłącznie na podstawie
transkryptu** — dlatego każdy cel kończy się raportem weryfikacyjnym
z wklejonym wynikiem poleceń. Bez raportu cel się nie zamknie.

- `/goal` — stan bieżącego celu, `/goal clear` — przerwanie.
- Cele najlepiej uruchamiać w trybie auto (bez pytań o każde polecenie).
- **Jeden cel naraz.** Kolejny cel wklejasz dopiero po przejściu bramki.

## Bramki właściciela — celów nie da się ich obejść

Zasada zero z `CLAUDE.md`: kod powstaje tylko dla wymagań **Approved**,
a approve daje właściciel. Łańcuch wygląda więc tak:

```
G1  specy (autonomicznie)
 └─ BRAMKA A  właściciel: przegląd, zmiana statusu na Approved
             + pomiary D-1 i D-2 (02-ms-rdpbcgr.md §5)
G2  input: P-04, P-05 (autonomicznie)
 └─ BRAMKA B  właściciel: test ręczny prawdziwym klientem
G3  Suppress Output / Refresh Rect: P-06 (autonomicznie)
 └─ BRAMKA C  właściciel: test ręczny, F-004 AC-5 (Cmd+Tab)
 → dalej F-005 (waves.md)
```

Każdy cel sprawdza swój warunek wejścia i, jeśli nie jest spełniony, kończy się
raportem `BLOCKED` zamiast zgadywać.

---

## G1 — specy dla poprawek (start: teraz)

Zakres: wyłącznie dokumenty. Wklej:

```text
/goal Przygotuj specy (status Draft) dla poprawek Fali 3 z specs/02-ms-rdpbcgr.md §6 pkt 1–3, zgodnie z CLAUDE.md (format speca, język polski, odwołania do stron z specs/ref/MS-RDPBCGR-v20260309.txt).

Zakres:
(a) specs/features/F-003-input.md — nowe wymagania oznaczone „Draft (2026-09-17)”, numeracja od F-003.22: Synchronize zwalnia wszystkie trzymane modyfikatory i przyciski prawdziwymi zdarzeniami KeyUp/MouseUp (P-04, MS-RDPBCGR 2.2.8.1.1.3.1.1.5); stan klawiszy i przycisków nie przecieka między połączeniami. Do tego kryteria akceptacji i pozycje checklisty ręcznej.
(b) specs/features/F-003a-keyboard-mapping.md — wymagania od F-003a.14: składanie par surogatów UTF-16 w jeden znak; samotny surogat odrzucony, nigdy keycode 0; zdarzenia Unicode bez flag modyfikatorów (P-05). Do tego testy jednostkowe wymienione w kryteriach akceptacji.
(c) nowy specs/features/F-008-suppress-output-refresh-rect.md ze wszystkimi 6 sekcjami (P-06, G-2, G-3). Musi zawierać: wstrzymanie wysyłania po Suppress Output, pełne odświeżenie po wznowieniu i po Refresh Rect (z rozstrzygnięciem, jak zresetować diff naszego enkodera i IronRDP), wymaganie diagnostyczne dla pomiaru D-2 oraz checklistę ręczną z F-004 AC-5 (Cmd+Tab).
(d) nowy specs/adr/ADR-0013-rzeczywista-sciezka-obrazu.md, status Proposed (P-01): stan faktyczny z ironrdp-server-0.13.0/src/encoder/mod.rs:132-159 i :491-499, procedura pomiaru D-1 dla właściciela, pusta tabela wyników, informacja, które zdania ADR-0006 i F-004 §7/§8 zmieni.
(e) specs/waves.md — tabela Fali 3 wskazuje nowe ID.

Ograniczenia: żadnych zmian w src/, tests/, examples/, Cargo.toml ani Cargo.lock. Żadnemu dokumentowi nie ustawiasz statusu Approved. Istniejących wymagań nie przepisujesz — tylko dopisujesz. Nie ruszasz niezacommitowanych zmian w specs/features/F-004-damage-tracking.md.

Zakończenie: jeden commit „specs: wave 3 MS-RDPBCGR fixes (Draft) — F-003.22+, F-003a.14+, F-008, ADR-0013”, bez F-004-damage-tracking.md. Potem raport w odpowiedzi zawierający: listę nowych ID wymagań, wynik `git show --stat HEAD`, wynik `git status --short` oraz wynik `git diff HEAD~1 --stat -- src tests examples Cargo.toml Cargo.lock` (musi być pusty).

Cel jest spełniony, gdy ten raport jest w transkrypcie i wszystkie punkty (a)–(e) są w commicie — albo po 30 turach.
```

**Po G1 → BRAMKA A.** Przeczytaj specy, popraw, ustaw `Approved` (z datą)
tam, gdzie się zgadzasz. Wykonaj D-1 (Wireshark: ścieżka obrazu) i D-2
(log przy Cmd+Tab); wyniki wpisz do ADR-0013 i `02-ms-rdpbcgr.md` §5 albo
podyktuj je Claude'owi.

---

## G2 — input: Synchronize i surogaty (start: po BRAMCE A)

```text
/goal Zaimplementuj wymagania F-003.22+ i F-003a.14+ (dodane 2026-09-17) zgodnie z CLAUDE.md.

Warunek wejścia: każde z tych wymagań ma w swoim specu status Approved. Jeśli któreś nie ma — nic nie implementuj. Wypisz raport „BLOCKED: <ID> nie jest Approved” i zakończ; to także kończy cel.

Zakres: tylko src/input/**, w razie potrzeby src/session/** oraz testy. Bez zmian w Cargo.toml i Cargo.lock. Tylko src/session/ może importować ironrdp_*. Logika czysta (stan modyfikatorów i przycisków, składanie surogatów) jest w funkcjach testowalnych bez uprawnień macOS. Każde wymaganie ma co najmniej jeden test jednostkowy, którego nazwa albo komentarz zawiera ID wymagania. Istniejących testów nie zmieniasz.

Commity: każdy zaczyna się od ID wymagania (np. „F-003.22: …”), po angielsku. Na końcu status wymagań w specach zmieniasz na „Implemented (data)” commitem „specs: …”.

Zakończenie — raport w odpowiedzi z wynikami, każdy z kodem wyjścia 0:
`cargo fmt --all -- --check`
`cargo clippy --all-targets -- -D warnings`
`cargo test --all-targets`
Do tego: lista ID → nazwa testu, `git log --oneline` nowych commitów oraz checklista testu ręcznego dla właściciela (Cmd+Tab po stronie klienta, rozłączenie z wciśniętym Cmd, emoji w edytorze).

Cel jest spełniony, gdy ten raport jest w transkrypcie i wszystkie trzy polecenia przeszły — albo gdy jest raport BLOCKED — albo po 40 turach.
```

**Po G2 → BRAMKA B:** test ręczny według checklisty z raportu; wynik wpisz
do F-003 / F-003a §Status.

---

## G3 — Suppress Output i Refresh Rect (start: po BRAMCE B)

```text
/goal Zaimplementuj specs/features/F-008-suppress-output-refresh-rect.md zgodnie z CLAUDE.md i rozstrzygnięciem z ADR-0013.

Warunek wejścia: F-008 ma status Approved, a wynik pomiaru D-2 jest wpisany w F-008 albo w specs/02-ms-rdpbcgr.md §5. Jeśli nie — wypisz „BLOCKED: <powód>” i zakończ; to także kończy cel.

Zakres: IronRDP importuje tylko src/session/. Enkoder (src/encoder/) zostaje czysty i testowalny bez GUI. Reset bazy damage i wstrzymanie wysyłania mają testy jednostkowe z ID wymagań. Tam, gdzie da się to zrobić bez GUI, jest test integracyjny w tests/ (np. klient ironrdp wysyła Suppress Output i sprawdzamy, że strumień aktualizacji staje, a po wznowieniu przychodzi pełna klatka). Bez zmian w Cargo.toml i Cargo.lock, chyba że spec wprost na to pozwala. Jeśli okaże się, że wymaganie jest niewykonalne na IronRDP 0.13.0, zatrzymaj się i opisz propozycję zmiany speca albo ADR — zgodnie z CLAUDE.md, bez obchodzenia problemu w kodzie.

Commity z ID „F-008.N: …”. Na końcu commit ze statusem F-008 „Implemented (data)”.

Zakończenie — raport w odpowiedzi z wynikami `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` i `cargo test --all-targets` (wszystkie z kodem 0), listą ID → test, `git log --oneline` nowych commitów oraz checklistą ręczną z F-008 (w tym F-004 AC-5: Cmd+Tab, minimalizacja i przywrócenie okna klienta).

Cel jest spełniony, gdy ten raport jest w transkrypcie — albo gdy jest raport BLOCKED lub propozycja zmiany speca — albo po 40 turach.
```

**Po G3 → BRAMKA C:** test ręczny. Jeśli F-004 AC-5 przechodzi, zamykasz
F-004 §8 pkt 6 i Fala 3 idzie dalej do F-005.

## Dziennik

| Cel | Start | Koniec | Wynik |
|---|---|---|---|
| G1 | 2026-09-17 | 2026-09-17 | Commit `0e39064`; specy Approved przez właściciela 2026-09-17 (ADR-0011, ADR-0013 nadal Proposed) |
| G2 | 2026-09-18 | 2026-09-18 | Commity `5e57e3e`…`f4978d8`; BRAMKA B 2026-09-18: F-003.22–26 Verified; F-003a.14–18 Implemented, AC-10 odłożone (mstsc nie wysyła Unicode, F-003a §8); właściciel: kontynuować G3 |
| G3 | 2026-09-18 | 2026-09-18 | F-008 Implemented; czeka BRAMKA C (AC-3…AC-9, w tym F-004 AC-5) |
