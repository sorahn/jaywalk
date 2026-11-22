# Development

Jaywalk is a native Rust application built with GPUI Kit for macOS and Linux.
Read the [coding standards](../CONTRIBUTING.md) before making changes.

## Prerequisites

- Rust and Cargo (validated with Rust 1.100.0).
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

On Apple Silicon, run Python tooling through `scripts/python.sh` (for example,
`scripts/python.sh -m unittest discover -s scripts -p 'test_*.py'`). Repository
recipes and workflows use this wrapper. It selects and explicitly launches an
ARM64 Python 3.11+ interpreter even when PATH contains an Intel Python, preventing
its Apple tool subprocesses from inheriting Rosetta. Shell entry points also
re-launch natively when started by an Intel parent.
These guards leave other platforms unchanged.
Native bundle builds reject an Intel Rust host on Apple Silicon. Local bundles
validate both app executables and every nested Mach-O against the native host,
so Intel Macs can still build and install locally. Release bundles, packaging and
release manifests retain an ARM64 requirement; Intel-only helpers are rejected,
while universal helpers with ARM slices work. The `check-architecture --native`
option applies only to local bundle validation, never release metadata.

## Profile PR opening

Normal PR opens and updates emit `PR open:` timing lines to stderr (`jaywalk.log` in the default config directory for normal detached launches; an explicit `--config` uses that config path with a `.log` extension). They report account-loading time, manifest resolution, repository matching, Git acquisition, file metadata, verification and optional checkout, plus a logical review-API request count. Token refresh, identity validation and HTTP retries are not separate entries in that request count. The diagnostics omit account/repository identifiers, URLs, paths, commits and tokens. Cancellation and failures also record completed stages.

A deterministic synthetic profile is available without a desktop, credentials or network access:

```sh
just test --lib profile_bitbucket_open_with_simulated_network_latency -- --ignored --nocapture
```

It uses a 44-file Bitbucket PR and a simulated 100 ms latency per API request. On the development Mac it reduced 52 review requests (46 file-metadata reads) and roughly 5.4 seconds in hosted mode to six requests (zero file-metadata reads) and roughly 1.1 seconds in local isolated mode. These are harness measurements, not a live-service guarantee; checkout, authentication, Git transport and repository size add their own costs.

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

## Build and install on Fedora or openSUSE Linux

Run in a graphical Wayland or X11 session with a working Vulkan driver. Rust,
Cargo, a C/C++ toolchain, Clang, CMake and the platform development libraries
are needed. GPUI Kit's [installation guide](https://gpui-kit.com/docs/installation/)
describes the platform dependencies. On Fedora, the corresponding core packages are:

```sh
sudo dnf install cargo rust gcc gcc-c++ clang cmake pkgconf-pkg-config \
  fontconfig-devel wayland-devel libxkbcommon-x11-devel libX11-devel \
  libxcb-devel openssl-devel libzstd-devel vulkan-loader-devel dbus-devel \
  rpm-build desktop-file-utils
```

On openSUSE Tumbleweed, install the prerequisites with:

```sh
sudo zypper install rust cargo gcc gcc-c++ clang cmake pkgconf-pkg-config \
  fontconfig-devel wayland-devel libxkbcommon-devel libxkbcommon-x11-devel \
  libX11-devel libxcb-devel libopenssl-devel libzstd-devel vulkan-devel dbus-1-devel \
  rpm-build desktop-file-utils
```

Account credentials use the desktop's D-Bus Secret Service (for example GNOME
Keyring or KWallet). A running, unlocked Secret Service is needed for account
features; local diff viewing does not read the credential store at startup.
There is no plaintext or Linux kernel-keyring fallback.

`just` is optional; the packaging script can be run directly.

Build and package the locked source, then install the exact RPM path printed
by rpmbuild:

