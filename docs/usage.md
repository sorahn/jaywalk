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
opens a new window. Ordinary opens follow `application.single_window_mode`;
reopening the same comparison in the reused window preserves its selection. File → Clear recent repositories clears saved recent history.

In a colocated checkout, Jaywalk prefers jj. Its default comparison uses the
recorded tip and its nearest immutable ancestor; ambiguous endpoints require an
explicit `--base` and `--tip`. Unsnapshotted working-file edits are not captured.

In a plain Git repository, the default compares HEAD with staged, unstaged, and
untracked worktree files. `--base REV --tip REV` compares recorded commits;
`--tip @` means the worktree. Contents are captured when a comparison loads, so
selecting another file does not mix in later edits. Automatic worktree refresh
installs a new coherent capture, retaining the open file and reading position.
Merge commits use the first
parent for their patch. Evolution controls require jj metadata.

With `[application] single_window_mode = true`, `jw`, normal
Open actions, and choosing a repository with Ctrl+R replace the current viewer,
even for another repository. CLI launches use the active or most recently used
viewer. The default is `false`, opening a new window for each request. The recent
picker's explicit Open in new window action/modifier always opens another window.

## Overview and files

A repository opens to the current-work overview. In plain Git, this compares the
working tree (including staged, unstaged and untracked changes) with the common
ancestor of HEAD and the locally known integration branch: a unique remote
default branch, otherwise a single local `main`/`master`/`trunk`. Tracking a feature
branch does not make that upstream the integration target. If the target or
common ancestor is missing or ambiguous, the caption explains the fallback to
HEAD → working tree; choose a branch/base explicitly to review another scope.
No fetch is performed. jj retains its mutable-stack default. It lists the commits in the
comparison and shows a unified diff for every changed file, with unchanged ranges
collapsed. Expand a range for context, use **Show full files** for everything,
or choose **Open file** for the regular diff viewer. Clicking a file header folds
that file; its Open file action remains separate. Sticky headers show which file
you are reading, with a corresponding reading indicator in the sidebar.

Plain Git repositories also have a virtual working-copy row above HEAD. Select
it to review current staged, unstaged, and untracked changes against HEAD. It
remains visible as “no changes” when clean. With `live.enabled = true` (the default),
saves and index changes refresh this view automatically after a short debounce.
Historical commit views stay pinned while their history graph updates.
This row has no commit ID, branch-comparison actions, or change evolution.

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

Git, jj, standalone, and evolution comparisons retain file kinds on both sides.
A symlink-to-file transition remains visible even when the link target and file
text match, with an explicit type-change notice. Deleted symlinks keep their
link icon and target text; deleted submodules retain their kind and show that a
text preview is unavailable. Empty-file additions and deletions remain distinct.

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
comparison's window to close or be replaced by another comparison; closing another
window does not release it. Use it when another tool supplies temporary paths. Quitting Jaywalk
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
# Deliver changed paths together in one directory callback:
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

Git recognizes recorded/staged renames with rename detection enabled. For an
unstaged move to an untracked path, Jaywalk also matches captured deleted/added
text without staging, writing the index or creating Git objects. Unique nonempty
exact matches are preferred. Small edited text files can match by shared line
bytes (at least 60% of the larger file), only when each side is the other's clear
best match and no competing candidate reaches 30%. Duplicates, splits, file-type
changes and ambiguous candidates stay separate. Unchanged source files are never
consumed to infer copies. Symlinks use their link text, never target contents.

Inference is deliberately bounded: at most 200 readable candidates per side and
64 MiB total candidate text for exact matching; approximate matching additionally
limits each file to 64 KiB, total text to 8 MiB, 20,000 candidate pairs, 65,536
source lines and one million distinct-line comparisons. If a pass exceeds its
budget, remaining additions/deletions stay intact. Binary, unavailable and
oversized text cannot be inferred; Git-reported renames remain visible. These
limits bound this matching pass, not total comparison memory. A detected rename
is one file entry with its original path and both captured content sides; later
edits appear on refresh.

In fluid view, drag the short, full-width thumb in the center gutter to scroll both panes
through the comparison. Its travel stays about 5% clear of each end so the thumb
remains discoverable, with three faint grabber ridges that brighten on hover/drag.
The up–down cursor marks the grabbable gutter. Track clicks follow
`panels.diff.shared_scrollbar_click`: `system` (default), `jump`, or `page`.
Paging moves one viewport toward the click; jumping moves to the clicked position.
System preferences are read at startup, with `jump` as the fallback. Dragging the
thumb still reaches the entire comparison; holding a page click does not repeat. Each pane’s own scrollbar remains
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

The native title bar identifies the repository or file comparison. One comparison
header holds back/forward navigation, the current comparison and its dropdown,
evolution review modes when applicable, and Overview/Refresh icon actions.
All changes, Current work and branch/base selection live in the comparison
dropdown. Find File lives in the Files header; when that panel is hidden its
search button appears in the comparison header. Cmd+P/Ctrl+P remains available
in either layout and searches the entire repository.

