# Changelog

User-visible changes are recorded here. Version 0.1.1 is planned for the current
branch after it merges; it has not been released yet.

## Unreleased

Planned version: **0.1.1**.

### Added

- `jw` terminal launcher, standalone file/directory comparisons, and optional
  Git and jj diff-tool integrations with per-window waiting.
- Plain Git repository history, commit comparisons, and working-tree diffs.
- Native macOS menus, an app bundle, and a layered app icon with dark variants.
- Configurable shortcuts, separate viewer/sidebar typography, ligatures,
  tab width, line numbers, and default file-list/tree selection.
- Configurable editor for opening settings with Cmd+,.
- CLI version reporting and an About window with copyable build information.

### Changed

- Keep the open file, diff view, and source-line reading position across
  comparison changes; retain absent files as italic temporary previews.

- TOML settings apply live, retaining the last valid configuration on errors.
- Terminal launches share one app process and focus an already-open repository;
  other repositories open additional windows in that process.

### Fixed

- Follow replacement working-copy changes with the default `@` target, pause
  retries of unchanged failed operations, and keep notices in the fixed footer.

- Preserve overview and file-view backgrounds and filename headers across loading and error states;
  delay transient History loading text consistently with the other panels.

- Mark retained previews with an inline icon and keep their sidebar entries in
  normal sorted order without adding a header row.

- Keep the evolution toolbar and changed-file list stable during commit changes;
  suppress transient loading indicators with a configurable delay, and retain
  the evolution label, controls and timeline rows until replacement metadata is ready.
  Reuse open editors across comparisons and restore changed-text positions before
  painting; prepare syntax off-thread to avoid a second highlighting flash.

- Clearly identify the working-copy change and whether it contains changes.
- Preserve reading position when fonts, wrapping, and diff views change.
- Preserve text selection and copy bindings when shortcuts reload.
- Open the Git overview without requiring jj operation metadata.
- Refresh the Dock icon after rebuilding an existing app bundle.
- Rebuild the macOS bundle without overwriting a running executable.

## 0.1.0 — initial prototype

### Added

- Native jj history and change/file evolution browsing.
- Inline, standard side-by-side, and fluid comparisons with syntax highlighting.
- Resizable panels, file navigation, TOML configuration, and recorded-change refresh.
