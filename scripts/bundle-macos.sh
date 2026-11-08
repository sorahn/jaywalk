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
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/jaywalk" "$bundle/Contents/MacOS/jaywalk"
cp packaging/macos/Info.plist "$bundle/Contents/Info.plist"
# Compile the editable Icon Composer document, including the legacy fallback.
# Let Xcode supply the matching icon names rather than duplicating plist keys.
xcrun actool packaging/macos/jaywalk.icon \
    --compile "$bundle/Contents/Resources" \
    --platform macosx --target-device mac --minimum-deployment-target 15.0 \
    --app-icon jaywalk \
    --output-partial-info-plist "target/$profile/icon-info.plist" \
    --output-format human-readable-text --warnings --errors
/usr/libexec/PlistBuddy -c "Merge target/$profile/icon-info.plist" "$bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$bundle"
# Replacing files inside an existing bundle leaves its outer timestamp stale.
# Refresh this bundle's registration so the Dock notices newly compiled icons.
/usr/bin/touch "$bundle"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$bundle"
echo "$bundle"
