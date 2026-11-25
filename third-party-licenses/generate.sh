#!/usr/bin/env bash
set -euo pipefail

target="$1"
output="$2"
shift 2

cargo about generate --frozen --fail --target "$target" "$@" --output-file "$output" "$(dirname "$0")/template.hbs"

registry="${CARGO_HOME:-$HOME/.cargo}/registry/src"
if command -v cygpath > /dev/null; then
    registry="$(cygpath --unix "$registry")"
fi

cargo tree --frozen --package cassiopeia --edges normal --target "$target" "$@" --prefix none --format '{p}' \
    | cut -d ' ' -f 1,2 \
    | sort -u \
    | while read -r name version; do
        for notice in "$registry"/*/"$name-${version#v}"/NOTICE*; do
            [ -f "$notice" ] || continue
            printf '### %s %s\n\n~~~text\n' "$name" "${version#v}"
            cat "$notice"
            printf '\n~~~\n\n'
        done
    done >> "$output"
