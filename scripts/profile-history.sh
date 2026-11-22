#!/bin/sh
# Explicit native build; real repositories are read-only and never snapshotted.
set -eu
. "$(dirname "$0")/native-shell.sh"
cd "$(dirname "$0")/.."
profile_host=$(rustc -vV | sed -n 's/^host: //p')
test -n "$profile_host"
cargo build --locked --release --example history-profile --target "$profile_host" --target-dir target
exec "target/$profile_host/release/examples/history-profile" "$@"
