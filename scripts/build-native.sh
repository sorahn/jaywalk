#!/bin/sh
# Packaging and installation consume only artifacts from this native build.
set -eu
. "$(dirname "$0")/native-shell.sh"
cd "$(dirname "$0")/.."
profile=${1:-debug}
case "$profile" in
    debug|release) ;;
    *) echo 'usage: scripts/build-native.sh [debug|release] [target-directory]' >&2; exit 2 ;;
esac
host=$(rustc -vV | sed -n 's/^host: //p')
if [ -z "$host" ]; then
    echo 'Could not determine the native Rust target.' >&2
    exit 1
fi
# An Intel Rust installation must not silently produce a Rosetta app on ARM Macs.
if [ "$(uname -s)" = Darwin ] && [ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ] && [ "$host" != aarch64-apple-darwin ]; then
    echo 'Apple Silicon builds require a native aarch64-apple-darwin Rust toolchain.' >&2
    exit 1
fi
output=${2:-"$(pwd)/target"}
case "$output" in
    /*) ;;
    *) output="$(pwd)/$output" ;;
esac
set --
if [ "${JAYWALK_MACOS_UPDATER:-0}" = 1 ]; then set -- --features macos-updater; fi
if [ "$profile" = release ]; then
    cargo build --locked --bins "$@" --release --target "$host" --target-dir "$output" >&2
else
    cargo build --locked --bins "$@" --target "$host" --target-dir "$output" >&2
fi
printf '%s\n' "$output/$host/$profile"