History, Files, Change evolution and File evolution share the same compact
panel-header typography, aligned edge spacing and grouped controls. Persistent
icon toggles use the application accent instead of a selected background.

Each open sidebar has a footer section matching its measured width, with its
visibility button at the outer-facing edge. Its count/status appears to the left
and its evolution toggle to the right within that section, on either side of the
window. Counts truncate to preserve the controls in narrow panels.
Hidden sidebars keep compact buttons; when the outside sidebar is closed, its
button packs beside the inner sidebar’s button. Footer sections follow panel
order, side, visibility and resizing. The comparison status belongs to the viewer
footer, beginning at its left edge after any compact closed-panel buttons.
File diff controls and the bottom-dock toggle sit at the viewer footer’s right
edge. The bottom toggle appears only when a timeline is placed in the real
bottom dock and controls only those docked timelines.

History and Files put their info in the app footer only while their sidebar is
open. History shows its revision count/status; Files shows changed
file counts, or total/changed counts in the all-files tree, plus a retained-preview
count when applicable. Each evolution toggle controls only its own timeline.

Hover a history row for its message, author, authored timestamp, full commit ID,
jj change ID when available, merge parent count and local/remote references.
The plain Git working-copy row describes uncommitted changes instead of showing
a commit identity. Hover details use the loaded snapshot and do not refresh or
record working files. Very long messages are clipped to keep the card compact;
commit details and copy actions remain available from the row context menu.

The single-file viewer uses the same compact header height and text sizing as
the panel headers. In Fluid view, the filename and Before/After (or interdiff
version) labels share one row, with pane-aligned gutter dividers and a solid
bottom border. Rename/type notices remain inline in that row.

At narrow viewer widths, the footer groups wrapping/view controls under a diff
display-options menu and uses a compact change counter. The full counter remains
available on hover. Counts stay on one line; drawer footer sections follow drawn
widths when the window constrains the sidebars.

## Selecting comparison endpoints

Click a history row to inspect that revision. Cmd-click another row on macOS,
Ctrl-click on Linux, to compare it with the inspected revision. From an overview,
the right endpoint is the anchor. Related points default to ancestor → descendant;
set `comparison.pair_order = "selection"` to retain click order instead.

The header shows the ordered pair. **Swap** reverses the captured comparison;
**L** and **R** reveal endpoints in history, including points outside the current
loaded graph. Inspecting a commit retains the aggregate pair: **All changes**
returns to it, **Current work** resets to workspace changes, and Back/Forward
restores captured views. Explicit `--base` / `--tip` CLI launches retain their
specified direction and initialize the same endpoint markers. **Diff from branch
point…** in a revision's context menu uses the existing branch/base picker;
this is distinct from an exact snapshot-to-snapshot comparison.

Standard Git difftool callbacks now copy their inputs before acknowledging them.
On macOS and Linux, Jaywalk attempts a bounded same-user parent-process lookup
to recover the effective Git request and invocation identity (including the Linux boot identity for persisted captures). Files from that
invocation accumulate in one window, even with single-window mode disabled.
Each callback returns after acknowledgement so Git can deliver the next file;
`--wait` waits for window closure only on the final callback. The progress caption
also makes partial delivery visible if Git is cancelled before the final file.
Temporary captures use the private runtime directory; a window holds a lease
until it closes. Captures left by application termination are reclaimed by later
difftool launches after one day, provided the producer has exited and no window
holds a lease. Owned copies use disk space proportional to the supplied inputs.
No personal alias or tracing configuration is required.

When resolved snapshots match every supplied path's contents, the header says
**matched snapshots** and shows L/R markers. This is a match for the supplied
subset, not proof that a mutable branch name could not have moved. The supplied
contents remain authoritative, including path-filtered comparisons. Unsupported
request syntax, index endpoints, binary/oversized content, moved refs and
unavailable process metadata retain the external diff without guessed commit
markers. Index comparisons do not create a fictitious history row. If process
identity is unavailable, inputs are owned independently rather than grouped by
repository or time. Detached wrappers must call Jaywalk before Git removes its
temporary inputs. Viewing never checks out revisions, edits the index or fetches.

## Hosting accounts (development preview)

Open **Accounts…** from the command palette or Welcome. Accounts and Welcome
open in the current main window's central panel. Back/Forward returns to the
retained comparison, file selection, folds and scroll position; sidebars hide
on these pages and restore their previous layout when you return. With no main
window open, an account command creates one. Tabs are not introduced by this
change; the page model can support opt-in tabs in the future.

Connect GitHub or Bitbucket in your browser; Jaywalk shows a waiting indicator
and an **Open browser again** action. No token or client secret is entered into
the app. **Cancel** or Escape on the waiting page cancels sign-in. Navigating to
another page keeps it running; closing its owning main window cancels it. Once
saving has begun, cancellation shows a finishing state instead. Escape from an
idle Accounts page returns to the previous page. Other repository windows share
the same account state.

