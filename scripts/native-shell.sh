# Sourced by script entry points; keep Apple helpers out of Rosetta even when
# an Intel terminal/interpreter starts a universal /bin/sh on Apple Silicon.
if [ "$(uname -s)" = Darwin ] && [ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ] && [ "$(uname -m)" != arm64 ]; then
    exec /usr/bin/arch -arm64 /bin/sh "$0" "$@"
fi