```sh
just package-rpm
# Or run scripts/package-rpm.sh directly.
sudo dnf install ./target/rpm/RPMS/x86_64/jaywalk-VERSION-RELEASE.x86_64.rpm
# On openSUSE:
sudo zypper install ./target/rpm/RPMS/x86_64/jaywalk-VERSION-RELEASE.x86_64.rpm
```

The script creates a local binary RPM for the build host's architecture and
libraries. RPM's native distribution macros select the Fedora or openSUSE
Vulkan and Wayland runtime package names. Build on the distribution where the
package will be installed. It is not a distribution/source RPM recipe or a
portable cross-distribution bundle. Generated archives, staging files and RPMs stay under
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
process to use the new build. Remove with `sudo dnf remove jaywalk` on Fedora or
`sudo zypper remove jaywalk` on openSUSE; user settings
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
and legacy icon resources, merges icon metadata, and signs the helper and app.
It uses the single valid Apple Development identity, or ad hoc signing when no
Apple Development identity is installed. If identities are installed but none
is usable (for example, an expired certificate), bundling stops rather than
silently changing the app’s Keychain identity. Renew/repair the certificate, or
explicitly select `JAYWALK_SIGNING_IDENTITY=-` for an intentional ad hoc build.
If several valid development identities are installed, choose one explicitly.
`JAYWALK_SIGNING_IDENTITY` accepts a certificate fingerprint or name;
`-` explicitly requests ad hoc signing. No certificates are created or trusted
by the scripts.

```sh
JAYWALK_SIGNING_IDENTITY="CERTIFICATE_FINGERPRINT" just bundle
```

Certificate-backed signing preserves code identity across rebuilds. Ad hoc
signatures identify one exact binary, so Keychain's Always Allow can prompt
again after a rebuild. Use the signed bundle when checking account integration;
`cargo run` does not apply certificate-backed signing. Existing development
credential entries may require one initial approval for the new identity.
Changing certificate/identifier can require approval again. Do not remove
credential access controls to suppress prompts. This is local signing, not
notarization or proof of distribution readiness.

The `account-store-probe` example uses only a dedicated synthetic Keychain
entry. To check access across two different builds, build with
`JAYWALK_SIGNING_PROBE_VERSION=one cargo build --locked --example account-store-probe`,
sign the resulting executable with `scripts/sign-macos.sh PATH dev.jaywalk.SigningProbe`,
and run `PATH write`. Rebuild with version `two`, sign with the same identity and
identifier, then run `PATH read` and `PATH delete`. It must recover the synthetic
credential without a new item-access prompt. The signing key itself may prompt
for permission when codesign first uses it. Never run this check against real
account entries.
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

GPUI Kit is pinned to 0.10.0 in `Cargo.toml`. Keep the lockfile when building;
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

Release CI allocates the app's calendar version after successful main checks and
supplies `JAYWALK_RELEASE_VERSION` at build time. Cargo.toml intentionally omits
its version; Cargo.lock retains the internal `0.0.0` package identity. No version
bumps are committed. The bundle builder reads the compiled application's
numeric version through `--package-version`.

Local builds display `dev (REVISION)` with a dirty marker when applicable,
including local release-profile builds. A release with an injected version
displays that calendar version. `jw --version` and **About Jaywalk** share the
compiled identity; a running process retains it until restarted. For headless
version-injection checks, use:

```sh
JAYWALK_RELEASE_VERSION=2026.11.42 cargo test --locked --test build_version --test versioning
```

This does not create a release or tag. Release notes live in GitHub Releases.

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

## Performance measurements

`just profile --jj` builds a release-profile synthetic domain harness for the
native Rust host in an explicit target directory, then runs
it under the platform's process-memory/time reporter. It creates and removes its
own temporary repositories; it never profiles or changes your checkout. Omit
`--jj` when jj is unavailable. `--samples 5` increases repetitions and
`--large-mib 50` increases the generated text workload and exercises capture at
the default size limit (actual byte counts are reported). `--wide-history` adds
700 distinct branch tips with a 1,000-row history limit. Output is JSON Lines with separate diff, syntax, full-patch preparation/parsing, Fluid scrolling,
Git capture/refresh, overview, references, history and optional jj evolution
measurements. Resident-memory snapshots are taken outside operation timers,
before the samples and after their returned values are dropped. Retained allocator
pages are not proof of a leak. Fixture generation is excluded from operation timings; total
process time and peak memory include it. Runs are warm-cache measurements, not
cold disk/cache benchmarks. Required Linux process reporting uses `/usr/bin/time`.

