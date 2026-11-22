#!/bin/sh
# Protected CI only. Secret values are never printed or stored in the source tree.
set -eu
. "$(dirname "$0")/native-shell.sh"
automation=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
cd "${JAYWALK_RELEASE_SOURCE:-$automation/..}"
for name in MACOS_CERTIFICATE_P12_BASE64 MACOS_CERTIFICATE_PASSWORD MACOS_SIGNING_IDENTITY APPLE_NOTARY_KEY_BASE64 APPLE_NOTARY_KEY_ID APPLE_NOTARY_ISSUER_ID SPARKLE_PRIVATE_KEY SPARKLE_PUBLIC_KEY UPDATE_FEED_URL JAYWALK_GITHUB_CLIENT_ID JAYWALK_GITHUB_CLIENT_SECRET JAYWALK_GITHUB_APP_SLUG; do
    "$automation/python.sh" -c 'import os,sys; name=sys.argv[1]; sys.exit(0 if os.environ.get(name) else "Missing release credential/configuration: "+name)' "$name"
done
release_tag=${JAYWALK_RELEASE_TAG:-${GITHUB_REF_NAME:?Run on a release tag}}
"$automation/python.sh" "$automation/release-macos.py" check-tag "$release_tag"
case "$UPDATE_FEED_URL" in https://*) ;; *) echo 'Update feed must use HTTPS' >&2;exit 2;; esac
workspace=$(mktemp -d)
keychain="$workspace/release.keychain-db"
cleanup() {
    # Restore the runner's exact original search list before deleting our keychain.
    if [ -f "$workspace/search-list.json" ]; then
        if "$automation/python.sh" - "$workspace/search-list.json" <<'RESTORE'
import json,subprocess,sys
subprocess.run(['security','list-keychains','-d','user','-s',*json.load(open(sys.argv[1]))],check=True)
RESTORE
        then :; else echo 'Could not restore the original runner keychain search list.' >&2; fi
    fi
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
    rm -rf "$workspace"
}
trap cleanup EXIT
export JAYWALK_SECRET_DIRECTORY="$workspace"
"$automation/python.sh" - <<'SECRETS'
import base64,os
from pathlib import Path
root=Path(os.environ['JAYWALK_SECRET_DIRECTORY'])
for env,name in [('MACOS_CERTIFICATE_P12_BASE64','certificate.p12'),('APPLE_NOTARY_KEY_BASE64','notary.p8')]:
    path=root/name;path.write_bytes(base64.b64decode(os.environ[env],validate=True));path.chmod(0o600)
path=root/'sparkle.key';path.write_text(os.environ['SPARKLE_PRIVATE_KEY']);path.chmod(0o600)
SECRETS
# Build subprocesses do not need the original credential values in their env.
unset MACOS_CERTIFICATE_P12_BASE64 APPLE_NOTARY_KEY_BASE64 SPARKLE_PRIVATE_KEY
password=$(openssl rand -hex 24)
"$automation/python.sh" - "$workspace/search-list.json" <<'SEARCH'
import json,shlex,subprocess,sys
paths=shlex.split(subprocess.check_output(['security','list-keychains','-d','user'],text=True))
with open(sys.argv[1],'w') as output: json.dump(paths,output)
SEARCH
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
"$automation/python.sh" - "$workspace/search-list.json" "$keychain" <<'SEARCH'
import json,subprocess,sys
subprocess.run(['security','list-keychains','-d','user','-s',sys.argv[2],*json.load(open(sys.argv[1]))],check=True)
SEARCH
security import "$workspace/certificate.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
unset MACOS_CERTIFICATE_PASSWORD
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
export JAYWALK_SIGNING_IDENTITY="$MACOS_SIGNING_IDENTITY"
export JAYWALK_SIGNING_KEYCHAIN="$keychain"
export JAYWALK_SIGNING_MODE=release
# Fail fast on missing/invalid certificates or inaccessible private keys.
security find-identity -v -p codesigning "$keychain" > "$workspace/identities.txt"
if ! awk -v identity="$JAYWALK_SIGNING_IDENTITY" 'toupper($2)==toupper(identity) && /"Developer ID Application:/ {found=1} END {exit !found}' "$workspace/identities.txt"; then
    security find-identity -p codesigning "$keychain" >&2
    echo 'The intended Developer ID identity is not valid in the temporary signing keychain.' >&2
    exit 1
fi
printf '%s\n' 'int main(void) {return 0;}' > "$workspace/signing-probe.c"
xcrun clang -mmacosx-version-min=15.0 "$workspace/signing-probe.c" -o "$workspace/signing-probe"
scripts/sign-macos.sh "$workspace/signing-probe" dev.jaywalk.signing-probe
export JAYWALK_MACOS_UPDATER=1
export JAYWALK_SPARKLE_DIR="$(pwd)/target/sparkle"
"$automation/python.sh" scripts/fetch-sparkle.py "$JAYWALK_SPARKLE_DIR"
scripts/bundle-macos.sh release
bundle=target/release/Jaywalk.app
# Fresh installs must have a real publisher client, not a developer Keychain import.
"$bundle/Contents/MacOS/jaywalk" --hosting-capabilities | "$automation/python.sh" -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("github_browser") is True else "Release binary has no configured GitHub publisher client")'
# Submit the signed app, staple it, THEN produce end-user payloads.
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$workspace/notarize.zip"
xcrun notarytool submit "$workspace/notarize.zip" --key "$workspace/notary.p8" --key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER_ID" --wait --output-format json > "$workspace/notary-app.json"
"$automation/python.sh" -c 'import json,sys;sys.exit(0 if json.load(open(sys.argv[1]))["status"]=="Accepted" else "App notarization was not accepted")' "$workspace/notary-app.json"
xcrun stapler staple "$bundle"
"$automation/python.sh" "$automation/release-macos.py" package "$bundle" target/distribution
stem="target/distribution/Jaywalk-$("$bundle/Contents/MacOS/jaywalk" --package-version)-macos-arm64"
codesign --force --sign "$JAYWALK_SIGNING_IDENTITY" --keychain "$keychain" --timestamp "$stem.dmg"
xcrun notarytool submit "$stem.dmg" --key "$workspace/notary.p8" --key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER_ID" --wait --output-format json > "$workspace/notary-dmg.json"
"$automation/python.sh" -c 'import json,sys;sys.exit(0 if json.load(open(sys.argv[1]))["status"]=="Accepted" else "DMG notarization was not accepted")' "$workspace/notary-dmg.json"
xcrun stapler staple "$stem.dmg"
"$automation/python.sh" "$automation/release-macos.py" manifest "$bundle" "$stem.zip" "$stem.dmg" --official > target/distribution/manifest.json
"$automation/python.sh" "$automation/release-macos.py" cask target/distribution/manifest.json > target/distribution/jaywalk.rb
# Feed entries and update archives are signed with the separate Sparkle key.
mkdir "$workspace/feed"
cp "$stem.zip" "$workspace/feed/"
"$JAYWALK_SPARKLE_DIR/bin/generate_appcast" --ed-key-file "$workspace/sparkle.key" --download-url-prefix "https://github.com/sorahn/jaywalk/releases/download/$release_tag/" -o "$workspace/feed/appcast.xml" "$workspace/feed"
cp "$workspace/feed/appcast.xml" target/distribution/appcast.xml
(cd target/distribution && shasum -a 256 ./*.zip ./*.dmg > SHA256SUMS)
