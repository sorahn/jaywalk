#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile=${1:-debug}
case "$profile" in
    debug) cargo build --locked ;;
    release) cargo build --locked --release ;;
    *) echo 'usage: scripts/bundle-macos.sh [debug|release]' >&2; exit 2 ;;
esac
bundle="target/$profile/Jaywalk.app"
mkdir -p "$bundle/Contents/MacOS"
cp "target/$profile/jaywalk" "$bundle/Contents/MacOS/jaywalk"
cp packaging/macos/Info.plist "$bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$bundle"
echo "$bundle"
