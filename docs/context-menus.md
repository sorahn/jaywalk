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
