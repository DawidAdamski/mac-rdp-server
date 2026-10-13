#!/usr/bin/env bash
# F-110.17: the notes for a release come from CHANGELOG.md, never from a
# heredoc in the workflow. Prints the section for one version on stdout,
# followed by the permission warning that applies to every upgrade (F-110.13).
#
# Exits non-zero when the version has no section, which is what makes the
# release workflow refuse such a tag (AC-11). Usage:
#
#   packaging/release-notes.sh 0.1.0 [path/to/CHANGELOG.md]
set -euo pipefail

version="${1:?usage: release-notes.sh VERSION [CHANGELOG]}"
changelog="${2:-"$(dirname "$0")/../CHANGELOG.md"}"

if [ ! -f "$changelog" ]; then
    echo "release-notes.sh: no changelog at $changelog" >&2
    exit 1
fi

# The section runs from its own "## [x.y.z]" heading to the next "## " heading.
# `-v` keeps the version out of the regex, so a dot in it cannot match any
# character, and a version containing regex metacharacters cannot inject one.
body="$(awk -v want="$version" '
    /^## / {
        # Heading: "## [0.1.0] — 2026-10-13" or "## [0.1.0]".
        if (inside) exit
        line = $0
        sub(/^## +\[/, "", line)
        sub(/\].*$/, "", line)
        if (line == want) { inside = 1; next }
    }
    inside { print }
' "$changelog")"

# Drop the link-reference definitions that sit at the end of the file: they
# belong to the changelog, carry no text, and the last section would swallow
# them. Then trim the blank lines the section boundaries leave behind.
body="$(printf '%s\n' "$body" | grep -v '^\[[^]]*\]: ')"
body="$(printf '%s\n' "$body" | sed -e '/./,$!d' -e ':a' -e '/^\n*$/{$d;N;};/\n$/ba')"

if [ -z "$body" ]; then
    echo "release-notes.sh: CHANGELOG.md has no section for $version" >&2
    echo "release-notes.sh: add '## [$version] — YYYY-MM-DD' before tagging" >&2
    exit 1
fi

printf '%s\n' "$body"
cat <<'WARNING'

---

**After upgrading, grant Screen Recording and Accessibility again.** macOS ties
a permission to the exact binary, so this version is a different program to it.
The old entry stays in the list and can still look switched on — select it,
remove it with the minus button, add the current one, and restart the server.

Read [SECURITY.md](SECURITY.md) before changing `--bind-addr`: **this server
has no authentication.**
WARNING
