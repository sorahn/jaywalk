# Using Jaywalk

Jaywalk reviews a repository comparison or two standalone paths. It reads local
data without committing, fetching, or snapshotting the observed repository.

## Repositories and comparisons

```sh
jw
jw .
jw /path/to/repository --base REV --tip REV
jw . --file src/main.rs
```

Bare `jw` opens the welcome page, regardless of the terminal's current directory.
`jw .` opens the current repository. Ctrl+R searches all saved recent repositories;
Enter opens a result and Escape dismisses the picker. Command+Enter or Command-click
opens a new window. Ordinary reopening focuses the existing matching window while
preserving its selection. Different revision endpoints or config files can have
separate windows. File → Clear recent repositories clears saved recent history.

In a colocated checkout, Jaywalk prefers jj. Its default comparison uses the
recorded tip and its nearest immutable ancestor; ambiguous endpoints require an
explicit `--base` and `--tip`. Unsnapshotted working-file edits are not captured.

In a plain Git repository, the default compares HEAD with staged, unstaged, and
untracked worktree files. `--base REV --tip REV` compares recorded commits;
`--tip @` means the worktree. Contents are captured when a comparison loads, so
selecting another file does not mix in later edits. Merge commits use the first
parent for their patch. Evolution controls require jj metadata.

## Overview and files

A repository opens to the whole-branch overview. It lists the commits in the
comparison and shows a unified diff for every changed file, with unchanged ranges
collapsed. Expand a range for context, use **Show full files** for everything,
or choose **Open file** for the regular diff viewer. Clicking a file header folds
that file; its Open file action remains separate. Sticky headers show which file
you are reading, with a corresponding reading indicator in the sidebar.

Select a commit in history to see its message and patch. **All changes** returns
to the original branch comparison. **Overview** returns from an open file to the
overview of the current comparison. Back and Forward retrace views you visited
in this window, including comparison, file, and reading position where possible.

The file pane offers list and tree modes. Vertical indent guides trace ancestor
folders; open/closed folder icons replace disclosure carets, and clicking a folder
row toggles it. File-type icons share the bundled icon catalog and syntax filename
associations, with generic fallback for unknown types. Icons and guides are live
settings. The active list/tree and file-evolution header icons use the live
`ui.accent_color` theme accent instead of a selected background. When switching
to All files, known folder expansion is preserved and new directories start
collapsed; Changed files initially reveals its folders. Tree mode can show changed files or all
files in the selected revision. Cmd+P searches any file in that revision, including
unchanged files. The flat list stays focused on changed files. Opening an unchanged file shows one
read-only, syntax-highlighted document without change markers; it uses inline
wrapping and preserves the preferred diff mode for the next changed file. Deleted
files remain available from the comparison's before side. An opened file outside
the sidebar filter appears temporarily in italics. Git worktree comparisons read
unchanged files from pinned base objects; refresh to include later filesystem edits.

Changing commits or evolution versions keeps the open file and restores its
reading position when possible. If it is absent from the selected comparison,
the previous diff stays as a retained preview: an eye marker and italic file row
identify it. Choosing another file or Overview closes the preview. A later
comparison that changes the file replaces it with the new diff.

## Diff views and panels

The footer switches between inline, aligned side-by-side, and Fluid views.
Fluid keeps independent document heights and connects corresponding changes;
the attachment can slide along a large block as you scroll. The nearby wrap
control applies to the code views. With wrapping off, Shift+scroll or a horizontal
trackpad gesture scrolls both panes horizontally, each stopping at its own end.

Newly opened changed files reveal their first change. Returning to a file keeps
its reading position. Previous/next change controls navigate the diff; in Fluid,
a navigation arrow first reveals an active change that has scrolled out of view.
Syntax highlighting and word-level change emphasis complement the full-row shading.
Binary, non-UTF-8, or oversized inputs show an explanation when no text preview is available.

Resize panels with their separators; double-click to restore configured sizes.
Panel-header menus and View → Panel Placement move sidebars and evolution timelines.
Choices are remembered per repository by default. **Reset panel layout** restores
panel defaults; **Reset repository interface choices** also restores view modes,
wrapping, and timeline filters. See [settings](settings.md) for the full controls.

## Recorded jj evolution

Select a history entry and open **Evolution** to inspect versions of that logical
change. **Change evolution** describes the complete change; **File evolution**
follows the selected file. Their selections, scroll positions, grouping, and rebase
filters are independent.

**Version patch** compares a version against its first parent. **Since previous**
shows the predecessor interdiff while accounting for changed parents. The first
recorded version has no predecessor comparison. Content-free events are labeled
accordingly; unchanged rebases are hidden by default and can be shown with **Rebases**.
Current and selected versions remain visible.

