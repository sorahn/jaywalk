# Cargo and scripts remain usable directly. Explicit DEVELOPER_DIR overrides win.
set shell := ["sh", "-eu", "-c"]
set positional-arguments
export DEVELOPER_DIR := env("DEVELOPER_DIR", if os() == "macos" { "/Applications/Xcode.app/Contents/Developer" } else { "" })

# List the available actions without building or launching anything.
[private]
default:
    @{{ quote(just_executable()) }} --list

# Build both executables; accepts debug (default) or release.
build profile="debug":
    case "$1" in debug) cargo build --locked --bins ;; release) cargo build --locked --bins --release ;; *) echo 'profile must be debug or release' >&2; exit 2 ;; esac

# Format first-party Rust code.
fmt:
    cargo fmt --all

# Check Rust formatting without editing files.
fmt-check:
    cargo fmt --all -- --check

# Run background tests; optional arguments are passed to Cargo.
test *args:
    cargo test --locked "$@"

# Exercise simulated windows/input without native activation or mouse movement.
test-ui:
    cargo test --locked --bin jaywalk viewer::interaction_tests

# Check all targets with warnings treated as errors.
clippy:
    cargo clippy --locked --all-targets -- -D warnings

# Run formatting checks, tests, and strict Clippy without launching the app.
check: fmt-check test clippy

# Build and sign the macOS app; accepts debug (default) or release.
[macos]
bundle profile="debug":
    scripts/bundle-macos.sh "$1"

# Build a native Fedora/openSUSE RPM; optionally supply its release value.
[linux]
package-rpm release="":
    scripts/package-rpm.sh "$1"

# Verify an existing app bundle's signature without launching it.
[macos]
verify-bundle profile="debug":
    case "$1" in debug|release) codesign --verify --deep --strict "target/$1/Jaywalk.app" ;; *) echo 'profile must be debug or release' >&2; exit 2 ;; esac

# Build and install jw; optional arguments select bin directory and build profile.
install-cli directory=(env("HOME") / ".local/bin") profile="debug":
    scripts/install-cli.sh "$1" "$2"

# Launch the native app; omit arguments for welcome, or pass . for this repository.
run *args:
    cargo run --locked --bin jaywalk -- "$@"

# Synthetic domain profiling; build outside the process-memory measurement.
profile *args:
    scripts/profile.sh "$@"

# Synthetic UI timings; never opens a native desktop window.
profile-ui:
    profile_host=$(rustc -vV | sed -n 's/^host: //p'); test -n "$profile_host"; cargo test --locked --release --target "$profile_host" --target-dir target --bin jaywalk viewer::performance_tests -- --ignored --nocapture --test-threads=1
