# specs/ref — przetworzone dokumenty źródłowe

Wersje tekstowe specyfikacji Microsoftu, wygodne do `grep` i do cytowania
w specach. **Oryginalne PDF-y nie są w repo** (`.gitignore`, ~15 MB każdy) —
leżą lokalnie w `specs/` i można je pobrać z adresów w `00-vision.md` §7.

| Plik | Źródło | Wersja |
|---|---|---|
| `MS-RDPBCGR-v20260309.txt` | `specs/[MS-RDPBCGR].pdf` | v20260309, Release: March 9, 2026, 444 str. |

## Format

- Znaczniki `==== page N / 444 ====` odpowiadają **stopkom PDF**. Tych numerów
  używamy w odwołaniach „str. N” w specach i ADR-ach.
- Spis treści w PDF podaje od okolic sekcji 3 numery o **1 większe** niż
  stopki. Przy rozbieżności obowiązuje stopka.
- Usunięte: powtarzany nagłówek i stopka strony oraz wielokrotne puste linie.
  Układ kolumn zachowany (`pdftotext -layout`), więc tabele pól PDU da się
  czytać, ale diagramy bitowe są tylko przybliżone — przy wątpliwościach
  sięgamy do PDF-a albo do wersji HTML.

## Licencja

Microsoft Open Specifications pozwala kopiować dokumentację na potrzeby
implementacji i dystrybuować jej fragmenty w dokumentacji implementacji
(nota „Intellectual Property Rights Notice”, str. 1 PDF-a). Patenty to osobna
kwestia — nota ich nie licencjonuje.

## Jak wygenerować ponownie

Wymaga `pdftotext` (poppler: `brew install poppler`).

```sh
pdftotext -layout "specs/[MS-RDPBCGR].pdf" /tmp/bcgr-raw.txt
python3 - /tmp/bcgr-raw.txt specs/ref/MS-RDPBCGR-v20260309.txt <<'PY'
import sys, re
pages = open(sys.argv[1], encoding="utf-8").read().split("\f")
drop = re.compile(r"^\s*(\[MS-RDPBCGR\] - v20260309|Remote Desktop Protocol: Basic "
                  r"Connectivity and Graphics Remoting|Copyright © 2026 Microsoft "
                  r"Corporation|Release: March 9, 2026|\d+ / 444)\s*$")
out = ["[MS-RDPBCGR] v20260309 (Release: March 9, 2026) — plain-text extraction.",
       "Source: specs/[MS-RDPBCGR].pdf (not committed). Regenerate: see specs/ref/README.md.",
       "Page markers '==== page N / 444 ====' match the PDF footers. Layout preserved (pdftotext -layout).", ""]
for i, p in enumerate(pages):
    if not p.strip():
        continue
    res, blank = [], 0
    for l in (l.rstrip() for l in p.split("\n")):
        if drop.match(l):
            continue
        blank = blank + 1 if not l.strip() else 0
        if blank > 1:
            continue
        res.append(l)
    while res and not res[0].strip(): res.pop(0)
    while res and not res[-1].strip(): res.pop()
    out += [f"==== page {i + 1} / 444 ===="] + res + [""]
open(sys.argv[2], "w", encoding="utf-8").write("\n".join(out) + "\n")
PY
```

Przy nowej wersji dokumentu: nowy plik z nową wersją w nazwie, stary zostaje,
dopóki specy się do niego odwołują.
