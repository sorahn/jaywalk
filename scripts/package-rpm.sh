#!/bin/sh
# Build a local native RPM with the current host's Rust toolchain and libraries.
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Linux ]; then
    echo 'RPM packaging requires a Linux build host.' >&2
    exit 1
fi
for tool in cargo rustc rpm rpmbuild desktop-file-validate; do
    command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
desktop-file-validate packaging/linux/io.github.sorahn.jaywalk.desktop
topdir="$(pwd)/target/rpm"
builddir="$topdir/cargo"
release=${1:-"0.$(date -u +%Y%m%d%H%M%S).local"}
case "$release" in
    ''|*[!a-zA-Z0-9._+]*) echo 'RPM release must contain only letters, digits, dots, underscores or +.' >&2; exit 2 ;;
esac
# Share native target selection while retaining RPM's isolated Cargo directory.
binaries=$(scripts/build-native.sh release "$builddir")
host=$(basename "$(dirname "$binaries")")
version=$("$binaries/jaywalk" --package-version)
mkdir -p "$topdir/SOURCES"
staging=$(mktemp -d "$topdir/stage.XXXXXX")
trap 'rm -rf "$staging"' EXIT
payload="$staging/jaywalk-$version"
mkdir -p "$payload"
cp "$binaries/jaywalk" "$binaries/jw" LICENSE README.md "$payload/"
cp vendor/gpui-component/LICENSE-APACHE "$payload/"
cp vendor/README.md "$payload/VENDORED.md"
cargo tree --locked --target "$host" \
    --prefix none --format '{p} {l}' > "$payload/DEPENDENCIES.txt"
cp packaging/linux/io.github.sorahn.jaywalk.desktop packaging/linux/io.github.sorahn.jaywalk.svg "$payload/"
arch=$(rpm --eval '%{_arch}')
tar -C "$staging" -czf "$topdir/SOURCES/jaywalk-$version-linux-$arch.tar.gz" "jaywalk-$version"
rpmbuild -bb --define "_topdir $topdir" --define "app_version $version" \
    --define "app_release $release" packaging/linux/jaywalk.spec
echo "RPM output: $topdir/RPMS/$arch/"
