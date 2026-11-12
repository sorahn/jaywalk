# Development

Jaywalk is a native Rust application built with GPUI Kit for macOS and Linux.
Read the [coding standards](../CONTRIBUTING.md) before making changes.

## Prerequisites

- Rust and Cargo.
- [just](https://just.systems/) for the task recipes (`brew install just` on macOS).
  It is a separate developer tool; Cargo does not install it for this project.
- Full Xcode on macOS, including the tools used to compile the Icon Composer asset.
- jj 0.47+ for Jujutsu repositories, and Git for plain Git repositories and jj interdiffs.

## Common actions

Run `just` or `just --list` to see the available recipes:

```sh
just check                 # formatting check, tests, strict Clippy; no GUI
just build                 # build both executables
just build release
just bundle                # build/sign the debug macOS app
just bundle release
just verify-bundle         # verify an existing debug bundle's signature
just install-cli           # build and install jw into ~/.local/bin
just run                   # launch the welcome page
just run .                 # launch this repository
just run . --file src/main.rs
```

`just fmt` formats Rust sources; `just fmt-check` only checks them. `just test`
accepts Cargo test arguments, for example `just test profile_paths`. Launching is
an explicit recipe: `just check`, `just build` and `just bundle` do not open the app.

On macOS, recipes default `DEVELOPER_DIR` to
`/Applications/Xcode.app/Contents/Developer`. An existing environment value takes
precedence, so an alternate Xcode install can be selected per invocation:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer just build
```

This selects Apple's build tools; it does not change the Rust installation or
the machine's global developer-directory selection. Cargo and the scripts below
remain usable directly. The justfile delegates bundling and launcher installation
to those scripts rather than duplicating them. Recipe arguments use quoted shell
positional parameters, preserving paths with spaces and literal special characters.

## Build directly with Cargo

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

## Build and install on Fedora Linux

Run in a graphical Wayland or X11 session with a working Vulkan driver. Rust,
Cargo, a C/C++ toolchain, Clang, CMake and the platform development libraries
are needed. GPUI Kit's [installation guide](https://gpui-kit.com/docs/installation/)
describes the platform dependencies. On Fedora, the corresponding core packages are:

```sh
sudo dnf install cargo rust gcc gcc-c++ clang cmake pkgconf-pkg-config \
  fontconfig-devel wayland-devel libxkbcommon-x11-devel libX11-devel \
  libxcb-devel openssl-devel libzstd-devel vulkan-loader-devel \
  rpm-build desktop-file-utils
```

Build and package the locked source, then install the exact RPM path printed
by rpmbuild:

```sh
just package-rpm
# Or run scripts/package-rpm.sh directly.
sudo dnf install ./target/rpm/RPMS/x86_64/jaywalk-VERSION-RELEASE.x86_64.rpm
```

The script creates a local binary RPM for the build host's architecture and
libraries. It is not a Fedora distribution/source RPM recipe or a portable
cross-distribution bundle. Generated archives, staging files and RPMs stay under
`target/rpm/`. The Rust build is pinned to the compiler's native host target and
`target/rpm/cargo/`, regardless of `CARGO_TARGET_DIR` or a configured cross target.
RPMs, macOS bundles, and CLI installation share `scripts/build-native.sh` for
native target selection; its optional second argument selects a target directory.
The package version comes from Cargo.toml; the default RPM release
contains a UTC timestamp so rebuilding the same app version produces an upgrade.
An optional first argument supplies a custom RPM release.

Launch **Jaywalk** from the application menu or Rofi, or run `jw` / `jw .` from
a terminal. The package installs `/usr/bin/{jaywalk,jw}`, the desktop entry
`/usr/share/applications/io.github.sorahn.jaywalk.desktop`, and a matching hicolor
SVG icon. Windows use the same app ID for Wayland/taskbar association. No checkout
or `target/` files are needed to run the installed app. Git, xdg-utils and the
Vulkan loader and runtime-loaded Wayland client library are required; jj 0.47+ is optional for Jujutsu repositories.

Rebuild and install a newer RPM to update. Quit and relaunch an existing Jaywalk
process to use the new build. Remove with `sudo dnf remove jaywalk`; user settings
and recent-repository data under `~/.config/jaywalk/` are retained. These private
runtime files are separate from the package and should not be committed.

Linux uses Ctrl for `secondary` shortcuts, Ctrl+Shift+R for recent repositories,
and F11 for fullscreen. Platform defaults live in `config/keybindings.toml` and
apply before user overrides. Native interaction still needs desktop testing.

## Build the macOS bundle

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/bundle-macos.sh
# For an optimized build:
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/bundle-macos.sh release
```

Output is `target/debug/Jaywalk.app` or `target/release/Jaywalk.app`. The script
compiles `packaging/macos/jaywalk.icon` with Xcode's `actool`, includes modern
and legacy icon resources, merges icon metadata, and applies an ad hoc signature.
Bundling and CLI installation explicitly build the native host target into
`target/<host>/<profile>/`, ignoring inherited Cargo target-directory and
cross-target settings. Version extraction and copying use those exact artifacts;
stale binaries in `target/<profile>/` are never consumed. Ordinary Cargo and
`just build` commands still honor your Cargo settings.
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

The short form is `just install-cli [BIN_DIRECTORY] [debug|release]`, for example
`just install-cli /usr/local/bin release`. The direct script invocation is:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/install-cli.sh
# Optional destination and build profile:
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/install-cli.sh /usr/local/bin release
```

The default destination is `~/.local/bin`; keep it on PATH. The installer builds
the binaries and macOS bundle, then links `jw` to this checkout. On macOS the
link targets the launcher inside the signed bundle; on Linux it targets the
explicit native build directory. Existing links made by the previous installer
are upgraded when rerunning it with the same checkout and profile. It refuses to
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
live alongside them. `settings::Config` models the main file; resolved `Settings`
combines that configuration with an independently validated keyboard profile. Update the typed schema, defaults, and corresponding
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


## State ownership

The application owns one settings watcher per resolved config source and the
shared keymap, menus, theme defaults and quit policy. Windows subscribe to validated
updates and keep their own reading, selection and panel state. The existing
last-applied-config policy remains application-wide; project config layering is
not implemented by this separation.

Window preparation reads remembered interface choices before entering the UI
update. An application-owned writer coalesces owned layout snapshots, preserves
other repositories, and flushes pending changes on quit. Closing a window does
not cancel its queued snapshot. Newly opened windows can use the latest queued
snapshot before it reaches disk.

`review_requests::RequestClock` ties every file request to its comparison as
well as its own sequence. The native review coordinator owns task cancellation
and reconciles overview/navigation state on notifications, outside `render()`.
Viewport restoration remains tied to measured editor layout. The editor
presentation module owns background preparation, editor mounting and in-place
updates, shared standard-diff construction and word decorations.

### Panel composition

`src/viewer/chrome.rs` owns the shared panel shell and header geometry. History,
Files, the comparison workspace and both evolution timelines compose a header,
flexible body and optional footer through that shell. It is a GPUI `RenderOnce`
component over ordinary elements, without panel-specific state or commands.

Bodies retain their existing scroll ownership: virtualized history/tree lists,
Fluid editors and the unified diff widget must not acquire an outer scroll view.
The comparison shell preserves body-before-caption prepaint ordering for sticky
overview captions. Panel state, resize/docking and navigation remain with their
existing owners.

### Headless UI-state regressions

`just test --test overview_scroll` exercises GPUI entities and scroll anchors
without creating or activating a window. It covers context expansion at the
overview header and source-line anchoring while reading code. The dev-only
`gpui-test` alias enables test support on the exact GPUI core version already
used by the app; production backend features remain unchanged. Import that
alias as `gpui` in context-backed integration tests, and import the normal Kit
facade for components. Native rendering and interaction still need separate
verification.

`just test-ui` runs the viewer interaction suite using GPUI simulated windows,
keyboard and pointer events. It covers recent-picker filtering/selection/Escape,
commit hover without selection, click-to-inspect, drawer buttons and footer
visibility, and header alignment after window resizing and divider drags. It uses
disposable Git fixtures, in-memory recents and disabled layout persistence/live
watching. It does not activate a native window or move the system pointer. These
tests also run as part of `just check`; native rendering still needs review.

The same headless suite exercises Fluid wheel input, shared gutter dragging,
independent editor offsets, wrapping and window/divider reflow. Geometry tests
cover insertion/deletion-only sides, unequal replacements, direction reversal
and endpoint clamping with different wrapping heights. These are behavioral
regressions, not substitutes for native visual comparison with reference clips.

Startup coverage also paints geometry before releasing a repository load. Native
startup still has a separate smoke check: the simulated platform did not expose
a stale-frame dependency issue found in inactive native windows. Keep the owning
viewer in the per-frame entity dependency set when adding geometry callbacks.
