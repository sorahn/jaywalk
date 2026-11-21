#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile=${1:-debug}
binaries=$(scripts/build-native.sh "$profile")
destination=${JAYWALK_BUNDLE_OUTPUT:-"target/$profile/Jaywalk.app"}
# Query the standalone Cargo binary before constructing/signing the app bundle.
version=$(DYLD_FRAMEWORK_PATH="${JAYWALK_SPARKLE_DIR:-}" "$binaries/jaywalk" --package-version)
mkdir -p "target/$profile" "$(dirname "$destination")"
staging=$(mktemp -d "target/$profile/bundle.XXXXXX")
trap 'rm -rf "$staging"' EXIT
bundle="$staging/Jaywalk.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$binaries/jaywalk" "$bundle/Contents/MacOS/jaywalk"
cp "$binaries/jw" "$bundle/Contents/MacOS/jw"
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
# Updater assets are optional for ordinary development builds, pinned for releases.
if [ "${JAYWALK_MACOS_UPDATER:-0}" = 1 ]; then
    : "${JAYWALK_SPARKLE_DIR:?Fetch the pinned Sparkle SDK first}"
    mkdir -p "$bundle/Contents/Frameworks"
    ditto "$JAYWALK_SPARKLE_DIR/Sparkle.framework" "$bundle/Contents/Frameworks/Sparkle.framework"
    framework="$bundle/Contents/Frameworks/Sparkle.framework/Versions/B"
    for nested in Autoupdate Updater.app XPCServices/Downloader.xpc XPCServices/Installer.xpc; do
        if [ -e "$framework/$nested" ]; then scripts/sign-macos.sh "$framework/$nested" preserve; fi
    done
    scripts/sign-macos.sh "$bundle/Contents/Frameworks/Sparkle.framework" preserve
    # Absence of public configuration is intentional in unsigned rehearsals.
    JAYWALK_BUNDLE_PATH="$bundle" python3 - <<'PLIST'
import os,plistlib
from pathlib import Path
path=Path(os.environ['JAYWALK_BUNDLE_PATH'])/'Contents/Info.plist'
data=plistlib.loads(path.read_bytes())
data.update(SUEnableAutomaticChecks=False,SUAutomaticallyUpdate=False,SUEnableSystemProfiling=False,SUAllowsAutomaticUpdates=True,SUVerifyUpdateBeforeExtraction=True,SURequireSignedFeed=True)
if os.environ.get('JAYWALK_SIGNING_MODE')=='release':
    data['SUFeedURL']=os.environ['UPDATE_FEED_URL']
    data['SUPublicEDKey']=os.environ['SPARKLE_PUBLIC_KEY']
    # Jaywalk owns opt-in; stock Sparkle's checkbox is disabled to avoid changing
    # the public TOML policy independently. Runtime opts in only after consent.
path.write_bytes(plistlib.dumps(data))
PLIST
fi
mkdir -p "$bundle/Contents/Resources/licenses"
cp LICENSE "$bundle/Contents/Resources/licenses/Jaywalk.txt"
python3 scripts/bundle-licenses.py "$bundle/Contents/Resources/licenses/dependencies"
if [ "${JAYWALK_MACOS_UPDATER:-0}" = 1 ]; then
    cp "$JAYWALK_SPARKLE_DIR/LICENSE" "$bundle/Contents/Resources/licenses/Sparkle.txt"
fi
scripts/sign-macos.sh "$bundle/Contents/MacOS/jw" dev.jaywalk.jw
scripts/sign-macos.sh "$bundle"
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
if [ "${JAYWALK_REGISTER_BUNDLE:-1}" = 1 ]; then
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$destination"
fi
echo "$destination"
