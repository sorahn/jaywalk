#!/bin/sh
set -eu
. "$(dirname "$0")/native-shell.sh"
cd "$(dirname "$0")/.."
: "${JAYWALK_SPARKLE_DIR:?Fetch the pinned Sparkle SDK first}"
workspace=$(mktemp -d)
trap 'rm -rf "$workspace"' EXIT
xcrun clang -fobjc-arc -fmodules -Wall -Wextra -Werror -Wno-unused-parameter -mmacosx-version-min=15.0 packaging/macos/updater_probe.m -F "$JAYWALK_SPARKLE_DIR" -framework Sparkle -framework AppKit -Wl,-rpath,"$JAYWALK_SPARKLE_DIR" -o "$workspace/probe"
"$workspace/probe"
