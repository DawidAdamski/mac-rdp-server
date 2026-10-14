# Sonda F-111 — `NSStatusItem` z binarki bez bundla `.app`

Ten katalog **nie jest częścią programu**. `cargo` go nie widzi, CI go nie
buduje. To jest dowód do pomiaru opisanego w `specs/waves.md`
(§„Pomiar 2026-10-14"), zachowany dlatego, że ADR do F-111 ma zawierać
zbadany stan faktyczny, a nie samą konkluzję (`CLAUDE.md` §ADR).

## Co sprawdza

1. Czy zwykła binarka CLI — bez bundla, bez `Info.plist` — może mieć własny
   element w pasku menu.
2. Czy jego menu rozwija się po kliknięciu **prawdziwym** zdarzeniem myszy.
3. Czy to samo działa, gdy proces startuje z `launchd`, a nie z terminala.
4. Jakie uprawnienia TCC ma taki proces pod `launchd`.

## Jak uruchomić

```sh
swiftc -O probe.swift -o /tmp/f111-probe/probe
/tmp/f111-probe/probe /tmp/f111-probe          # z terminala: ikonka, klik, dwa zrzuty
/tmp/f111-probe/probe /tmp/f111-probe hold     # tylko pokaż ikonkę i czekaj 12 s
```

Wariant z `launchd`:

```sh
launchctl submit -l probe -o /tmp/f111-probe/out.log -e /tmp/f111-probe/err.log \
  -- /tmp/f111-probe/probe /tmp/f111-probe hold
launchctl remove probe
```

Program sam się kończy, ale wariant `hold` warto po sobie sprzątnąć
(`launchctl remove`), żeby ikonka nie została w pasku.

## Czego tu nie ma

Zrzutów ekranu. Były dowodem i zostały obejrzane, ale pokazują cały pasek
menu i okno przeglądarki właściciela, więc nie trafiają do publicznego repo.
Wynik, który z nich wynika, jest opisany w `specs/waves.md`.
