# Jaywalk

A native Rust/GPUI Jujutsu comparison viewer. The prototype uses GPUI Kit's
existing text and panel components.

## Build

Requires Rust and full Xcode on macOS. jj repositories require jj 0.47+;
plain Git repositories require Git. Build without changing the
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
separate settings file. Saved changes reload automatically, including when the
file is created after launch or replaced by an editor's atomic save. Configuration
polling is independent of repository refresh and controlled by
`settings.poll_interval_ms` (500 ms by default). Two matching reads let saves
settle, so updates normally appear within about a second.

Cmd+, opens this file in your default text editor. Choose another app or command
with `settings.editor`:

```toml
[settings]
editor = "Zed" # macOS app name or absolute .app path
# Or an executable plus arguments, on any supported platform:
# editor = ["code", "--reuse-window"]
```

A string selects a native app on macOS (an executable on other platforms).
An array runs a command directly without a shell, appending the config filename
as one argument. Paths with spaces need no shell escaping. Omit `editor` to
restore the default. Changes apply to the next Cmd+, or Settings menu action;
opening errors appear in the viewer.

All current settings apply live; no restart is required. Invalid TOML, unknown
keys, invalid values, or unreadable explicit files show a settings error while
the last valid configuration remains in effect. Fixing the file clears the
error. Removing the optional default config restores shipped defaults; removing
an explicit `--config` file retains the last valid settings and reports an error.

Only changed settings override runtime choices. For example, changing a font
preserves toolbar view/wrapping choices and panel visibility. Editing a default
view, wrap flag, panel visibility, width, or height applies that value immediately;
unchanged panel sizes retain manual adjustments. Dimension limits clamp the
current layout. These changes preserve the selected comparison/file and reading
position. Reducing the evolution limit retains the versions selected in either
dock even when they fall outside the new limit. Future startup-only settings
must identify themselves with a restart-required notice.

`config/default.toml` documents the shipped defaults and available keys. A user
file can override only the values it needs; omitted values inherit defaults.
Unknown keys, malformed TOML, and invalid sizes produce an error with the
config path. An explicitly supplied config file must exist.

Settings cover window size, independent fonts for application controls, source
code, History, and Changed files, panel sizes, history density, file-list/tree
defaults, diff modes, line numbers, tab width, and per-pane word wrapping.
Sidebar evolution docks inherit their sidebar's font. Font sizes are logical
pixels. Ligature controls enable or disable OpenType `liga`, `clig`, and `calt`;
the visible effect depends on the selected installed font.

```toml
[editor]
font_family = "Menlo"
font_size = 14
line_numbers = true
tab_size = 4
ligatures = false

[panels.history.font]
size = 15
# family = "Helvetica Neue"
ligatures = true

[panels.files.font]
size = 14
ligatures = true

[files]
default_view = "tree"

[keybindings]
toggle_history = ["secondary-j"]
find_file = ["secondary-k secondary-p"]
```

`ui.font_family`, `ui.font_size`, and `ui.ligatures` style application controls.
The toolbar wrap/line-number/view controls and viewer font-size commands are
session choices; they do not rewrite TOML. A later edit to the corresponding
setting overrides that choice. Viewer font-size commands leave sidebar sizes
unchanged. Changing tab width re-prepares the selected file in the background,
retaining source text and reading position.

## Keyboard commands

Shortcuts follow familiar VS Code/Zed conventions where the actions correspond.
`secondary` means Command on macOS and Control elsewhere. These are the defaults:

| Action | macOS shortcut |
| --- | --- |
| Open repository in another window | Cmd+O |
| Open settings in the system text editor | Cmd+, |
| Find changed file | Cmd+P |
| Command palette | Cmd+Shift+P or F1 |
| Toggle History | Cmd+B |
| Toggle Changed files | Cmd+Shift+E |
| Toggle word wrap | Option+Z |
| Increase/decrease/reset viewer font size | Cmd+= / Cmd+- / Cmd+0 |
| Next/previous change | Option+F5 / Option+Shift+F5 (also F7 / Shift+F7) |
| Next/previous file | Option+Down / Option+Up |
| Next/previous revision | Cmd+Option+Down / Cmd+Option+Up |
| Whole branch | Cmd+Shift+B |
| Refresh | Cmd+R |
| Close window / quit | Cmd+W / Cmd+Q |

