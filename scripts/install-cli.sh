#!/bin/sh
# Install a development CLI, pointing at this checkout's chosen native profile.
set -eu
cd "$(dirname "$0")/.."
prefix=${1:-"$HOME/.local/bin"}
profile=${2:-debug}
case "$profile" in
    debug|release) ;;
    *) echo 'usage: scripts/install-cli.sh [bin-directory] [debug|release]' >&2; exit 2 ;;
esac
if [ "$(uname -s)" = Darwin ]; then
    scripts/bundle-macos.sh "$profile"
    target="$(pwd)/target/$profile/Jaywalk.app/Contents/MacOS/jw"
else
    binaries=$(scripts/build-native.sh "$profile")
    target="$binaries/jw"
fi
if [ ! -x "$target" ]; then
    echo "Build did not produce an executable at $target" >&2
    exit 1
fi
mkdir -p "$prefix"
if [ -e "$prefix/jw" ] || [ -L "$prefix/jw" ]; then
    if [ ! -L "$prefix/jw" ]; then
        echo "Refusing to replace existing $prefix/jw" >&2
        exit 1
    fi
    existing=$(readlink "$prefix/jw")
    if [ "$existing" != "$target" ]; then
        # Upgrade only the exact symlink created by the previous installer.
        if [ "$existing" != "$(pwd)/target/$profile/jw" ]; then
            echo "Refusing to replace existing $prefix/jw" >&2
            exit 1
        fi
        staging=$(mktemp -d "$prefix/.jaywalk-install.XXXXXX")
        trap 'rm -rf "$staging"' EXIT
        ln -s "$target" "$staging/jw"
        mv -f "$staging/jw" "$prefix/jw"
    fi
else
    ln -s "$target" "$prefix/jw"
fi
echo "Installed $prefix/jw (keep this directory on PATH)."
