#!/bin/sh
# Test dependencies only; jj is not bundled with the end-user app.
set -eu
. "$(dirname "$0")/native-shell.sh"
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    echo 'This CI toolchain requires macOS ARM64.' >&2
    exit 2
fi
version=0.47.1
checksum=50129b527f7cd6aef82aebd4bb2b670bb56766adc509ef3279c4327aa3592255
# Partial Cargo-cache restores prune arbitrary files below target/. Keep
# executable test tools outside that tree so cache hits cannot delete them.
bin="${RUNNER_TEMP:?CI tools require RUNNER_TEMP}/jaywalk-ci-tools/bin"
mkdir -p "$bin"
workspace=$(mktemp -d)
trap 'rm -rf "$workspace"' EXIT
/usr/bin/curl --fail --location --silent --show-error --retry 3 \
    "https://github.com/jj-vcs/jj/releases/download/v$version/jj-v$version-aarch64-apple-darwin.tar.gz" \
    -o "$workspace/jj.tar.gz"
printf '%s  %s\n' "$checksum" "$workspace/jj.tar.gz" | /usr/bin/shasum -a 256 --check --status
tar -xzf "$workspace/jj.tar.gz" -C "$workspace"
install -m 755 "$workspace/jj" "$bin/jj"
reported=$("$bin/jj" --version)
# Upstream archives append the source hash; Homebrew omits it.
case "$reported" in
    "jj $version"|"jj $version"-*) ;;
    *) echo "Unexpected jj version: $reported" >&2; exit 2 ;;
esac
if [ -n "${GITHUB_PATH:-}" ]; then
    printf '%s\n' "$bin" >> "$GITHUB_PATH"
fi
"$bin/jj" --version
git --version
xcrun --find clang
