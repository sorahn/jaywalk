# Development

Jaywalk is a native Rust application built with GPUI Kit. macOS is the current
development target. Read the [coding standards](../CONTRIBUTING.md) before making changes.

## Prerequisites

- Rust and Cargo.
- Full Xcode on macOS, including the tools used to compile the Icon Composer asset.
- jj 0.47+ for Jujutsu repositories, and Git for plain Git repositories and jj interdiffs.

Use the full Xcode developer directory per command without changing the machine's
global developer-directory selection:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo build --locked
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo run -- /path/to/repository
```

Omit the repository argument to open the welcome page; use `.` explicitly for
the current directory. Normal launches detach from the terminal. For attached
GUI diagnostics, run the built executable with `--foreground`:

```sh
./target/debug/jaywalk . --foreground
./target/debug/jaywalk . --inspect
```

`--inspect` prints a comparison summary without opening a window. `--help` and
`--version` also remain terminal commands. GUI commands open native windows;
do not use them as unattended checks that might interrupt someone's desktop.

## Build the macOS bundle

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/bundle-macos.sh
# For an optimized build:
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/bundle-macos.sh release
```

Output is `target/debug/Jaywalk.app` or `target/release/Jaywalk.app`. The script
compiles `packaging/macos/jaywalk.icon` with Xcode's `actool`, includes modern
and legacy icon resources, merges icon metadata, and applies an ad hoc signature.
Generated assets stay under `target/`. This is a local build, without notarization
or an installer.

The script replaces the bundle by renaming directories, preserving a running
executable rather than overwriting it. It refreshes Launch Services registration.
After saving changes in Icon Composer, rebuild the bundle and manually quit and
relaunch the app to see the new icon and code. Rebuilding does not restart an
existing process.

Verify the signature without launching the app:

```sh
codesign --verify --deep --strict target/debug/Jaywalk.app
```

## Install the development launcher

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/install-cli.sh
# Optional destination and build profile:
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/install-cli.sh /usr/local/bin release
```

The default destination is `~/.local/bin`; keep it on PATH. The installer builds
the binaries and macOS bundle, then links `jw` to this checkout. It refuses to
replace an unrelated existing command. Keep the checkout and its `target/`
binaries in place while using the link. Rerun after rebuilding to refresh the bundle.

Quit an older running Jaywalk process manually after installing a new build.
Subsequent terminal launches route to the running application, so an existing
process continues using its old code until restarted.

## Checks

```sh
cargo fmt --check
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo clippy --all-targets -- -D warnings
```

Repository tests use synthetic temporary fixtures and leave external checkouts
unchanged. Background checks verify models and repository boundaries; native
selection, scrolling, focus, and layout still need interaction review. Run native
checks deliberately rather than stealing focus during unrelated work.

## Dependencies and local patches

GPUI Kit is pinned to 0.8.0 in `Cargo.toml`. Keep the lockfile when building;
framework upgrades need separate compatibility checks against stable Rust and
our vendored component patch. [vendor/README.md](../vendor/README.md) records
patch provenance, changes, and licenses. The existing component supplies text,
diff, and panel widgets; Jaywalk does not implement its own UI framework.

Shipped settings and shortcuts are separate canonical files:
[`config/default.toml`](../config/default.toml) and
[`config/keybindings.toml`](../config/keybindings.toml). Sparse user templates
live alongside them. Update the typed schema, defaults, and corresponding
[settings](settings.md) or [keybinding](keybindings.md) documentation together.

## Versions and diagnostics

`Cargo.toml` owns the release version; update `Cargo.lock` alongside it.
The bundle builder reads the executable's package version. Debug builds show
`VERSION-dev (REVISION)` with a dirty marker when applicable; release-profile
builds show the release version. `jw --version` and **About Jaywalk** identify
the compiled build. A running process retains that identity until restarted.
Release notes will live in GitHub Releases when releases begin.

Detached GUI diagnostics append to `~/.config/jaywalk/jaywalk.log`, or beneath
`$XDG_CONFIG_HOME/jaywalk/` when set. An explicit `--config PATH` uses the
adjacent file with its extension replaced by `.log`. Argument and settings
errors are reported before launch. Use `--foreground` when you need live
terminal output.

On macOS, subprocesses preserve the app's inherited PATH and append standard
Homebrew, Cargo, and system tool directories when absent. This lets a Finder-launched
instance locate jj and Git when a later `jw` launch reuses it. Jaywalk does not
run shell startup files or change the process-wide environment. Custom tool
locations must be present in the app's launch environment.

## Syntax highlighting and file sources

GPUI Kit supplies Tree-sitter grammars and queries through `gpui-component`;
Jaywalk enables the complete `tree-sitter-languages` bundle. Detection is shared
by the views in `src/syntax.rs`. It uses provider names and aliases plus a central
table for extensions and conventional filenames such as `Makefile` and `Cargo.lock`.
Unknown files remain plain text; highlighting coverage depends on the provider's
queries. Markdown fences can use other bundled grammars.

Add an alias there for a provider-supported extension that is not already detected.
New grammars outside the bundle need a dependency and provider registration;
custom grammar loading and user-defined file associations remain future work.

File identity and change metadata are independent. Counts and change navigation
use the comparison's change list; source reads use explicit before/after paths.
Recorded jj reads stay pinned to an operation, and evolution uses the interdiff's
synthesized objects. Git worktree comparisons capture changed contents when loaded
and read unchanged files from pinned base objects. External comparison inputs
retain read-on-selection behavior. Preserve these boundaries when changing file
browsing or refresh so one comparison cannot silently mix different snapshots.
