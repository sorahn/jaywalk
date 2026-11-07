# Jaywalk

A native Rust/GPUI Jujutsu comparison viewer. The prototype uses GPUI Kit's
existing text and panel components.

## Build

Requires Rust, jj 0.47+, and full Xcode on macOS. Build without changing the
global developer directory:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo build
cargo run -- /path/to/jj/repository
```

GPUI Kit is pinned to 0.8.0: its 0.9.0 macOS backend currently uses an unstable
Rust API that fails on the installed stable compiler. The lockfile records the
verified dependency set. The existing diff component is vendored for scoped scrolling, wrapping, and
font-size extensions; see `vendor/README.md`.

## Settings

Edit `~/.config/jaywalk/config.toml` directly. When `XDG_CONFIG_HOME` is set,
the path is `$XDG_CONFIG_HOME/jaywalk/config.toml`. Use `--config PATH` for a
separate settings file. Restart the app to reload changes.

`config/default.toml` documents the shipped defaults and available keys. A user
file can override only the values it needs; omitted values inherit defaults.
Unknown keys, malformed TOML, and invalid sizes produce an error with the
config path. An explicitly supplied config file must exist.

Settings cover window size, UI/code fonts, panel default widths and limits,
history density, compact file paths, file-list wrapping, default diff view,
and separate before/after/inline code wrapping. The toolbar wrap button
changes the current view for the running session; it does not rewrite TOML.

## Development history

`main` is the minimal Rust baseline. The mutable `viewer-prototype` bookmark
holds the evolving native implementation. Use jj for history operations.

## Prototype usage

```sh
./target/debug/jaywalk /path/to/jj/repository
./target/debug/jaywalk /path/to/jj/repository --base REV --tip REV
./target/debug/jaywalk /path/to/jj/repository --file src/main.rs
./target/debug/jaywalk /path/to/jj/repository --inspect
```

The default view is the whole-branch overview. The outer left pane shows up to
200 recorded jj revisions, one message per line. The graph column reserves
space only for lanes in the visible rows, expanding and shrinking as you scroll.
Selecting a revision updates the inner file pane to that commit's changes.
Merge revisions compare against their first parent, as indicated in the view.
The Whole branch button (Cmd+Shift+B on macOS) returns to the original
branch comparison. Overview clears the selected file within the current
comparison; Refresh records a new read-only snapshot and returns to the branch.

The file pane defaults to a single-line list. Directory prefixes shorten
when space is tight, remaining distinct among the displayed changed paths;
filenames stay intact, with horizontal scrolling for long names. Its list/tree
buttons switch to a collapsible directory tree of changed files. File colors
indicate additions, modifications, renames, and deletions. Pick a file to load
its comparison. Both left panes resize independently. Double-click a sidebar
separator to restore that pane's configured default width. Double-click either
Fluid separator to restore an even code split with the configured gutter width.

The bottom-right three-way toggle selects inline, aligned side-by-side, or
Fluid diff (the default). The standard views use GPUI Kit's existing diff
widget with full context and syntax highlighting. The separate word-wrap
button to the left of the mode toggle applies to all views. Fluid retains
independent document heights, with connectors following wrapped rows.
Switching modes retains the selected file and the Fluid scroll position.
With wrapping off, Shift+scroll or a horizontal trackpad gesture scrolls
both code panes together while the divider and line numbers stay fixed.
Each pane stops at its own horizontal end.
Drag the panel separators to resize. Scroll either code pane to exercise the
experimental centre-anchored Fluid mapping and live connectors. Each change
has one thin midpoint-to-midpoint connector. Oversized blocks let attachments
slide into view along their edges, using Kaleidoscope as the visual reference
while scrolling is refined.

The change nearest the viewport center gets stronger emphasis, including its
line-number area. In Fluid mode, the footer shows its position in the file
and provides previous/next change navigation. Added/deleted blocks have a
full-width marker
on the empty side. Scrolling resumes automatic active-change selection after
using the navigation controls. The standard modes show the total change count
and use the built-in widget's previous/next navigation.

All repository commands ignore the working copy and pin reads to an operation.
Unsnapshotted working-file edits are intentionally not captured. By default,
the base is the nearest immutable ancestor of the selected tip; ambiguous
endpoints require an explicit --base/--tip. Renames read their original path
on the left and new path on the right. Binary/non-UTF-8 and oversized files
show an explanation instead of a text comparison.

This prototype has history selection, list/tree file browsing, manual refresh,
line-level diff emphasis, and optional word wrapping. Repository reads and
file/diff preparation run in background tasks; superseded requests cannot
replace the current selection. The Fluid mapping remains experimental.
Change-evolution browsing, a command palette, word-level emphasis, saved
layouts, and automatic refresh remain planned. The default-style syntax palette maps selected VS Code
colors; it is not a complete VS Code theme importer.

## Checks

```sh
cargo fmt --check
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo clippy --all-targets -- -D warnings
```

Tests use synthetic temporary jj repositories and leave external checkouts unchanged.
