#!/bin/sh
# Protected CI only. Secret values are never printed or stored in the source tree.
set -eu
cd "$(dirname "$0")/.."
for name in MACOS_CERTIFICATE_P12_BASE64 MACOS_CERTIFICATE_PASSWORD MACOS_SIGNING_IDENTITY APPLE_NOTARY_KEY_BASE64 APPLE_NOTARY_KEY_ID APPLE_NOTARY_ISSUER_ID SPARKLE_PRIVATE_KEY SPARKLE_PUBLIC_KEY UPDATE_FEED_URL; do
    python3 -c 'import os,sys; name=sys.argv[1]; sys.exit(0 if os.environ.get(name) else "Missing release credential/configuration: "+name)' "$name"
done
python3 scripts/release-macos.py check-tag "${GITHUB_REF_NAME:?Run on a release tag}"
case "$UPDATE_FEED_URL" in https://*) ;; *) echo 'Update feed must use HTTPS' >&2;exit 2;; esac
workspace=$(mktemp -d)
keychain="$workspace/release.keychain-db"
cleanup() {
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
    rm -rf "$workspace"
}
trap cleanup EXIT
export JAYWALK_SECRET_DIRECTORY="$workspace"
python3 - <<'SECRETS'
import base64,os
from pathlib import Path
root=Path(os.environ['JAYWALK_SECRET_DIRECTORY'])
for env,name in [('MACOS_CERTIFICATE_P12_BASE64','certificate.p12'),('APPLE_NOTARY_KEY_BASE64','notary.p8')]:
    path=root/name;path.write_bytes(base64.b64decode(os.environ[env],validate=True));path.chmod(0o600)
path=root/'sparkle.key';path.write_text(os.environ['SPARKLE_PRIVATE_KEY']);path.chmod(0o600)
SECRETS
password=$(openssl rand -hex 24)
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$workspace/certificate.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
export JAYWALK_SIGNING_IDENTITY="$MACOS_SIGNING_IDENTITY"
export JAYWALK_SIGNING_KEYCHAIN="$keychain"
export JAYWALK_SIGNING_MODE=release
export JAYWALK_MACOS_UPDATER=1
export JAYWALK_SPARKLE_DIR="$(pwd)/target/sparkle"
python3 scripts/fetch-sparkle.py "$JAYWALK_SPARKLE_DIR"
scripts/bundle-macos.sh release
bundle=target/release/Jaywalk.app
# Submit the signed app, staple it, THEN produce end-user payloads.
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$workspace/notarize.zip"
xcrun notarytool submit "$workspace/notarize.zip" --key "$workspace/notary.p8" --key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER_ID" --wait --output-format json > "$workspace/notary-app.json"
python3 -c 'import json,sys;sys.exit(0 if json.load(open(sys.argv[1]))["status"]=="Accepted" else "App notarization was not accepted")' "$workspace/notary-app.json"
xcrun stapler staple "$bundle"
python3 scripts/release-macos.py package "$bundle" target/distribution
stem="target/distribution/Jaywalk-$("$bundle/Contents/MacOS/jaywalk" --package-version)-macos-arm64"
codesign --force --sign "$JAYWALK_SIGNING_IDENTITY" --keychain "$keychain" --timestamp "$stem.dmg"
xcrun notarytool submit "$stem.dmg" --key "$workspace/notary.p8" --key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER_ID" --wait --output-format json > "$workspace/notary-dmg.json"
python3 -c 'import json,sys;sys.exit(0 if json.load(open(sys.argv[1]))["status"]=="Accepted" else "DMG notarization was not accepted")' "$workspace/notary-dmg.json"
xcrun stapler staple "$stem.dmg"
python3 scripts/release-macos.py manifest "$bundle" "$stem.zip" "$stem.dmg" --official > target/distribution/manifest.json
python3 scripts/release-macos.py cask target/distribution/manifest.json > target/distribution/jaywalk.rb
# Feed entries and update archives are signed with the separate Sparkle key.
mkdir "$workspace/feed"
cp "$stem.zip" "$workspace/feed/"
"$JAYWALK_SPARKLE_DIR/bin/generate_appcast" --ed-key-file "$workspace/sparkle.key" --download-url-prefix "https://github.com/sorahn/jaywalk/releases/download/$GITHUB_REF_NAME/" -o "$workspace/feed/appcast.xml" "$workspace/feed"
cp "$workspace/feed/appcast.xml" target/distribution/appcast.xml
(cd target/distribution && shasum -a 256 ./*.zip ./*.dmg > SHA256SUMS)
