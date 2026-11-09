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

## Reading across comparisons

The comparison toolbar reserves its vertical space while revisions and evolution
metadata load. Its previous label and controls stay visible during fast metadata
loads, without briefly substituting a commit hash. Retained controls cannot act
on the old timeline while replacement metadata is pending. The previous
changed-file list stays visible until its replacement arrives. Loading indicators appear only after `[ui].loading_delay_ms` (200 ms by
default); this does not delay loading or displaying results. Set it to `0` for
immediate feedback. `[ui].comparison_bar_height` sets the toolbar's minimum height
(36 logical pixels by default); larger UI fonts can increase it. Both settings
apply live.


Changing commits, evolution versions, or returning to Whole branch keeps the
open file and restores its source-line reading position when possible. Live
refresh follows the same behavior. Matching content anchors account for lines
inserted above the viewport; removed or rewritten anchors fall back to the
nearest valid line. View mode and wrapping remain selected.

If the selected comparison does not change that file, its last loaded diff
stays visible as a retained preview, marked by an eye icon beside the filename
and an italic row in its normal sorted position in either file list or tree. It is excluded from changed-file counts. Selecting another file
or Overview closes that temporary preview. If a later comparison changes the
file, its new diff replaces the preview automatically. Explicit detected
renames are followed; copies do not move the open file to their destination.

## Version

`Cargo.toml` owns the release version, including the macOS bundle's version.
Run `jw --version` (or `jaywalk --version`), or choose **Jaywalk → About Jaywalk**
to identify the build. About also offers Copy version and remains available with
no repository windows open.

Development builds show `0.1.1-dev (REVISION)`, with `-dirty` for modified
checkouts. Release-profile builds show `0.1.1`. The About window includes source
provenance for either build. A running process retains its compiled identity
until it is quit and restarted. Release notes will live in GitHub Releases when
releases begin.

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

The editable Icon Composer source is `packaging/macos/jaywalk.icon`.
The bundle script compiles it with Xcode's `actool`, includes `Assets.car`
and a legacy `.icns` fallback, and merges the generated icon metadata before
signing. Generated icon resources stay under `target/`. The script refreshes
the bundle timestamp and its Launch Services registration so the Dock notices
icon updates. Rebuild after saving icon changes, then quit and relaunch the app
to see them.

