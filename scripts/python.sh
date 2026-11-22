#!/bin/sh
# Avoid inheriting Rosetta into Apple toolchain subprocesses on Apple Silicon.
set -eu
. "$(dirname "$0")/native-shell.sh"
if [ "$(uname -s)" = Darwin ] && [ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then
    for interpreter in "$(command -v python3 || true)" /opt/homebrew/bin/python3 /usr/bin/python3; do
        if [ -x "$interpreter" ] && /usr/bin/arch -arm64 "$interpreter" -c 'import platform,sys; sys.exit(platform.machine() != "arm64" or sys.version_info < (3, 11))' >/dev/null 2>&1; then
            exec /usr/bin/arch -arm64 "$interpreter" "$@"
        fi
    done
    echo 'A native ARM64 Python 3.11+ is required; install it with /opt/homebrew/bin/brew install python.' >&2
    exit 1
fi
exec python3 "$@"
