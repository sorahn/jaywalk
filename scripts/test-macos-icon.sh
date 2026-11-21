#!/bin/sh
# Fail before a full Cargo build if the runner cannot compile the shipping icon.
set -eu
cd "$(dirname "$0")/.."
sw_vers
xcodebuild -version
rustc --version
output=$(mktemp -d)
trap 'rm -rf "$output"' EXIT
xcrun actool packaging/macos/jaywalk.icon \
    --compile "$output" --platform macosx --target-device mac \
    --minimum-deployment-target 15.0 --app-icon jaywalk \
    --output-partial-info-plist "$output/icon-info.plist" \
    --output-format human-readable-text --warnings --errors
test -s "$output/Assets.car"
test -s "$output/jaywalk.icns"
/usr/libexec/PlistBuddy -c 'Print :CFBundleIconName' "$output/icon-info.plist"
