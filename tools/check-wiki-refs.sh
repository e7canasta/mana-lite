#!/usr/bin/env bash
# Run: ./tools/check-wiki-refs.sh
#
# Checks repository-relative source-path citations in docs/wiki and reports
# symbol-like inline references that cannot be found in src/ or std/.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

status=0

# A citation is a repository-relative source/config path, optionally followed
# by a Markdown anchor. URLs and prose such as "src/ directory" are excluded.
while IFS=: read -r doc path; do
    path="${path%%\?*}"
    path="${path%%#*}"
    if [[ ! -e "$path" ]]; then
        printf 'missing wiki path: %s (%s)\n' "$path" "$doc" >&2
        status=1
    fi
done < <(
    rg --glob '!style.md' -o '(src|std|config)/[A-Za-z0-9_./-]+\.(rs|toml)([?#][^ )\]]*)?' docs/wiki \
        | awk -F: '{ print $1 ":" $2 }' \
        | sort -u
)

# Symbol-like references are intentionally advisory: the wiki also names
# external APIs and configuration values. Keep the grep bounded to project
# source, while making unresolved candidates visible for review.
while IFS=: read -r doc symbol; do
    symbol="${symbol#\`}"
    symbol="${symbol%\`}"
    base="${symbol%%::*}"
    base="${base%%(*}"
    if ! rg -q --glob '*.rs' -F "$base" src std; then
        printf 'unresolved wiki symbol: %s (%s)\n' "$symbol" "$doc" >&2
    fi
done < <(
    rg --glob '!style.md' -o '`([A-Z][a-z]+[A-Z][A-Za-z0-9_]*|[A-Za-z_]+::[A-Za-z0-9_]+|[a-z_]+\\(\\))`' docs/wiki \
        | sort -u
)

exit "$status"
