# Context menus

Right-click a history, files, change-evolution or file-evolution panel header for
its placement and visibility commands. The header’s overflow button opens the
same actions. Placement choices show the current selection.

These use the existing commands and active keybindings profile, including
remapped shortcuts shown by the menu. Panel placement/visibility follow the
per-repository interface-persistence setting.

## Files and folders

File-list and directory-tree rows offer Open file diff, Copy file name, Copy
relative path, Copy working-tree path, and Reveal working-tree location. Folder
rows also expand/collapse their entire subtree. Right-clicking preserves the
file currently displayed until an explicit Open action is chosen.

The files header/overflow offers list/tree mode, changed/all scope,
Reveal active file in sidebar, and Collapse/Expand all folders. Folding includes
hidden descendants and preserves expansion state across comparison updates.
Folder expansion commands apply in tree mode. Scope is used in tree mode;
Show all repository files includes unchanged files in the selected revision.

Filesystem actions refer to the local working tree, even when viewing an older
revision. Deleted/missing paths reveal their nearest existing ancestor. These
actions are disabled for external file/directory comparisons, where a single
working-tree location would be ambiguous. Historical contents are never written
out by these actions. File-opening and folder-changing actions check the original
comparison identity before running, so a stale menu cannot act on a new revision.

## History and evolution

History rows offer View commit changes (the usual parent comparison), Show change
evolution in jj repositories, Copy commit ID, Copy change ID (jj only), and Copy
commit message. Copy keeps the full IDs and full multiline message, independent
of the compact row label. Whole branch, references and refresh are also available.
The history header includes reference labels, remote visibility, and reveal-selection
controls in addition to placement and hiding.

Both evolution timelines offer the pressed version’s patch, changes since its
predecessor, full commit/change/operation IDs, and commit message. A folded row
uses the same representative version as normal clicking. Interdiff is disabled
for an original version without predecessors; copying an unknown operation ID is
disabled. Grouping and unchanged-version toggles affect that timeline.

A menu never substitutes another row that has moved into the same index: version
navigation checks the operation, file scope and commit ID. Clipboard actions
retain the original row’s metadata even if background refresh has changed selection.
The same commands can be remapped; keyboard copy commands use the selected
revision or active timeline version. Commands without a current subject do nothing.
