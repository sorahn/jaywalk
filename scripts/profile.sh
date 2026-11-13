#!/bin/sh
# Pin the host and output path, as packaging does: never time a stale cross build.
set -eu
cd "$(dirname "$0")/.."
host=$(rustc -vV | sed -n 's/^host: //p')
if [ -z "$host" ]; then
    echo 'Could not determine the native Rust target.' >&2
    exit 1
fi
cargo build --locked --release --example profile --target "$host" --target-dir target
binary="target/$host/release/examples/profile"
case "$(uname -s)" in
    Darwin) exec /usr/bin/time -l "$binary" "$@" ;;
    Linux) exec /usr/bin/time -v "$binary" "$@" ;;
    *) exec "$binary" "$@" ;;
esac
