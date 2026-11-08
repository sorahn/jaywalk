#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile=${1:-debug}
case "$profile" in
    debug) cargo build --locked ;;
    release) cargo build --locked --release ;;
    *) echo 'usage: scripts/bundle-macos.sh [debug|release]' >&2; exit 2 ;;
esac
destination="target/$profile/Jaywalk.app"
# Query the standalone Cargo binary before constructing/signing the app bundle.
version=$("target/$profile/jaywalk" --package-version)
staging=$(mktemp -d "target/$profile/bundle.XXXXXX")
trap 'rm -rf "$staging"' EXIT
bundle="$staging/Jaywalk.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/jaywalk" "$bundle/Contents/MacOS/jaywalk"
cp packaging/macos/Info.plist "$bundle/Contents/Info.plist"
# The standalone executable gets this value from Cargo.toml.
/usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $version" "$bundle/Contents/Info.plist"
# Compile the editable Icon Composer document, including the legacy fallback.
# Let Xcode supply the matching icon names rather than duplicating plist keys.
xcrun actool packaging/macos/jaywalk.icon \
    --compile "$bundle/Contents/Resources" \
    --platform macosx --target-device mac --minimum-deployment-target 15.0 \
    --app-icon jaywalk \
    --output-partial-info-plist "$staging/icon-info.plist" \
    --output-format human-readable-text --warnings --errors
/usr/libexec/PlistBuddy -c "Merge $staging/icon-info.plist" "$bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$bundle"
# Replace the bundle by renaming directories. Never overwrite a running Mach-O
# executable or leave the installed bundle half-signed if compilation fails.
if [ -e "$destination" ]; then
    mv "$destination" "$staging/previous.app"
fi
if ! mv "$bundle" "$destination"; then
    if [ -e "$staging/previous.app" ]; then
        mv "$staging/previous.app" "$destination"
    fi
    exit 1
fi
/usr/bin/touch "$destination"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$destination"
echo "$destination"
