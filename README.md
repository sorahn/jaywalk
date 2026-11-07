# Jaywalk

A native Rust/GPUI Jujutsu comparison viewer. The prototype uses GPUI Kit's
existing text and panel components. Plans and research live in Homelab in the
Obsidian vault.

## Build

Requires Rust, jj 0.47+, and full Xcode on macOS. Build without changing the
global developer directory:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo build
cargo run -- /path/to/jj/repository
```

GPUI Kit is pinned to 0.8.0: its 0.9.0 macOS backend currently uses an unstable
Rust API that fails on the installed stable compiler. The lockfile records the
verified dependency set. No framework patches are required.

## Development history

`main` is the minimal Rust baseline. The mutable `viewer-prototype` bookmark
holds the evolving native implementation. Use jj for history operations.

## Prototype usage

```sh
./target/debug/jaywalk ~/src/man-site
./target/debug/jaywalk ~/src/man-site --base REV --tip REV
./target/debug/jaywalk ~/src/man-site --file src/server/server.ts
./target/debug/jaywalk ~/src/man-site --inspect
```

The default view is a changeset overview. Pick one file from the left panel.
The bottom-right three-way toggle selects inline, aligned side-by-side, or
Fluid diff (the default). The standard views use GPUI Kit's existing diff
widget with full context, syntax highlighting, and wrapped long lines. Fluid
retains independent document heights and its unwrapped code panes. Switching
modes retains the selected file and the Fluid scroll position.
Drag the panel separators to resize. Scroll either code pane to exercise the
experimental centre-anchored Fluid mapping and live connectors. Each change
has one thin midpoint-to-midpoint connector. Oversized blocks let attachments
slide into view along their edges, using
Kaleidoscope as the visual reference while scrolling is refined. Overview
returns to the summary; Refresh reloads a coherent recorded jj operation.

The change nearest the viewport center gets stronger emphasis, including its
line-number area. In Fluid mode, the footer shows its position in the file and provides
previous/next change navigation. Added/deleted blocks have a full-width marker
on the empty side. Scrolling resumes automatic active-change selection after
using the navigation controls. The standard modes show the total change count
and use the built-in widget's previous/next navigation.

All repository commands ignore the working copy and pin reads to an operation.
Unsnapshotted working-file edits are intentionally not captured. By default,
the base is the nearest immutable ancestor of the selected tip; ambiguous
endpoints require an explicit --base/--tip. Renames read their original path
on the left and new path on the right. Binary/non-UTF-8 and oversized files
show an explanation instead of a text comparison.

This first prototype has a flat file list, manual refresh, line-level diff
emphasis, and wrapping disabled. The Fluid mapping is an experiment, not an
exact reproduction of Kaleidoscope. Graph/evolution browsing, a command
palette, word-level emphasis, saved layouts, and automatic refresh remain in
the Homelab plan. The default-style syntax palette maps selected VS Code
colors; it is not a complete VS Code theme importer.

## Checks

```sh
cargo fmt --check
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo clippy --all-targets -- -D warnings
```

Tests use synthetic temporary jj repositories and do not modify man-site.
