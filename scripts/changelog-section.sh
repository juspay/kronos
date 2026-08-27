#!/usr/bin/env bash
# Prints one release's section from CHANGELOG.md, for GitHub release notes.
# Usage: ./scripts/changelog-section.sh 0.2.0 [changelog-path]

set -euo pipefail

VERSION="${1:?usage: changelog-section.sh <version> [changelog]}"
FILE="${2:-$(git rev-parse --show-toplevel)/CHANGELOG.md}"

section=$(awk -v want="## v$VERSION" '
    index($0, want) == 1   { found = 1; next }
    found && /^## v/       { exit }
    found && $0 == "- - -" { next }
    found                  { print }
' "$FILE")

if [ -z "$(printf '%s' "$section" | tr -d '[:space:]')" ]; then
    echo "See [CHANGELOG.md](CHANGELOG.md) for changes in v$VERSION."
else
    printf '%s\n' "$section"
fi
