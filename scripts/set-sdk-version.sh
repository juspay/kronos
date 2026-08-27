#!/usr/bin/env bash
# Sets the [package] version in crates/client/Cargo.toml (the smithy-generated
# SDK), which is outside the workspace and so missed by cargo set-version.
# Usage: ./scripts/set-sdk-version.sh 0.2.0

set -euo pipefail

VERSION="${1:?usage: set-sdk-version.sh <version>}"
MANIFEST="$(git rev-parse --show-toplevel)/crates/client/Cargo.toml"

[ -f "$MANIFEST" ] || { echo "error: $MANIFEST not found" >&2; exit 1; }

awk -v ver="$VERSION" '
    /^\[/            { section = $0 }
    section == "[package]" && /^version[[:space:]]*=/ {
        print "version = \"" ver "\""; stamped = 1; next
    }
    { print }
    END { if (!stamped) { print "error: no [package] version found" > "/dev/stderr"; exit 1 } }
' "$MANIFEST" > "$MANIFEST.tmp"

mv "$MANIFEST.tmp" "$MANIFEST"
echo "kronos_sdk version set to $VERSION"