The command palette also includes line numbers, all three diff views, evolution
review modes, and Reset panel layout. Bound commands display their current
shortcut. The complete command IDs are listed under `[keybindings]` in
`config/default.toml`. Override a command's array to replace its shortcuts;
use `[]` to remove them. Space-separated strokes form a chord. Invalid shortcuts,
unknown commands, and duplicate shortcuts assigned to different commands reject
the entire update while retaining the last valid settings/keymap. Reloading keys
preserves the toolkit's editing, selection, and copy bindings.

Shortcut references: [VS Code defaults](https://code.visualstudio.com/docs/reference/default-keybindings)
and [Zed keymaps](https://zed.dev/docs/key-bindings). Jaywalk-specific actions
keep their existing shortcuts where there is no direct editor equivalent.

## macOS application

The macOS integration provides native Jaywalk, File, Edit, View, Go, Window,
and Help menus through GPUI's AppKit bridge. Menus, shortcuts, and the palette
use the same commands. Open repository creates another window in the same
process, and remains available after the last window closes. Edit exposes
Copy and Select All for the focused source/picker. Settings opens the current
config file and creates a default config if none exists.

Build a local app bundle with an ad hoc signature:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/bundle-macos.sh
# Or: scripts/bundle-macos.sh release
open target/debug/Jaywalk.app --args /absolute/path/to/repository
```

The bundle is `target/debug/Jaywalk.app` (or `target/release/Jaywalk.app`).
It runs directly without the terminal-detachment re-exec used by the CLI.
Opening the app from Finder can start without a repository; use File > Open
repository. This is a local build, without release notarization or an installer.
Routing a second terminal launch to the existing application and global tabs
remain future work.

## Development history

`main` contains the merged native prototype. New work starts on feature
branches; the original prototype remains available as `viewer-prototype`.

## Terminal launcher and diff tools

Install `jw` from this checkout:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer scripts/install-cli.sh
# Optional bin directory and profile:
# scripts/install-cli.sh /usr/local/bin release
```

The default destination is `~/.local/bin`; add it to PATH if necessary. The
installer refuses to overwrite another command. Keep this checkout and its
`target` binaries in place while using the development link. Rerun the installer
after rebuilding to refresh the macOS app bundle.

```sh
jw .                                      # jj or Git repository
jw . --base REV --tip REV                  # explicit revision endpoints
jw . --file src/main.rs                    # select a changed file
jw diff before.txt after.txt               # standalone files
jw diff /path/to/before /path/to/after      # directory trees
jw --wait diff before.txt after.txt        # wait until all launched windows close
```

In a colocated checkout Jaywalk prefers jj. In plain Git, the default compares
HEAD with staged and unstaged worktree changes, including untracked files.
`--base REV --tip REV` compares recorded Git commits; `--tip @` means the
working tree. The graph shows Git history and commit patches (merges use the
first parent). Evolution controls require jj metadata. Git and external
comparisons refresh manually; jj keeps its recorded-operation live refresh.
Git comparisons capture file contents when loaded so later file selection does
not mix in subsequent edits. Neither backend writes to the observed repository.

External file/tree comparisons have no repository history. Directory paths are
matched literally; renames appear as an addition and deletion. Symlink targets
are compared without traversing directory symlinks. Unsupported binary/large
text is reported in the viewer. Use Refresh to reread inputs that still exist.

Configure jj's optional read-only diff tool in your jj config:

```toml
[merge-tools.jaywalk]
program = "jw"
diff-args = ["--wait", "diff", "$left", "$right"]
diff-invocation-mode = "dir"
```

Then run `jj diff --tool jaywalk`, including its usual `-r`, `--from`, `--to`,
and path filters. `--wait` keeps jj's temporary input trees alive until the
window closes. This is a diff viewer, not a `jj diffedit` or merge tool.
See [jj external tool configuration](https://docs.jj-vcs.dev/latest/config/#external-diff-tools).

Configure Git's optional difftool:

```sh
git config --global difftool.jaywalk.cmd 'jw --wait diff "$LOCAL" "$REMOTE"'
git config --global difftool.jaywalk.trustExitCode true
git difftool --no-prompt --tool=jaywalk
# Compare a whole tree in one window:
git difftool --no-prompt --dir-diff --no-symlinks --tool=jaywalk
```

Use Git's usual `--cached` or revision arguments as needed. This configuration
does not change either tool's default viewer. See [Git difftool](https://git-scm.com/docs/git-difftool).

## Prototype usage

```sh
./target/debug/jaywalk /path/to/jj/repository
./target/debug/jaywalk /path/to/jj/repository --base REV --tip REV
./target/debug/jaywalk /path/to/jj/repository --file src/main.rs
./target/debug/jaywalk /path/to/jj/repository --inspect
```

Normal GUI launches return the shell prompt immediately. The window stays open
independently of the invoking terminal, with diagnostics appended to
`~/.config/jaywalk/jaywalk.log` (under `XDG_CONFIG_HOME` when set). With
`--config PATH`, diagnostics use the adjacent file with its extension replaced
by `.log`. Argument and settings errors are reported before launching.

Use `--foreground` to keep the GUI attached and print diagnostics in the
terminal. `--inspect`, `--help`, and argument errors remain terminal commands;
they do not detach. For example:

```sh
./target/debug/jaywalk .
./target/debug/jaywalk . --foreground
```

The default view is the whole-branch overview. The outer left pane shows up to
200 recorded jj revisions, one message per line. The graph column reserves
space only for lanes in the visible rows, expanding and shrinking as you scroll.
Selecting a revision updates the inner file pane to that commit's changes.
Merge revisions compare against their first parent, as indicated in the view.
The Whole branch button (Cmd+Shift+B on macOS) returns to the original
branch comparison. Overview clears the selected file within the current
comparison. Refresh reloads recorded changes while preserving the selected
change, file, and content-based reading anchors.

Click **Evolution** in the history header to open **Change evolution** below
it. Select a history row to load recorded versions of that logical change.
**File evolution** lives below Changed files and follows the selected file;
its header button shows or hides that dock. Each timeline has its own selection,
scroll position, grouping, and rebase filter. Both share the same loaded jj
metadata. Returning from a file to the same history row restores the commit
overview immediately from the recorded snapshot, without reloading its timeline.
Refresh invalidates that snapshot.

Change evolution titles summarize what each rewrite changed: a filename for a
single-file edit, or counts such as `2 added · 1 updated`. File evolution titles
describe the focused file in its review mode, including
`renamed · contents unchanged` and `no changes to this file`. A caption identifies
the focused file and comparison. Hollow markers distinguish rows without content
changes. The commit-wide event stays in secondary details and tooltips.
Current divergent versions share the rewrite graph; selecting an older version
keeps the main history anchored to its change. Whole-branch comparisons have no
single change timeline; select a history row before exploring file evolution.

Hover a version for its description, revision ID, timestamp, and operation.

**Version patch** reviews the selected version against its first parent;
**Since previous** reviews its predecessor interdiff, accounting for changed
parents. The timeline highlights both comparison endpoints, and the same file
list and inline/split/Fluid modes display the result. The first recorded version
has no predecessor comparison. Rebases with unchanged patches show an explicit
empty comparison. Whole branch exits evolution review. Historical evolution
reviews pin the complete recorded view: external operations and Refresh leave
that review intact and display a notice. Select its history row to resume
following the logical change.

Consecutive matching entries on a linear evolution path collapse into one row
with a version count. Click the folded row to review its newest revision in the
active comparison mode. Use its chevron to expand the individual revisions;
the down chevron collapses them again. Expanding/collapsing
keeps the selected revision and diff pinned, including when that revision is
inside a collapsed group. The **Group** toggle shows all entries individually;
`evolution.collapse_repeats = false` disables grouping. Grouping uses
the focused file and review mode; overview groups must match their affected
paths and statuses, not just their displayed counts. Branch boundaries,
divergent versions, and conflicted entries stay separate.

Unchanged rebases are hidden by default. The timeline's **Rebases** toggle
reveals them; set `evolution.show_unchanged_rebases = true` to show them. Current and selected versions stay visible, and hidden nodes are
bridged in the graph without changing the comparison endpoints. Description
changes remain visible even when the same operation also changes parents.

The `[evolution]` settings control Change evolution visibility and height,
shared filter/grouping defaults, row density, and the version limit.
`[file_evolution]` controls the File evolution dock's visibility and height.
Double-click either horizontal separator to reset its configured height. Interdiff content currently requires a Git-backed jj
repository and Git on PATH. jj may synthesize backend objects while calculating
interdiffs; these queries do not update the working copy or record new operations.
Evolution renames can appear as a removal and addition. Arbitrary version-pair
comparison remains future work.

The file pane defaults to a single-line list. Directory prefixes shorten
when space is tight, remaining distinct among the displayed changed paths;
filenames stay intact, with horizontal scrolling for long names. Its list/tree
buttons switch to a collapsible directory tree of changed files. File colors
indicate additions, modifications, renames, and deletions. Pick a file to load
its comparison. Rename/copy headers show the source path. When contents are
identical, the header and footer explicitly say so instead of showing a
zero-change navigation counter: a path change is still a file change. Use
Since previous to inspect the evolution edit that introduced a move. Both left panes resize independently. Double-click a sidebar
separator to restore that pane's configured default width. Double-click either
Fluid separator to restore an even code split with the configured gutter width.

The bottom-right three-way toggle selects inline, aligned side-by-side, or
Fluid diff (the default). The standard views use GPUI Kit's existing diff
widget with full context and syntax highlighting. The separate word-wrap
button to the left of the mode toggle applies to all views. Fluid retains
independent document heights, with connectors following wrapped rows.
Switching modes retains the selected file and anchors the view to its source
line. Wrapping/reflow and returning to previously inspected files preserve
reading context; deleted anchor lines fall back to the nearest valid line.
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

The history graph labels the recorded working-copy revision explicitly, with
`no changes` or `changes`. This marker is independent of the selected row; an
empty working copy above the latest saved change is normal jj state.

All repository commands ignore the working copy and pin reads to an operation.
Unsnapshotted working-file edits are intentionally not captured. By default,
the base is the nearest immutable ancestor of the selected tip; ambiguous
endpoints require an explicit --base/--tip. Renames read their original path
on the left and new path on the right. Binary/non-UTF-8 and oversized files
show an explanation instead of a text comparison.

## Navigation and live updates

Cmd+P opens a fuzzy finder scoped to changed files in the active comparison,
including deleted and renamed paths. Cmd+Shift+P opens the command palette for
views, wrapping, refresh, panels, and change navigation. Both use the native
command component with arrow-key selection and Enter to confirm.

- Cmd+Option+Down/Up selects the next/previous history row.
- Option+Down/Up selects the next/previous changed file.
- F7 / Shift+F7 moves between changes.
- Cmd+Shift+B restores Whole branch; Cmd+R refreshes recorded changes.

Live refresh polls recorded jj operations in background tasks; `[live]`
controls `enabled` and `poll_interval_ms` (default 1500). The viewer never
snapshots unsaved/unsnapshotted working files. A normal history selection follows
its stable change ID through rewrites. Evolution selections remain pinned as
described above. With the default `--tip @`, the initial tip's sole bookmark
becomes the branch tracking expression, or its stable change ID if no single
bookmark is available. Explicit `--tip` expressions are reevaluated as supplied;
a literal commit ID therefore stays fixed. Maintain a feature bookmark when
new tip changes should be included automatically.

A refresh keeps the previous coherent view visible until replacement data is
ready. Missing/divergent changes or failed reads keep that view and report an
error rather than selecting an unrelated revision. File, comparison, and
refresh generations reject superseded results. A selected file that is no
longer changed returns to the overview with an explanation.

Both editors now emphasize changed words within modified lines, alongside
syntax colors and full-row diff shading. Fluid computes Unicode-safe word
ranges off the UI thread and applies native editor decorations. Insertions and
deletions retain full-row emphasis; unrelated paired lines and lines over
1000 bytes omit extra word shading. Standard views use the widget's existing
word emphasis.

The upper-right panel buttons toggle History, Changed files, and the evolution
docks. Panel widths, evolution heights, the Fluid split ratio, and visibility
are saved separately in `~/.config/jaywalk/layout.toml`. With `--config PATH`,
layout uses the adjacent `PATH` with its extension replaced by `layout.toml`.
Saved sizes are clamped to current settings limits; double-click resets remain
available. Writes happen off the UI thread, are serialized, and atomically
replace the state file. Deleting it restores startup defaults; settings TOML is
never rewritten by runtime controls. Panel rearrangement remains future work.

The Fluid mapping remains a prototype, with automated endpoint/wrapping tests
and native interaction checks. Arbitrary evolution-pair comparison, broader
language detection, symbol/LSP navigation, panel rearrangement, and full theme
import are deferred. The default-style syntax palette maps selected VS Code
colors; it is not a complete VS Code theme importer.

## Checks

```sh
cargo fmt --check
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo clippy --all-targets -- -D warnings
```

Tests use synthetic temporary jj repositories and leave external checkouts unchanged.
