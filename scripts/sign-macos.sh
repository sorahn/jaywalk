#!/bin/sh
# Keep the signing identity stable across local rebuilds. Never weaken Keychain ACLs.
set -eu
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo 'usage: scripts/sign-macos.sh TARGET [IDENTIFIER]' >&2
    exit 2
fi
target=$1
identifier=${2:-dev.jaywalk.Jaywalk}
signing_identity=${JAYWALK_SIGNING_IDENTITY:-}
if [ -z "$signing_identity" ]; then
    # Use a certificate-backed identity only when the choice is unambiguous.
    # CI or machines without a certificate can still produce an ad hoc bundle.
    identities=$(/usr/bin/security find-identity -v -p codesigning | awk '/"Apple Development:/ {print $2}')
    count=$(printf '%s\n' "$identities" | awk 'NF {n++} END {print n+0}')
    case "$count" in
        0) signing_identity=- ;;
        1) signing_identity=$identities ;;
        *) echo 'Multiple Apple Development identities found; set JAYWALK_SIGNING_IDENTITY to the intended certificate fingerprint.' >&2; exit 2 ;;
    esac
fi
if [ "$signing_identity" = - ]; then
    echo 'Using ad hoc signing: Keychain approval may recur after rebuilding. Set JAYWALK_SIGNING_IDENTITY for a stable certificate-backed identity.' >&2
fi
/usr/bin/codesign --force --sign "$signing_identity" --identifier "$identifier" "$target"
/usr/bin/codesign --verify --strict "$target"