**Reconnect account…** replaces a grant only after verifying the same provider
account. **Check account connection…** verifies saved access. **Disconnect
account…** asks before removing the local connection; it does not revoke access
on GitHub or Bitbucket. Choose **Bitbucket: Choose workspace…**, then an account,
to load the workspaces that account can access. The selection persists through
restart and token refresh; it is not fixed to the workspace owning the OAuth app.

This preview requires a development app registration. **Developer: Import hosting
setup…** explicitly copies registrations saved by the local authentication probe
into Jaywalk's OS credential store. It does not import account tokens, and does
not register a new provider app. After importing, return to **Accounts** to connect.
Public-release credential provisioning is still undecided. Credentials are never
saved in Jaywalk's settings or repository files. On macOS, use the signed app
bundle for stable Keychain access across rebuilds; see the development guide.


## Finding pull requests

**Open pull request…** continues inside the command palette from the command
list, a configured shortcut, or Welcome, even without a repository open. The
current view stays in place until you pick a PR; Escape or clicking outside
cancels without navigating. Search comes first, followed by two passive context
and status lines and the fuzzy results. The search field shows loading activity;
there are no filter or account-management buttons in this palette step.
Press **F5** or your configured **Refresh** shortcut to refresh without closing
it. Explicit refresh bypasses the 60-second list cache and respects account rate
limits. Previous results, filters, search and the selected PR remain in place
until replacement data arrives; a failed refresh keeps the previous results.
Press **Tab** to change scope, account or current-checkout repository, or refresh.
Each choice opens a searchable keyboard picker; selecting a value returns to PR
results with your original search intact. **Escape** backs out one filter step,
including when that submenu has a search query. **Tab** also returns directly to
PR results from any filter step. Scopes include My open PRs, Review requested,
Recent PRs and Current repository; account selection includes All accounts.
Selecting a PR opens its metadata in the central PR page. **Browse pull requests…**
opens that retained page directly for browsing. The picker starts with **My open
PRs** across connected accounts. The full browser also exposes scope and account filters.
**Review requests** uses GitHub's requested-review filter;
Bitbucket accounts explicitly report that this scope is unsupported. Current
repository remotes provide optional repository filters, without changing the
app's local comparison. Account buttons narrow discovery without silently
falling back to another account.

Type to fuzzy-match cached titles, repository names, source branches or numbers.
Typing does not issue network requests. A bare number matches cached candidates;
in an explicit current-repository scope it also offers a direct lookup there.
Paste a GitHub/Bitbucket PR URL, or enter `owner/repository#123`, to look up an
open or closed PR outside the cached list. Choose the provider/account explicitly
when multiple connected accounts could resolve it. Numbers are repository-scoped.
Only github.com and bitbucket.org are supported in this preview.

GitHub discovery uses GraphQL search to obtain source branches and stable PR/repo
IDs with each result, limited to three pages of 30 results per account/scope.
Bitbucket authored discovery visits up to ten accessible workspaces, prioritizing
the account's selected workspace, with at most three pages of 20 per workspace
and ten pages total per account. Partial results and their coverage are labelled;
use a repository scope or direct URL to narrow the request. Lists reflect the
user's and app registration's effective permissions, not every repository on the
provider. API search indexing may also lag recent changes.

Refresh runs off the UI thread, supports cancellation, and shares a 60-second
cache between windows. Reopening or changing filters within that interval can
reuse cached results; explicit Refresh bypasses it. Failed refreshes retain previous usable results with an error; rate
limits pause further requests for that account for at least 60 seconds. There
is no automatic polling. Account disconnect/replacement invalidates old results.

Select a result to inspect its title, author, source branch and state, open it
in the browser, or find matching current/recent local checkouts. Matching reads
Git remote configuration only; it does not fetch, checkout, snapshot or change
working files. Up to 50 current/recent paths are inspected, with unreadable and
truncated candidates reported. **Choose a local checkout…** can validate another
folder against the PR's provider/repository remote. Included Git config files,
SSH aliases and custom hosting instances are not inferred by this preview.

This slice provides discovery and metadata. **Open local repository** opens that
checkout normally; it does not claim the displayed comparison is the published
PR diff. Exact published comparisons and missing-object handling follow separately.

Recent PRs persist up to 50 canonical URLs in `recent-pull-requests.json` beside
the user config. Titles, tokens and account credentials are not stored there.
Recent entries use loaded metadata, including recovered snapshots, where
available; an unresolved entry prompts for an authenticated lookup. A local
repository is not required to discover a review or open it in the browser.

PR list metadata is also saved beside the config in `pull-request-cache.json`,
so restarting the app can show previous results while updating them. Saved
snapshots are labelled as such, recovered only for restored connected accounts,
and kept separate by account and scope. They contain PR titles, branches,
authors, states, canonical URLs and provider IDs, but no credentials or bodies.
The file is private to the local user and limited to 4 MiB and 16 account/scope
snapshots. Successful empty results replace old lists; failed refreshes do not.
