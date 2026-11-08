#!/bin/sh
# Install a development CLI, pointing at this checkout's chosen build profile.
set -eu
cd "$(dirname "$0")/.."
prefix=${1:-"$HOME/.local/bin"}
profile=${2:-debug}
case "$profile" in
    debug) cargo build --locked --bins ;;
    release) cargo build --locked --bins --release ;;
    *) echo 'usage: scripts/install-cli.sh [bin-directory] [debug|release]' >&2; exit 2 ;;
esac
if [ "$(uname -s)" = Darwin ]; then
    scripts/bundle-macos.sh "$profile"
fi
target="$(pwd)/target/$profile/jw"
mkdir -p "$prefix"
if [ -e "$prefix/jw" ] || [ -L "$prefix/jw" ]; then
    if [ ! -L "$prefix/jw" ] || [ "$(readlink "$prefix/jw")" != "$target" ]; then
        echo "Refusing to replace existing $prefix/jw" >&2
        exit 1
    fi
else
    ln -s "$target" "$prefix/jw"
fi
echo "Installed $prefix/jw (keep this directory on PATH)."