The bundle is `target/debug/Jaywalk.app` (or `target/release/Jaywalk.app`).
It runs directly without the terminal-detachment re-exec used by the CLI.
Opening the app from Finder can start without a repository; use File > Open
repository. This is a local build, without release notarization or an installer.
Terminal launches route to the running application. Reopening the same
repository focuses its window; other repositories open additional windows in
the same process. Global tabs remain future work.

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
jw --wait diff before.txt after.txt        # wait until this review window closes
```

`jw .` reuses a repository's existing window, including launches through a
symlink or from a subdirectory. The current file/review selection stays intact
unless `--file` requests another file. Different explicit comparison endpoints
or config files can use separate windows. All windows share one application
process, including concurrent cold launches and File > Open repository.

`--wait` belongs to the requested window: closing another window does not
release it, and reusing an existing window waits for that window to close.
Quitting the application releases waiting clients as well. On macOS the app
remains available after its last window closes. Stale launch endpoints from a
crash are recovered automatically. The local transport currently supports
macOS and Linux; a Windows transport remains future work.

After installing a new build, quit any older running Jaywalk instances once
to load it. Subsequent launches use the new shared process.

In a colocated checkout Jaywalk prefers jj. In plain Git, the default compares
HEAD with staged and unstaged worktree changes, including untracked files.
`--base REV --tip REV` compares recorded Git commits; `--tip @` means the
working tree. The graph shows Git history and commit patches (merges use the
first parent). Evolution controls require jj metadata. Git comparison contents and external
comparisons refresh manually; local Git reference metadata and recorded jj
operations refresh live.
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
comparison. The overview shows the selected commit's full message, author and
recorded time, followed by a unified diff for every changed file. Whole-branch
comparisons identify their scope and show the tip commit's message; working-tree
and external comparisons have no recorded commit message. Unchanged ranges fold
to three context lines by default. Use the range disclosures to reveal more
context, **Show full files** to expand everything, and **Open file** in a file
header to switch to the regular viewer. Empty files, renames and unavailable text
previews remain listed. Overview uses the inline wrapping preference and existing
next/previous-change commands; Fluid and split controls belong to the file viewer.

Overview file headers stick to the top of the diff viewport; the next header
pushes the current one away. The sidebar uses a reading tint for the top file,
separate from file selection, and scrolls only enough to keep that row visible.
Collapsed folders stay collapsed and carry the indicator for a contained file.
Tracking neither opens a file nor moves keyboard focus. List mode follows the
same directory-first traversal as tree mode, omitting directory rows; the overview
uses that ordering too.

The `[overview]` settings control `context_lines` (default 3), `expansion_lines`
(default 20) and `message_max_height` (default 220 logical pixels). They apply
live. Overview preparation runs in the background and keeps the previous complete
view visible until the new comparison is ready. Refresh reloads recorded changes
while preserving the selected change, file, and content-based reading anchors.

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
widget with full context and syntax highlighting. The complete GPUI Kit
Tree-sitter language bundle is enabled in all three diff views, including
Markdown, TOML, Make, Rust, JavaScript/TypeScript, JSON, YAML, Python, Go, C/C++,
and the other grammars shipped by the provider. The separate word-wrap button
to the left of the mode toggle applies to all views. Fluid retains
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
snapshots unsaved/unsnapshotted working files. With the default `--tip @`,
each refresh resolves the recorded workspace reference at one pinned operation,
including when a squash or abandon replaces its working-copy change. Explicit
`--tip` expressions are reevaluated as supplied; a literal commit ID stays
fixed. Use `--tip <bookmark>` to follow a feature independently of the checkout.

Normal history selections follow stable change IDs through rewrites. If a
selected working-copy change is abandoned while following `@`, the viewer can
follow the replacement working copy. Other missing/divergent selections keep
the previous view with an explanation. Evolution selections remain pinned as
described above.

A refresh keeps the previous coherent view visible until replacement data is
ready. Failed reads are not automatically retried against the same recorded
operation: another operation, selecting another scope, or a manual Refresh
allows a new attempt. File, comparison, and refresh generations reject
superseded results. An open file that is no longer changed stays as a retained
preview. Refresh notices and settings errors occupy the existing footer,
truncate to one line, and expose their full text on hover; they never add a row
or resize the panels.

Opening a changed file without a remembered reading position reveals its first
change. Returning to a file preserves your reading position. Fluid change arrows
remain available at both ends, including a single-change file; if the current
change is off-screen, either arrow reveals it before advancing.

Both editors now emphasize changed words within modified lines, alongside
syntax colors and full-row diff shading. Fluid computes Unicode-safe word
ranges off the UI thread and applies native editor decorations. Insertions and
deletions retain full-row emphasis; unrelated paired lines and lines over
1000 bytes omit extra word shading. Standard views use the widget's existing
word emphasis.

Panel buttons toggle History, Changed files and the evolution docks. Interface
choices are stored per repository in `~/.config/jaywalk/interface.toml`, or a
sibling `*.interface.toml` for alternate config profiles. Saved sizes are clamped
to current settings limits; double-click resets remain available. Writes happen
off the UI thread, are serialized, and atomically replace the state file.
Settings TOML is never rewritten by runtime controls. Set
`layout.remember_per_repository = false` to use configured defaults on later opens.

The Fluid mapping remains a prototype, with automated endpoint/wrapping tests
and native interaction checks. Arbitrary evolution-pair comparison, broader
language detection, symbol/LSP navigation, panel rearrangement, and full theme
import are deferred. The default-style syntax palette maps selected VS Code
colors; it is not a complete VS Code theme importer.

## Local and remote history

History branches keep their lanes and colors until they rejoin a commit dot.
Branches stack outward, with the rightmost route drawn on top. Turns use the
35% logo curve in `assets/branch-curve-35pct.svg`, mapped between row centers
and split at row boundaries for continuous scrolling.

History rows stay single-line, with one bounded reference badge and a `+N`
count. Hover for full names; click a badge for references grouped by local
branches/bookmarks and remote. The header's reference icon opens all references,
even when badges are hidden. Select a reference to inspect its commit patch;
Copy copies its qualified name. Tracking information is available on hover.
The command palette and macOS View > References menu offer the same actions.

The filter icon controls each remote's history independently, with Show all and
Hide all. Hiding labels changes presentation only. History combines local roots
and enabled remote targets; shared ancestry remains visible once. Hiding an
inspected revision retains the open file and reading position. Use **Reveal
inspected revision** to temporarily include it again; changing source filters
clears that temporary reveal. Choosing a hidden reference reveals its commit.
Whole branch continues to use the comparison requested when the window opened.

```toml
[history]
show_reference_labels = true
reference_badge_width = 110