`just profile-ui` measures synthetic windows on GPUI's test platform in release
mode. It covers reference counts through 5,000, repeated open/redraw, viewer
loading, resizing and wheel input across diff modes. A draw includes three
settling frames; reported durations are not native GPU frame times. These
profiling tests are ignored during normal `just check` runs and contain no
wall-clock correctness assertions.

Record the source revision, OS, hardware, profile, fixture sizes and raw output
when comparing runs. Use fresh processes for peak-memory comparisons. Native
profiling and selection/copy/scrolling checks need a coordinated graphical
session; successful headless measurements do not establish native appearance or
interaction behavior.

For native frame measurements, build with `cargo build --release --features
profiling --bin jaywalk`, then launch an isolated instance with
`JAYWALK_PROFILE_FRAMES=1` and `--foreground`. Set `XDG_CONFIG_HOME` to a temporary
directory to isolate settings, recent repositories and instance routing. Once per
second, changed frame counters are emitted to stderr as JSON: cumulative CPU draw
percentiles, dirty-to-present latency and presentation intervals. Sampling does
not request redraws and stops when the window closes. These diagnostics require
both the opt-in build feature and environment variable; normal builds are
unaffected. CPU draw time is not GPU execution time, and presentation intervals
should not be interpreted as a constant FPS during idle periods.

### Read-only history profiling

`just profile-history --repo /path/to/repository --label example` builds the native
release `history-profile` example. It reads Git metadata, including working-copy
status with optional index locks, fsmonitor and status hooks disabled. It never
checks out, fetches, snapshots jj, or changes the repository. Run each case in a
fresh process; `--page-size 500 --pages 10 --retain-snapshots` additionally models
Back/Forward retaining older page snapshots. `--initial-batch-size 100` measures a small first batch followed by background-equivalent
filling of the configured page; `--search` changes the query. Output
contains counts/timings rather than private commit messages, authors or paths.

Capture/reference discovery, initial page, subsequent pages and full-source
search are timed separately. On macOS, memory counters are cumulative process
peak RSS and the largest completed Git child's peak RSS, in KiB; they are not
additive. This harness exercises backend parsing and graph layout, excluding
native rendering, syntax setup and full application memory. Warm filesystem
caches, page position, graph fanout and other running workloads affect timings.
Keep private real-repository results in the personal notes store; repository
regressions use disposable synthetic fixtures only. Streamed metadata fields
are bounded to 1 MiB and query stderr to 64 KiB; errors retain the current view.

### Recorded jj overview content

A recorded comparison captures file kinds and object IDs for both immutable
endpoints once, using the pinned jj operation. It resolves the Git object-store
directory once, including for non-colocated repositories. Overview preparation
then reads normal blobs through one size-checked `git cat-file --batch` stream.
The stream belongs to the read session and closes when preparation finishes;
comparison/navigation snapshots retain metadata, not idle child processes or
unbounded decoded-text caches. Oversized, binary or invalid replies reset the
stream before another file is read. Conflicted/materialized entries retain the
operation-pinned, bounded `jj file show` fallback. Missing Git selects that same
fallback for normal jj files; only an executable-not-found error does so.
Permission, object-store and protocol errors remain visible. The reader remembers
Git's absence for its session, and jj history/reference discovery uses jj's own
remote listing. Specialized evolution interdiffs still read synthesized Git
objects directly and require Git; they cannot substitute original commit files
without changing the comparison's meaning.

### Publisher GitHub setup and account errors

