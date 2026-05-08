#!/usr/bin/env bash
# Print the CHANGELOG.md section for the given version on stdout.
# Exits non-zero if no entry is found (or if the section is empty).
#
# Usage: extract-changelog.sh <version> [path]
#   <version>  semver string without leading "v" (e.g. 0.1.0)
#   [path]     defaults to CHANGELOG.md
set -euo pipefail

VERSION="${1:?usage: extract-changelog.sh <version> [path]}"
FILE="${2:-CHANGELOG.md}"

if [ ! -f "$FILE" ]; then
  echo "extract-changelog: $FILE not found" >&2
  exit 2
fi

SECTION="$(awk -v ver="$VERSION" '
  BEGIN { target = "## [" ver "]" }
  /^## \[/ {
    if (in_section) exit
    if (substr($0, 1, length(target)) == target) {
      in_section = 1
      next
    }
  }
  in_section { print }
' "$FILE")"

if [ -z "$(printf '%s' "$SECTION" | tr -d '[:space:]')" ]; then
  echo "extract-changelog: no entry for version '$VERSION' in $FILE" >&2
  exit 1
fi

printf '%s\n' "$SECTION"