[history.remotes]
default_visible = true

[history.remotes.overrides]
origin = false
upstream = true
```

These defaults apply live. Toolbar choices are remembered per repository using
`layout.remember_per_repository`, independently of the settings file; **Reset
repository interface choices** restores configured defaults. New remotes inherit
`default_visible`; named overrides survive remote removal and re-addition.

Jaywalk reads local metadata only: it never fetches, imports Git references into
jj, or manages authentication. Run `jj git fetch` externally for jj repositories,
or `git fetch` for Git-only repositories. In colocated repositories a raw Git
fetch must be imported by jj externally before recorded jj history changes.
Reference refreshes preserve pinned reviews; newly discovered commits use their
source operation when opened. Imported commits without jj operation metadata
remain browseable and show unavailable operation/time information. Git reference
updates refresh the graph without
recapturing the displayed worktree. `[live]` controls this polling. Network
fetching and remote management remain future work.

## Checks

```sh
cargo fmt --check
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo test
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo clippy --all-targets -- -D warnings
```

Tests use synthetic temporary jj repositories and leave external checkouts unchanged.


## License

Jaywalk's original code is licensed under the GNU General Public License,
version 3 only (`GPL-3.0-only`). See [LICENSE](LICENSE) for the full terms.

Third-party code retains its respective licenses. In particular, the vendored
`gpui-component` code and its local patches remain under Apache-2.0; its license
is preserved in [vendor/gpui-component/LICENSE-APACHE](vendor/gpui-component/LICENSE-APACHE).
See [vendor/README.md](vendor/README.md) for dependency provenance and local changes.

## Panel layout defaults

`[layout]` controls default sidebar visibility, bottom-dock height and whether
interface choices are remembered per repository. `[layout.placement]` selects
`left` or `right` for `history_side` and `files_side`, with `sidebar_order` of
`history_first` or `files_first` in left-to-right order when they share a side.
The `change_evolution` and `file_evolution` placements accept `sidebar` or
`bottom`. Panel identity and review content stay independent of placement.

Interface choices are remembered per canonical repository checkout in
`~/.config/jaywalk/interface.toml`. Alternate `--config` profiles use a sibling
`*.interface.toml` file. Set `layout.remember_per_repository = false` to stop
reading and writing remembered choices; later opens start from TOML defaults.
Standalone file/directory comparisons do not persist temporary input paths.
The former shared `layout.toml` is left untouched and is not imported into
unrelated repositories. Configuration edits still apply live to their matching
controls. Different open comparisons of one repository share its last saved
interface choices; different repositories keep independent entries.

Panel-header menus, **View → Panel Placement**, and the command palette move
sidebars and timelines without changing the selected comparison. Bottom timelines
share the central workspace's bottom dock side by side. **Reset panel layout**
restores configured placement, visibility and sizes; **Reset repository interface
choices** also restores diff/file modes, wrapping and timeline filters. Separator
double-click resets only the adjacent panel (or the split) to its configured size.

`ui.row_height` supplies the compact 26-pixel minimum for history rows, flat file
rows and tree/directory rows. Optional `history.row_height`,
`files.list_row_height` and `files.tree_row_height` override individual views.
Larger panel fonts establish a readable minimum; wrapped file paths can grow.
Evolution cards keep their separate `evolution.row_height` setting. All row
settings apply live.

Panel surfaces, selection states and hairline separators use the active theme.
`ui.panel_header_height` and `ui.toolbar_height` set compact header geometry;
large fonts establish readable minimums. Placement controls are available from
each panel's ellipsis menu.

The full-window footer uses `ui.footer_height` (28 pixels),
`ui.footer_font_size` (12) and `ui.footer_control_height` (22). Its height grows
only when those configured metrics require it, never in response to status or
error text. Notices truncate in place and expose their full text on hover.
The word-wrap control remains immediately left of the three diff-view buttons.

### Adding syntax languages

GPUI Kit supplies the Tree-sitter grammars and highlight queries through
`gpui-component`; Jaywalk enables its complete `tree-sitter-languages` bundle.
Language detection is shared by Fluid and standard/inline views in `src/syntax.rs`.
It uses the provider's language names and aliases, with a small central table
for extra extensions and conventional filenames such as `Makefile`,
`GNUmakefile`, `CMakeLists.txt`, `Cargo.lock`, and shell startup files. Unknown
files remain plain text. Markdown code fences can use other bundled grammars.
Highlight coverage follows the provider's queries; a grammar alone does not
imply rich coloring for every language.

For another provider-supported extension, add its alias in that module if the
provider does not already recognize it. New bundled grammars become available
through the same bundle feature when the pinned provider is upgraded. A grammar
outside that bundle still needs a dependency and provider registration; custom
grammar loading and user-defined file associations are future work.

### Comparison file sources

File identity and optional change metadata are independent. Changed-file counts
and navigation use the actual comparison change list, while source reads use
explicit before/after paths. Recorded jj reads remain pinned to their operation;
evolution reads use the interdiff's synthesized objects. Git working-tree
comparisons capture changed contents when loaded and read unchanged files from
pinned base objects instead of mixing in later filesystem edits. External diff
inputs retain their supplied-path, read-on-selection behavior.

### Browsing the repository

Cmd+P searches the entire repository at the selected revision, including files
untouched by the branch. Its scope is independent of the file sidebar, collapsed
directories, and panel visibility. Change status appears separately and breaks
otherwise-equal fuzzy matches. Deleted files remain available from the before
side of the comparison.

In tree mode, use the Changed/All button to switch between changed files and the
full repository tree. `files.tree_scope = "changed"` is the default; `"all"`
shows unchanged files with subdued styling. The flat list stays focused on
changes. This preference reloads live and is remembered per repository when
`layout.remember_per_repository` is enabled; Reset interface uses TOML defaults.

Unchanged files open as one syntax-highlighted, read-only document with no change
markers. They use inline wrapping and preserve the preferred diff mode for the
next changed file. A file opened outside the sidebar filter appears temporarily
in italics; a genuinely absent retained file also shows the preview indicator.
Counts and next/previous-change navigation continue to describe actual changes.
Full-tree metadata is read in the background and shared by comparison identity.
Git working-tree unchanged files use pinned base objects, while changed files
retain captured bytes; later filesystem edits require refreshing the comparison.

On macOS, repository subprocesses preserve the app's inherited PATH and append
standard Homebrew (`/opt/homebrew/bin`, `/usr/local/bin`), Cargo
(`~/.cargo/bin`) and system tool directories when absent. This lets an app
started through Finder locate jj and Git when `jw` later reuses that instance.
The app does not run shell startup files or change its process-wide environment.
Custom tool locations must be present in the app's launch environment.
