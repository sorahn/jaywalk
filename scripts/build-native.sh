#!/bin/sh
# Packaging and installation consume only artifacts from this native build.
set -eu
cd "$(dirname "$0")/.."
profile=${1:-debug}
case "$profile" in
    debug|release) ;;
    *) echo 'usage: scripts/build-native.sh [debug|release]' >&2; exit 2 ;;
esac
host=$(rustc -vV | sed -n 's/^host: //p')
if [ -z "$host" ]; then
    echo 'Could not determine the native Rust target.' >&2
    exit 1
fi
output="$(pwd)/target"
if [ "$profile" = release ]; then
    cargo build --locked --bins --release --target "$host" --target-dir "$output" >&2
else
    cargo build --locked --bins --target "$host" --target-dir "$output" >&2
fi
printf '%s\n' "$output/$host/$profile"