Shipped builds resolve the public GitHub client from compile-time
`JAYWALK_GITHUB_CLIENT_ID`, `JAYWALK_GITHUB_CLIENT_SECRET`, and
`JAYWALK_GITHUB_APP_SLUG`. These describe the publisher's public native app;
private keys and user tokens are never build inputs. Browser sign-in retains
PKCE, state and the fixed loopback callback. A fresh OS store automatically
uses this bundled client, with no development setup import. Explicit local
registration selections retain priority. Recovered sessions resolve their
exact client ID; another client's credentials are never substituted. Store
access denial remains an error rather than silently switching clients.

`jaywalk --hosting-capabilities` reports the bundled GitHub capability as JSON
without reading Keychain or making network requests. Partial or invalid setup
reports unavailable; official packaging checks it before notarization. Local
builds without these inputs remain developer-only for provider connection.
The Accounts page links to the public app's repository installation page when
configured; installation determines access to private repositories separately
from user authorization.

Account error notices and store errors use the shared selectable diagnostic
component with **Copy error**, preserving complete literal multiline text.
These are sanitized application error messages, never raw OAuth responses,
callback URLs or credentials. Publisher setup failures now identify a missing
or invalid application setup rather than telling ordinary users to perform
developer setup. Public Bitbucket registration/provisioning remains separate.

## File picker performance checks

The file picker searches the full captured comparison catalog, but mounts at most
200 results. Its four-query cache keeps the empty query and three recent searches
for the current comparison identity; it retains paths, status and identity only.
A refreshed comparison invalidates these results. Narrow the query to reach files
outside the initial rows. Opening the picker does not scan the working tree.

Run the opt-in synthetic headless measurement with:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
cargo test --locked --release --target aarch64-apple-darwin --bin jaywalk \
  viewer::performance_tests::profile_file_palette -- --ignored --nocapture --test-threads=1
```

`JAYWALK_PROFILE_PALETTE_FILES` changes the default 10,000-file fixture size.
The output reports cold/repeat openings, redraws and a full-catalog search, each
including three settling frames. These are test-platform timings, not native GPU
latency. The ordinary interaction regression also checks bounded rows, cache reuse
and opening a file beyond the initial results.

## Existing GitHub credential proof of concept

`examples/github-token-poc.rs` exercises the application's real read-only
`HttpProvider` with a copied bearer credential, independently of OAuth session
registration and application account configuration. It is not an in-app connection
flow. Run an explicit import using a public test PR:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo run --locked \
  --example github-token-poc -- import-gh \
  --pull-request https://github.com/OWNER/REPOSITORY/pull/NUMBER
```

The probe runs `gh auth token --hostname github.com` with ambient token overrides
removed, captures stdout privately and never prints the token or API response
bodies. `gh` must already have a saved connection. The subprocess has a timeout,
output is bounded and diagnostics never include its stdout/stderr. Network reads
use Jaywalk's existing host-checked, redirect-disabled provider transport. Identity
validation must succeed before saving; per-operation permission failures are
reported independently and do not imply invalid authentication.

The only persisted credential is a separately typed token record in OS-store
service `jaywalk.hosting-auth-poc.v1`, account `github-token-validation`.
Application accounts, developer registrations and publisher validation sessions
are untouched. A separate process can repeat the checks without invoking `gh`:

```sh
target/debug/examples/github-token-poc restore \
  --pull-request https://github.com/OWNER/REPOSITORY/pull/NUMBER
target/debug/examples/github-token-poc forget
```

`forget` deletes only this copy, not the source credential or its remote grant.
The probe checks identity, authored/requested-review discovery, direct lookup,
changed files, a pinned source tree/blob and checks/status/review endpoints.
Successful empty search results still establish request acceptance, not coverage
of every repository or permission combination. Imported GitHub CLI credentials
may themselves be OAuth tokens; this test does not establish fine-grained PAT
coverage or transfer the CLI's refresh lifecycle. Integration still needs explicit
OAuth/personal-token session types, migration and replacement/expiry handling.