Consecutive equivalent entries can fold into one row. Click the row to inspect
its newest version; use its chevron to expand individual events. **Group** toggles
this behavior. Branch boundaries, conflicts, and divergent versions stay separate.

Historical evolution reviews remain pinned. Select the change's history row to
resume following its latest version, or All changes to leave evolution review.
Interdiffs require Git-backed jj and Git on PATH; jj may synthesize backend objects
while calculating them, without updating the working copy or recording an operation.

## References and refresh

Reference badges identify local branches/bookmarks and remote references. Bordered,
blue branch badges mark local refs; green cloud badges mark remote-only refs and
retain the remote-qualified name. A local badge also shows a cloud when the commit
has visible remote refs. The +N count includes other visible refs at that commit;
hover summaries explicitly label each ref as Local or Remote. Icons distinguish
the kinds without relying only on color. Hover
for full names, click a badge for its references, or use the history header's
reference control to browse all references. Remote history and label visibility
can be hidden independently; hiding labels does not remove history.

Jaywalk polls recorded jj operations and local reference metadata. It does not
fetch or import references: run `jj git fetch` or `git fetch` yourself. In a
colocated repository, raw Git changes must be imported by jj before recorded jj
history reflects them. Git worktree contents and standalone comparisons refresh
manually.

Ordinary jj history selection follows the logical change through rewrites.
With `--tip @`, refresh follows the recorded working copy even when it is replaced;
use `--tip BOOKMARK` to follow a feature independently of the checkout. Explicit
commit IDs remain fixed. Missing or divergent selections retain the previous view
with an explanation. Refresh preserves a coherent previous view until replacement
data is ready. Cmd+R refreshes; historical evolution reviews stay pinned.

## Standalone files and directories

```sh
jw diff before.txt after.txt
jw diff /path/to/before /path/to/after
jw --wait diff before.txt after.txt
```

Standalone comparisons do not have repository history or evolution. Directory
paths match literally, so renames appear as additions and deletions. Symlink targets
are compared without traversing directory symlinks. Use Refresh to reread inputs.

Normal launches return the shell prompt immediately. `--wait` waits for the requested
window to close, including when reusing a window; closing another window does not
release it. Use it when another tool supplies temporary paths. Quitting Jaywalk
also releases waiting clients.

## Use with jj diff

Add this optional tool to your jj config:

```toml
[merge-tools.jaywalk]
program = "jw"
diff-args = ["--wait", "diff", "$left", "$right"]
diff-invocation-mode = "dir"
```

Then run `jj diff --tool jaywalk` with the usual revision or path filters.
`--wait` keeps jj's temporary input trees alive until the window closes.
This integration is for viewing diffs; Jaywalk does not edit patches or resolve merges.

## Use with git difftool

```sh
git config --global difftool.jaywalk.cmd 'jw --wait diff "$LOCAL" "$REMOTE"'
git config --global difftool.jaywalk.trustExitCode true
git difftool --no-prompt --tool=jaywalk
# Compare a whole tree in one window:
git difftool --no-prompt --dir-diff --no-symlinks --tool=jaywalk
```

Git's usual `--cached` and revision arguments work here. This registers an optional
tool without changing the default viewer. Keep `--wait` for temporary inputs.

## Current work and branch reviews

Current work returns to the latest combined workspace view. All changes returns
to the full review scope, while Overview shows the selected comparison as a
stacked diff. Choose another branch/base from the dropdown beside All changes,
or use the history/reference context menus. The caption shows which source and
base you are reviewing. See [context menus](context-menus.md#branch-review-context)
for ambiguity, merged branches and live/pinned behavior.

In fluid view, drag the short, full-width thumb in the center gutter to scroll both panes
through the comparison. The up–down cursor marks the grabbable gutter; clicking above or
below the thumb moves to that position. Each pane’s own scrollbar remains
independent. Mouse-wheel scrolling over either pane or the gutter stays linked.

Jaywalk follows system appearance by default. Set `ui.appearance` to `"light"`,
`"dark"` or `"system"`; changes apply live across all windows, including source
highlighting. Configure the application highlight color with a shared hex string
or separate light/dark values:

```toml
[ui]
appearance = "system"
accent_color = { light = "#0066b8", dark = "#3794ff" }
```

The accent updates active icons, standard control fills, focus and selection.
Icon-only toggles show their active color without a persistent filled background.
Repository status, diff and graph colors keep their separate meanings with
readable light/dark palettes. See [settings](settings.md#ui).

Customize diff marks/connectors, row-background hues and word highlights under
`[diff.colors]`. Like the accent, each color accepts one shared hex value or a
light/dark pair. Edits recolor open viewers and overviews live; source syntax and
file-list status colors keep their own palettes. See [diff colors](settings.md#diffcolors).
