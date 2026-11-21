#!/bin/sh
# Disposable keys only: never touches the login Keychain or production credentials.
set -eu
. "$(dirname "$0")/native-shell.sh"
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
public_key=$(cat "$workspace/public.key")
xcrun swift scripts/verify-sparkle-key.swift "$public_key" < "$workspace/private.key"
if xcrun swift scripts/verify-sparkle-key.swift AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= < "$workspace/private.key" > "$workspace/key-mismatch.log" 2>&1; then
    echo 'Mismatched Sparkle key pair unexpectedly verified' >&2
    exit 1
fi
mkdir -p "$workspace/FeedProbe.app/Contents/MacOS" "$workspace/feed"
printf '%s\n' 'int main(void) {return 0;}' > "$workspace/probe.c"
xcrun clang "$workspace/probe.c" -o "$workspace/FeedProbe.app/Contents/MacOS/probe"
scripts/python.sh - <<'PY'
import os,plistlib
from pathlib import Path
root=Path(os.environ['JAYWALK_FEED_FIXTURE'])
data=dict(CFBundleIdentifier='dev.example.FeedProbe',CFBundleExecutable='probe',CFBundleName='FeedProbe',CFBundlePackageType='APPL',CFBundleVersion='2026.11.1',CFBundleShortVersionString='2026.11.1',LSMinimumSystemVersion='15.0',SUPublicEDKey=(root/'public.key').read_text(),SUFeedURL='https://example.invalid/appcast.xml',SUVerifyUpdateBeforeExtraction=True,SURequireSignedFeed=True)
(root/'FeedProbe.app/Contents/Info.plist').write_bytes(plistlib.dumps(data))
PY
codesign --force --sign - "$workspace/FeedProbe.app"
ditto -c -k --sequesterRsrc --keepParent "$workspace/FeedProbe.app" "$workspace/feed/FeedProbe.zip"
"$JAYWALK_SPARKLE_DIR/bin/generate_appcast" --ed-key-file "$workspace/private.key" --download-url-prefix https://github.com/example/fixture/releases/download/v2026.11.1/ "$workspace/feed" > "$workspace/generation.log" 2>&1
"$JAYWALK_SPARKLE_DIR/bin/sign_update" --verify --ed-key-file "$workspace/private.key" "$workspace/feed/appcast.xml"
scripts/python.sh - <<'VERIFY'
import importlib.util,os,subprocess
from pathlib import Path
root=Path(os.environ['JAYWALK_FEED_FIXTURE'])
spec=importlib.util.spec_from_file_location('promotion',Path('scripts/promote-release.py'))
promotion=importlib.util.module_from_spec(spec);spec.loader.exec_module(promotion)
report={'artifacts':[{'filename':'FeedProbe.zip','bytes':(root/'feed/FeedProbe.zip').stat().st_size}]}
archive,signature=promotion.enclosure(root/'feed/appcast.xml','v2026.11.1',report,'example/fixture')
subprocess.run([os.environ['JAYWALK_SPARKLE_DIR']+'/bin/sign_update','--verify','--ed-key-file',str(root/'private.key'),str(root/'feed'/archive),signature],check=True)
VERIFY
scripts/python.sh - <<'PY'
import os
from pathlib import Path
path=Path(os.environ['JAYWALK_FEED_FIXTURE'])/'feed/appcast.xml'
text=path.read_text()
assert '2026.11.1' in text
path.write_text(text.replace('2026.11.1','2026.11.2'))
PY
if "$JAYWALK_SPARKLE_DIR/bin/sign_update" --verify --ed-key-file "$workspace/private.key" "$workspace/feed/appcast.xml" > "$workspace/tamper.log" 2>&1; then
    echo 'Tampered feed unexpectedly verified' >&2
    exit 1
fi
printf '%s\n' 'Disposable Sparkle feed generation/signature/tamper checks passed; no Keychain access.'
