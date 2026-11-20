#!/bin/sh
# Disposable keys only: never touches the login Keychain or production credentials.
set -eu
cd "$(dirname "$0")/.."
: "${JAYWALK_SPARKLE_DIR:?Fetch the pinned Sparkle SDK first}"
workspace=$(mktemp -d)
trap 'rm -rf "$workspace"' EXIT
export JAYWALK_FEED_FIXTURE="$workspace"
cat > "$workspace/key.swift" <<'SWIFT'
import CryptoKit
import Foundation
let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["JAYWALK_FEED_FIXTURE"]!)
let key = Curve25519.Signing.PrivateKey()
let privateURL = directory.appendingPathComponent("private.key")
try key.rawRepresentation.base64EncodedString().write(to: privateURL, atomically: true, encoding: .utf8)
try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: privateURL.path)
try key.publicKey.rawRepresentation.base64EncodedString().write(to: directory.appendingPathComponent("public.key"), atomically: true, encoding: .utf8)
SWIFT
xcrun swift "$workspace/key.swift"
mkdir -p "$workspace/FeedProbe.app/Contents/MacOS" "$workspace/feed"
printf '%s\n' 'int main(void) {return 0;}' > "$workspace/probe.c"
xcrun clang "$workspace/probe.c" -o "$workspace/FeedProbe.app/Contents/MacOS/probe"
python3 - <<'PY'
import os,plistlib
from pathlib import Path
root=Path(os.environ['JAYWALK_FEED_FIXTURE'])
data=dict(CFBundleIdentifier='dev.example.FeedProbe',CFBundleExecutable='probe',CFBundleName='FeedProbe',CFBundlePackageType='APPL',CFBundleVersion='1.0.1',CFBundleShortVersionString='1.0.1',LSMinimumSystemVersion='15.0',SUPublicEDKey=(root/'public.key').read_text(),SUFeedURL='https://example.invalid/appcast.xml',SUVerifyUpdateBeforeExtraction=True,SURequireSignedFeed=True)
(root/'FeedProbe.app/Contents/Info.plist').write_bytes(plistlib.dumps(data))
PY
codesign --force --sign - "$workspace/FeedProbe.app"
ditto -c -k --sequesterRsrc --keepParent "$workspace/FeedProbe.app" "$workspace/feed/FeedProbe.zip"
"$JAYWALK_SPARKLE_DIR/bin/generate_appcast" --ed-key-file "$workspace/private.key" --download-url-prefix https://example.invalid/downloads/ "$workspace/feed" > "$workspace/generation.log" 2>&1
"$JAYWALK_SPARKLE_DIR/bin/sign_update" --verify --ed-key-file "$workspace/private.key" "$workspace/feed/appcast.xml"
python3 - <<'PY'
import os
from pathlib import Path
path=Path(os.environ['JAYWALK_FEED_FIXTURE'])/'feed/appcast.xml'
text=path.read_text()
assert '1.0.1' in text
path.write_text(text.replace('1.0.1','1.0.2'))
PY
if "$JAYWALK_SPARKLE_DIR/bin/sign_update" --verify --ed-key-file "$workspace/private.key" "$workspace/feed/appcast.xml" > "$workspace/tamper.log" 2>&1; then
    echo 'Tampered feed unexpectedly verified' >&2
    exit 1
fi
printf '%s\n' 'Disposable Sparkle feed generation/signature/tamper checks passed; no Keychain access.'
