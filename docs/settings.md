# Settings reference

Keyboard defaults include Linux-specific recent-repository and fullscreen bindings;
see the [keybindings guide](keybindings.md). Sparse profiles inherit the active
platform's defaults before applying their explicit overrides.

Jaywalk reads TOML settings. Keep only your overrides in your user file;
omitted values inherit the shipped [default configuration](../config/default.toml).
The schema and validation live in [src/settings.rs](../src/settings.rs).

## Configuration file and reload

- Default: `~/.config/jaywalk/config.toml`.
- With a nonempty `XDG_CONFIG_HOME`: `$XDG_CONFIG_HOME/jaywalk/config.toml`.
- `jw . --config /path/to/config.toml` selects a separate file, which must exist.
- Cmd+, or **Open settings** opens the active file. A missing default file is
  created for you; `settings.editor` chooses the app or command that opens it.
  First use also creates a sparse `keybindings/default.toml` profile; existing files
  are never overwritten. An explicit `--config` must exist and uses profiles beside it.
- Tables merge recursively with defaults. Arrays replace that value completely.
  Keyboard overrides belong in the selected profile, rather than this config.
- Optional values have no TOML `null`: remove the key to restore its inherited
  behavior. Commented font/row-height examples in the template are not defaults.

All current settings apply live; none requires an app restart. The settings
watcher is independent of repository refresh. One application-owned watcher serves
all windows using the same config source, including while those windows are closed. It polls at
`settings.poll_interval_ms` and requires two matching reads, so the default
interval usually applies a saved change in about one second. Creation, deletion
and atomic file replacement are detected. Invalid TOML, unknown keys, invalid
values and unreadable explicit files retain the last valid configuration and
show an error. Fixing the file clears the error. Deleting the optional default
file restores defaults; deleting an explicit `--config` file reports an error.

Changes affect the relevant UI immediately, or the next operation that uses
that setting: opening settings uses the new editor, a refresh uses its updated
interval, and the next window close uses the current application quit policy.

## Defaults and remembered interface state

Toolbar choices and manual resizing do not rewrite your configuration. With
`layout.remember_per_repository = true`, Jaywalk stores repository interface
choices separately in `interface.toml` next to the default config. An explicit
`--config /path/custom.toml` uses `/path/custom.interface.toml`. Repositories
are keyed by canonical root; external path-to-path comparisons do not restore
repository state. This file is managed by Jaywalk, not a second settings file.

Remembered choices can override startup defaults, including panel visibility,
placement, dimensions, file-list/tree choices and reference/remote visibility.
**Reset repository interface choices** restores current TOML defaults. Disabling
remembering stops reading/writing repository choices; it does not erase their
saved records. **Reset panel layout** resets sizes/placement rather than every
interface preference. Double-click a separator to reset its panel to the
configured default; the before/after split resets to even widths.

Only settings that actually change override current session choices during
reload. Editing a font preserves the current view and wrapping. Editing a
view/wrap/visibility/size default applies that new value; size limits clamp the
layout. Code zoom commands affect the viewer, not sidebar font sizes. Live
changes preserve the selected comparison/file and reading position where
possible. Reducing the evolution limit retains selected versions even if they
fall outside the newly loaded range.

Sizes and spacing below are **logical pixels**, not physical display pixels.
Intervals are milliseconds. Numeric ranges are inclusive; dimensions must be
finite. Row/header/footer heights are minimums: larger fonts and controls can
require more space. Booleans are TOML `true`/`false`, without quotes.

## Small configuration example

```toml
[application]
quit_on_last_window = false

[editor]
font_family = "Menlo"
font_size = 14
ligatures = false

[files]
default_view = "tree"
tree_scope = "all"

[history.remotes.overrides]
origin = false

[keyboard]
profile = "keybindings/default.toml"
additional_directories = ["../shared/keys"]
```

Use an installed font's actual family name. Application controls, source code,
history and file-browser fonts are independently configurable. Ligature settings
control OpenType `liga`, `clig` and `calt`; the selected font must support them.

## Reference

Each table uses full dotted keys. In TOML, put the short key below the matching
section header, for example `[editor]` followed by `font_size = 14`.

### `application`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `application.quit_on_last_window` | `true` | Quit when the final actual window closes, including About. `false` keeps the app available with no windows. This is application-wide: opening another config or editing this value updates the shared policy; CLI waiters are notified before shutdown. |
| `application.single_window_mode` | `false` | Normal opens from `jw`, Ctrl+R/recent repositories, welcome, and Open actions replace the originating viewer, or the active/most recently used viewer for a CLI launch, even for a different repository, range, or config. `false` opens a new window on each normal open. Explicit Open in new window and the recent-picker modifier always create a new window. Applies on the next open; uses the application-wide last-applied config-source policy. |

### `picker`

| Setting | Default | Behavior |
|---|---|---|
| `picker.adaptive_command_order` | `true` | Window-local preference controlling command usage learning and ordering. With an empty command query, frequent/recent commands come first; textual relevance takes priority for typed queries. Edits apply live. The usage snapshot stays fixed during an opening so activity in another window does not reorder the selected results. A new opening reads the latest shared history. `false` restores deterministic textual/alphabetical ordering and stops recording commands in that viewer; existing saved history remains available if re-enabled. |

Usage is shared across application windows and saved beside the default user configuration in `command-usage.json`, independently of an explicitly selected config. It stores only stable command IDs, bounded frequency/recency counters (up to 256 entries, 64 KiB on read), and no query text, command arguments or credentials. Writes run off the UI thread and flush at app exit. **Clear command usage history** resets it. An unreadable history is preserved until explicitly cleared; the command picker reports read/write errors.

### `welcome`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `welcome.recent_count` | `10` | Number of recent repositories shown on the welcome page; 1–50. Applies live. This changes only the preview count, not the saved repository history. |

### `navigation`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `navigation.history_limit` | `100` | Maximum in-memory visited views per window, including the current view; 1–1000. Changes apply live, retaining the current location while trimming old or forward entries. History is not persisted across launches. Snapshots retain recorded comparisons for Back/Forward. |

### `window`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `window.width` | `1500` | Window width; 320–10000. A saved edit resizes open viewers. |
| `window.height` | `950` | Window height; 240–10000. A saved edit resizes open viewers. |

### `ui`

Appearance and accent share the existing application-wide, last-successfully-applied config-source policy: opening another config or successfully editing its UI settings updates the shared theme for every window. It is not a per-repository interface override.

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `ui.appearance` | `"system"` | `"light"`, `"dark"` or `"system"`. Updates all windows, editor syntax and diff palettes live. System follows OS appearance changes; explicit modes override this app’s native window appearance where supported, without changing system preferences. |
| `ui.accent_color` | `"system"` | Application-wide accent: `"system"`, a shared `#RRGGBB` string or a light/dark pair (see below), case-insensitive hex. On macOS, system reads AppKit’s user accent preference for the app’s selected light/dark appearance and follows system-color notifications. Explicit hex/pair values override the system. Updates active icons, primary controls (including checkboxes/switches), focus rings, links and selection tints live in all windows; filled controls choose a light/dark foreground. Git statuses, diff colors and graph lanes stay semantic. Invalid edits retain the last valid config. |
| `ui.accent_color_fallback` | `{ light = "#0066b8", dark = "#3794ff" }` | Color used by `"system"` when native accent lookup is unavailable, including unsupported platforms. Accepts one shared hex color or a light/dark pair and applies live. |
| `ui.accent_floating_outlines` | `true` | Use the resolved accent for palette, popover, menu and tooltip edges. Applies live; `false` restores neutral theme edges. Does not change layout dimensions. |
| `ui.loading_delay_ms` | `200` | Delay before showing loading feedback; 0–5000. `0` shows it immediately; fast operations never expose a loading label. |
| `ui.row_height` | `26` | Shared minimum for history, tree and list rows; 12–160. Per-view overrides below take precedence; fonts establish a readable minimum. |
| `ui.panel_header_height` | `30` | Minimum panel-header height; 24–120. |
| `ui.toolbar_height` | `28` | Minimum review-control height within the single comparison header; 28–120. The header fits the greater of this value, `ui.comparison_bar_height`, and its font minimum. No separate application-toolbar row. Applies live. |
| `ui.footer_height` | `28` | Minimum application footer height; 20–120. Also sets the minimum History/Files footer height, which grows to fit their own font and evolution control. Effective height also fits its control height and at least 1.5 × footer font size. |
| `ui.footer_font_size` | `12` | Footer label font size; 8–48. |
| `ui.footer_control_height` | `22` | Footer control height; 16–80. |
| `ui.comparison_bar_height` | `30` | Reserved comparison/evolution toolbar height, including compact back/forward controls; 24–120. Grows to fit the UI font. |
| `ui.font_size` | `16` | Application control font size; 8–48. Does not replace independent source/sidebar sizes. |
| `ui.ligatures` | `true` | Enable ligatures and contextual alternates for application text. |
| `ui.font_family` | omitted | Installed family name; defaults to the theme/system UI font. Must be nonempty when supplied. |

All configurable colors accept the same hex formats; `ui.accent_color` additionally accepts `"system"`. A hex string applies in both modes;
a table selects a color for the active appearance. Omitted table members inherit
their shipped defaults. These examples are alternatives:

```toml
[ui]
appearance = "system"
accent_color = "system"
accent_color_fallback = { light = "#0066b8", dark = "#3794ff" }
```

```toml
[ui]
accent_color = "#8b5cf6"
```

Configurable colors include `ui.accent_color`, `ui.accent_color_fallback` and every entry in `diff.colors`. Opacity settings remain
numbers and apply to their theme-derived colors. Invalid colors or appearance
values retain the last valid settings and report a configuration error.

### `editor`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `editor.font_size` | `13` | Source viewer font size; 8–48. Zoom commands change it for the session without rewriting TOML. |
| `editor.line_numbers` | `true` | Show source line numbers in the file viewer and overview. |
| `editor.tab_size` | `4` | Tab-stop width in columns; integer 1–16. Re-prepares displayed source without changing its text. |
| `editor.ligatures` | `true` | Enable source-code ligatures and contextual alternates. |
| `editor.font_family` | omitted | Installed family name for source text; defaults to the theme/platform monospace font. Must be nonempty when supplied. |

### `panels.history`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `panels.history.default_width` | `280` | Sidebar width and separator reset target; must lie between min_width and max_width. |
| `panels.history.min_width` | `180` | Minimum sidebar width; 32–4000. The layout may shrink below this when the window cannot fit all panels. |
| `panels.history.max_width` | `700` | Maximum sidebar width; min_width–8000. |

### `panels.history.font`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `panels.history.font.size` | `14` | Independent sidebar font size; 8–48. Also styles its associated evolution timeline. |
| `panels.history.font.ligatures` | `true` | Enable sidebar ligatures and contextual alternates. |
| `panels.history.font.family` | omitted | Installed family name; inherits the application UI font when omitted. Must be nonempty when supplied. |

### `panels.files`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `panels.files.default_width` | `260` | Sidebar width and separator reset target; must lie between min_width and max_width. |
| `panels.files.min_width` | `200` | Minimum sidebar width; 32–4000. The layout may shrink below this when the window cannot fit all panels. |
| `panels.files.max_width` | `700` | Maximum sidebar width; min_width–8000. |

### `panels.files.font`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `panels.files.font.size` | `14` | Independent sidebar font size; 8–48. Also styles its associated evolution timeline. |
| `panels.files.font.ligatures` | `true` | Enable sidebar ligatures and contextual alternates. |
| `panels.files.font.family` | omitted | Installed family name; inherits the application UI font when omitted. Must be nonempty when supplied. |

### `panels.diff`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `panels.diff.shared_scrollbar_height` | `12` | Fixed shared thumb height in pixels; 1–1000, clipped to the track. Spans the fluid gutter width, with inset travel and faint grabber ridges. Applies live. |
| `panels.diff.shared_scrollbar_opacity` | `0.12` | Shared thumb opacity; 0–1, brighter on hover/drag. Zero hides the thumb while keeping the gutter interactive. Applies live. |
| `panels.diff.shared_scrollbar_click` | `"system"` | Track clicks: `system`, `jump`, or `page`. `page` moves one viewport toward the click; `jump` centers the thumb at the click. `system` uses the desktop preference captured at startup, falling back to `jump` if unavailable. Overrides apply live; restart to reread OS preferences. Thumb dragging always covers the full scroll range. |
| `panels.diff.change_map_opacity` | `0.24` | Whole-comparison change marks in the Fluid gutter; 0–1, zero hides them. Left marks show before-side changes; right marks show after-side changes. Uses the adaptive diff palette, brightens slightly while dragging, and applies live. Marks share the thumb's scroll coordinate; they have no hover UI or separate click behavior. |
| `panels.diff.gutter_width` | `48` | Fluid connector-gutter width; 8–240. |
| `panels.diff.min_code_width` | `100` | Minimum width of each Fluid code pane; 32–2000. The initial before/after split is even. |

The shared Fluid scrollbar reads macOS `AppleScrollerPagingBehavior` through
CoreFoundation. On KDE it reads `[KDE] ScrollbarLeftClickNavigatesByPage` from
`kdeglobals`; other Linux desktops use `gtk-primary-button-warps-slider` in
`gtk-4.0/settings.ini`, then `gtk-3.0/settings.ini`. Linux file lookup follows
user-first XDG configuration directories, then the `/etc` GTK defaults. Missing, unreadable or malformed
preferences use `jump`; session-only toolkit overrides are not queried.

### `history`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `history.page_size` | `5000` | Target initial graph rows and rows per Load older action; 1–50000. Includes Git's virtual working-copy row on the first page. Applies live by starting a fresh history snapshot; already loaded pages remain visible until replacement succeeds. |
| `history.initial_batch_size` | `100` | First rows published when opening a local repository; 1–50000, clamped to `page_size`. The rest of the initial page fills in the background. Changes apply on the next repository open; they do not discard already loaded rows. |
| `history.page_on_scroll` | `false` | Automatically load the next page when its footer row becomes visible. Applies live. Otherwise use Load older history or the graph's bottom row. Errors pause automatic loading until an explicit retry. |
| `history.search_page_size` | `100` | Matches per `/` history-search request; 1–1000. Applies to the next query/page request. Searches older commits beyond the graph's loaded pages; hosted reviews search their available acquired commits. |
| `history.stripe_opacity` | `0.08` | Alternate-row text-tint opacity; 0–0.2. `0` disables zebra stripes. |
| `history.lane_spacing` | `8` | Distance between graph lanes; 4–48. The graph column adapts to visible lane sprawl. |
| `history.show_reference_labels` | `true` | Show compact bookmark/branch badges. Does not hide history from any remote. |
| `history.reference_badge_width` | `110` | Maximum compact reference-badge width; 40–300. Full names remain available in reference details. |
| `history.row_height` | omitted | Override ui.row_height for graph rows; 12–200. Font size can raise the effective minimum. |

On opening a local repository, the viewer publishes `initial_batch_size` rows
(100 by default, or fewer when `page_size` is smaller) first, then
fills the configured initial page in the background even when `page_on_scroll`
is off. The first rows remain usable during filling; cancellation/source changes
invalidate the fill and errors retain those rows for retry. Source discovery and
reference capture still precede the first batch; this is not a zero-latency
startup guarantee. Later refreshes retain the old graph while preparing its
replacement.

History pages share a captured reference/root snapshot; jj reads stay at one explicit
operation and never snapshot the working copy. Loading more retains the current
comparison and reading position. Refresh, a remote-source change, or **Show latest
history** starts a fresh snapshot. Search result selection reveals an older commit
with a bounded ancestor context; the upward-arrow header action returns to latest
history without changing the inspected diff. Loading further pages retains more
metadata in memory; page size controls each request, not the eventual total.

Start with the default 5,000; 10,000 is a suggested upper starting point for
larger repositories. The allowed maximum of 50,000 is an explicit override,
not a recommended default. Cost depends on graph fanout as well as row count:
a branch-heavy 92,000-commit repository's backend harness peaked around 27 MiB
at 5,000 rows, 47 MiB at 10,000, 106 MiB at 20,000 and 874 MiB at 50,000.
These figures exclude full application/rendering memory and Git child processes.
Lower the page size if opening or refreshing history feels slow. Search can find
older commits without loading their entire graph first. Existing explicit
`page_size` values stay in effect; remove the override to follow the new default.

### `history.remotes`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `history.remotes.default_visible` | `true` | Include history contributed by remotes without an explicit override, including newly discovered remotes. Labels and history membership are independent. |
| `history.remotes.overrides` | `{}` | Map of remote names to boolean visibility overrides; use quoted keys for names containing dots. |

Each entry in `[history.remotes.overrides]` is a boolean named after a remote.
The map is empty by default. `true` includes that remote's locally available
history; `false` excludes history contributed only by it. Shared local/visible
ancestry remains present. Overrides survive remote removal/re-addition.
This does not fetch from remotes or change whether reference labels appear.

```toml
[history.remotes]
default_visible = false

[history.remotes.overrides]
origin = true
"another.remote" = false
```

### `overview`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `overview.context_lines` | `3` | Unchanged context around overview edits; integer 0–100. Range disclosures reveal more; Show full files temporarily unfolds everything. |
| `overview.expansion_lines` | `20` | Lines revealed by each directional context-expansion action; integer 1–1000. |
| `overview.message_max_height` | `220` | Maximum height of the scrollable message area when there are no changed files; 80–1200. With files, the full message scrolls with the overview and folds its subject into the existing comparison bar while scrolling. |

### `files`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `files.default_view` | `"list"` | `"list"` or `"tree"`. Both use directory-first order; list mode omits directory rows. |
| `files.tree_scope` | `"changed"` | `"changed"` or `"all"`. Filters tree mode only. `all` includes unchanged files in the selected comparison/revision; Cmd+P searches the entire repository regardless of this filter. |
| `files.indent_guides` | `true` | Show vertical ancestor guides in the directory tree. Applies live. |
| `files.indent_guide_offset` | `-2` | Horizontal guide offset from ancestor icon centers in pixels; −48–48. Negative moves left. Applies live. |
| `files.indent_guide_opacity` | `0.12` | Guide opacity using the theme foreground; 0–1. Applies live. |
| `files.file_icons` | `true` | File-type glyphs in list/tree views; unknown types use a generic file. Folder icons always indicate open/closed state. Applies live. |
| `files.indent_width` | `12` | Directory-tree indentation per level; 8–48. |
| `files.compact_paths` | `true` | Condense directory components in unwrapped flat-list labels as space runs out; filenames remain intact. |
| `files.list_word_wrap` | `false` | Wrap flat-list paths instead of scrolling them horizontally. Tree rows always remain single-line. |
| `files.tree_row_height` | omitted | Override ui.row_height for tree rows; 12–200. Font size can raise the effective minimum. |
| `files.list_row_height` | omitted | Override ui.row_height for flat-list rows; 12–200. Font size can raise the effective minimum. |

### `diff`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `diff.default_view` | `"fluid"` | `"inline"`, `"side_by_side"` or `"fluid"` for the file viewer. Overview always uses unified/inline diff. Automatic single-column presentation for unchanged/conflicted files does not replace the saved preference. |
| `diff.collapse_unchanged` | `false` | Fold unchanged ranges in single-file inline and side-by-side views. Applies live; the footer icon beside Word wrap or remappable toggle overrides it, remembered per repository when layout persistence is enabled. Fluid always shows full files. |
| `diff.context_lines` | `3` | Context on each side of an edit in folded standard views; integer 0–100. Applies live. Independent of overview context. |
| `diff.expansion_lines` | `20` | Lines revealed by each partial range disclosure in standard views; integer 1–1000. Applies live. |
| `diff.max_text_file_size_mb` | `5` | Maximum text bytes per before/after side in MiB (1,048,576 bytes). A positive integer whose byte count plus one fits the platform address space. Applies on the next comparison load or refresh. |

Oversized files stay in the file browser and overview with a byte-size notice.
Reads are bounded before decoding, diffing, and syntax highlighting; external
file discovery compares bytes in small chunks. This is a per-side text limit,
not an overall memory budget: an overview can retain many files, and parsed
text/diffs require additional memory. Image-preview limits are separate future work.

External file/directory comparisons capture their readable contents and file
metadata when opened. Clones, Swap and Back/Forward reuse that capture even if
the originals are edited or deleted. Contents within the text limit are streamed
into a private temporary file; oversized entries retain their captured size and
an unavailable-preview notice. Oversized equality comparisons stream bytes
without loading or storing the entire file. Temporary storage is released when
the last comparison owning it is dropped. Source changes detected during capture
fail the open with a request to reopen; this is not an atomic filesystem snapshot.
The limit is per side/file, not an aggregate temporary-disk budget.

The default text-file limit is **5 MiB per before/after side**. Files above it
remain listed with a size-limit notice. To inspect larger text files, raise
`diff.max_text_file_size_mb` in your config and reload or refresh the comparison:

```toml
[diff]
max_text_file_size_mb = 20
```

Raising the limit can substantially increase memory use and loading time.
The limit bounds source bytes per side, not total application memory: a diff
can retain both sides, syntax trees, layout data and other representations, and
an overview can hold multiple files. A source file near 50 MiB may require
several GiB of memory. Explicit user overrides are preserved; the default change
applies when this setting is omitted.

### `diff.colors`

Application-wide palette, live in every window, using the same config-source
policy as `ui.appearance`. Every entry accepts a shared `"#RRGGBB"` or
`{ light = "#RRGGBB", dark = "#RRGGBB" }`; omitted mode members inherit shipped
defaults. Colors are case-insensitive. Invalid edits keep the previous palette.
These settings do not change application accent, syntax foregrounds, or file-list
Git status colors. They apply to the file viewer and overview without replacing
editor state or reloading repository data.

| Setting | Light default | Dark default | Use |
| --- | --- | --- | --- |
| `diff.colors.added` | `"#22863a"` | `"#4ec96e"` | Added line signs/bars, Fluid addition connectors and empty-side markers; current-side conflict tint. |
| `diff.colors.removed` | `"#cf222e"` | `"#f85149"` | Removed line signs/bars, Fluid deletion connectors and empty-side markers. |
| `diff.colors.modified` | `"#0969da"` | `"#3794ff"` | Fluid replacement connectors/markers; incoming-side conflict tint. Inline and standard split views represent replacements as removed/added lines. |
| `diff.colors.added_background` | `"#22863a"` | `"#4ec96e"` | Added-row background hue. |
| `diff.colors.removed_background` | `"#cf222e"` | `"#f85149"` | Removed-row background hue. |
| `diff.colors.modified_background` | `"#0969da"` | `"#3794ff"` | Fluid replacement-row background hue. |
| `diff.colors.word_added` | `"#22863a"` | `"#4ec96e"` | Added-word background hue. |
| `diff.colors.word_removed` | `"#cf222e"` | `"#f85149"` | Removed-word background hue. |
| `diff.colors.conflict_base` | `"#57606a"` | `"#a4b4c2"` | Conflict ancestor/base tint in the standard diff widget. |

The viewer applies translucent emphasis: row backgrounds are subdued, changed
words stronger, and active Fluid changes brighter. Hue overrides retain that
emphasis and the one-pixel connector width. Text selection follows the application
accent. Context rows, separators and hover surfaces follow the light/dark theme.

```toml
[diff.colors]
# Same color in light and dark:
added = "#32a852"
# Mode-specific colors; all other entries inherit defaults:
removed = { light = "#b91c1c", dark = "#fb7185" }
```

### `diff.before`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `diff.before.word_wrap` | `false` | Wrap the before pane in side-by-side and Fluid views. |

### `diff.after`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `diff.after.word_wrap` | `false` | Wrap the after pane in side-by-side and Fluid views. |

### `diff.inline`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `diff.inline.word_wrap` | `false` | Wrap inline/unified source, including overview and automatic single-column file presentation. |

### `evolution`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `evolution.visible` | `false` | Default visibility of Change evolution. |
| `evolution.show_unchanged_rebases` | `false` | Include rebases with unchanged patches. Current and selected versions remain visible even when the filter is off. Shared with File evolution. |
| `evolution.collapse_repeats` | `true` | Group consecutive matching evolution entries. Shared default for both timelines; runtime grouping/expansion choices are independent. |
| `evolution.default_height` | `320` | Change-evolution height/reset target; must lie between its min/max height. |
| `evolution.min_height` | `180` | Minimum Change-evolution height; 100–2000. |
| `evolution.max_height` | `700` | Maximum Change-evolution height; min_height–4000. |
| `evolution.min_history_height` | `120` | Minimum space retained for history above a sidebar timeline; 80–2000. |
| `evolution.row_height` | `48` | Timeline row height for both evolution views; 24–160. |
| `evolution.limit` | `100` | Maximum loaded recorded versions; integer 1–1000. Both timelines share recorded data; selected versions are retained when reducing the limit. |

### `file_evolution`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `file_evolution.visible` | `true` | Default visibility of File evolution; independent of Change evolution. |
| `file_evolution.default_height` | `320` | File-evolution height/reset target; must lie between its min/max height. |
| `file_evolution.min_height` | `180` | Minimum File-evolution height; 100–2000. |
| `file_evolution.max_height` | `700` | Maximum File-evolution height; min_height–4000. |
| `file_evolution.min_files_height` | `120` | Minimum space retained for the file browser above a sidebar timeline; 80–2000. |

### `live`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `live.enabled` | `true` | Automatically follow plain Git worktree/index changes and recorded jj operations. Git watchers are shared per repository and debounce saves for 250 ms; ignored build trees are excluded from directory registration. Working-copy views update in place; commit/evolution inspection stays pinned while history updates. Watcher failures/missed events fall back to read-only metadata scans. Does not fetch, import into jj, or snapshot jj working files. Applies live, independently of settings reload. |
| `live.poll_interval_ms` | `1500` | Git fallback metadata-scan/retry interval and jj recorded-operation polling interval; integer 250–60000. Native Git events can trigger earlier refresh. Applies live. |

### `settings`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `settings.poll_interval_ms` | `500` | Configuration-file polling interval; integer 100–60000. Two stable reads settle editor saves. Independent of `live.poll_interval_ms`. |
| `settings.editor` | omitted | App name/path string on macOS, executable string elsewhere, or a nonempty array of executable + literal arguments. Omitted uses the platform default editor; see below. |

To choose the app that opens Cmd+,:

```toml
[settings]
editor = "Zed" # macOS app name, or an absolute .app path
# Alternatively, on any supported platform:
# editor = ["code", "--reuse-window"]
```

Arrays execute directly, without a shell, with the config filename appended as
one argument. Spaces in paths need no shell escaping. Shell expansions such as
`~` or `$HOME` are not performed in command arguments. The executable/app name
must be nonempty; no argument may contain a NUL character. Errors opening the
editor appear in the viewer. Removing `editor` restores the platform default.

### `layout`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `layout.remember_per_repository` | `true` | Restore/save interface choices per repository in the separate interface-state file. Does not rewrite settings TOML. |
| `layout.history_visible` | `true` | Default visibility of the history sidebar. |
| `layout.files_visible` | `true` | Default visibility of the changed/all-files sidebar. |
| `layout.bottom_height` | `280` | Shared bottom-dock height/reset target; must lie between min_bottom_height and max_bottom_height. |
| `layout.min_bottom_height` | `120` | Minimum shared bottom-dock height; 40–2000. |
| `layout.max_bottom_height` | `700` | Maximum shared bottom-dock height; min_bottom_height–4000. |

### `layout.placement`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `layout.placement.history_side` | `"left"` | `"left"` or `"right"` for the history sidebar. |
| `layout.placement.files_side` | `"left"` | `"left"` or `"right"` for the file sidebar. |
| `layout.placement.sidebar_order` | `"history_first"` | `"history_first"` or `"files_first"`; left-to-right order when both sidebars share a side. |
| `layout.placement.change_evolution` | `"sidebar"` | `"sidebar"` below history, or `"bottom"` below the workspace. |
| `layout.placement.file_evolution` | `"sidebar"` | `"sidebar"` below files, or `"bottom"` below the workspace. Two bottom timelines appear beside each other. |

## Keyboard profiles: `[keyboard]`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `keyboard.profile` | `"keybindings/default.toml"` | Selected keybinding file. Accepts a nonempty relative or absolute file path without NUL characters. Relative paths resolve beside the active config. Applies live. |
| `keyboard.additional_directories` | `[]` | Additional folders searched by **Select keybindings profile…**. Accepts an array of nonempty relative or absolute directory paths without NUL characters. Relative paths resolve beside the active config. Applies when the picker next opens. |

The `keybindings/` folder beside the config is recommended and searched by default,
but profiles can live anywhere. Extra directories supplement that folder. Discovery
lists `.toml` files without recursing, removes duplicate resolved paths and includes
the active profile even when it is elsewhere. Missing discovery folders are skipped;
unreadable folders report an error. Paths are literal: `~` and environment variables
are not expanded. Use an absolute path for locations outside the config directory.

**Select keybindings profile…** saves the selected path to `config.toml` while
preserving comments. It validates the profile and widget remapping before saving;
a failed selection retains the existing config and shortcuts. **Open keybindings**
edits the active profile using `settings.editor`. Both commands work from welcome
and repository windows and are remappable; they start unbound. Cmd+, continues to
edit the main config. The selected file is watched alongside the config, using the
same reload interval and settled-read behavior. Missing, invalid or unreadable
profiles retain the last valid live configuration and show a copyable error.
Recovery applies automatically. Profiles and shortcuts are application-wide, just
like the existing keymap; when different `--config` windows change bindings, the
last applied configuration controls the shared keymap.

On first use, Jaywalk creates a sparse main config and a commented default profile
if missing. Existing files are preserved. A custom selected path must already exist;
Jaywalk does not create arbitrary selected files. Neither `[keybindings]` nor
`[[keymap]]` is accepted in the main config. There is no automatic legacy migration.
The [keybindings guide](keybindings.md) documents profile syntax and every default.

## Scope and limits

History/evolution options use recorded jj metadata when available. Ordinary Git
comparisons do not gain evolution panels by enabling their visibility settings.
Remote history options control locally available data; Jaywalk does not fetch.

The overview uses inline wrapping and always shows unified diff. Sticky file
headers, its sidebar reading indicator and their directory-first ordering are
currently built-in behavior, not additional configuration keys. Themes and
locales are not configurable through undocumented keys.

When adding or changing a setting, update this reference together with
`config/default.toml`, the typed schema/validation and live-application behavior.

## Comparison selection

`comparison.pair_order` defaults to `"ancestry"`: newly selected related revisions
are ordered ancestor → descendant; unrelated revisions retain selection order.
`"selection"` always keeps first selected → second selected. Set it in the user
configuration or an explicit `--config` file. It applies live to the next manually
created pair. It does
not reorder open comparisons, Back/Forward snapshots, explicit CLI endpoints or
branch-review presets. Swap reverses only the current captured comparison.

## Connected accounts

Public account configuration lives in ordered `[[accounts]]` tables in the active
config file. Once account features restore the OS credential store, Jaywalk appends
newly connected accounts to that file. It preserves existing rows, comments and
unrelated settings. This stays lazy: opening a local diff alone does not read Keychain.

```toml
[[accounts]]
provider = "github"
id = "12345"                 # Stable provider account ID, not the login name.
name = "Personal"            # Optional display name; provider queries keep the real login.
client_id = "public-client-id"

[[accounts]]
provider = "bitbucket"
id = "{account-uuid}"
name = "Work"
client_id = "public-client-id"
workspace = { uuid = "{workspace-uuid}", slug = "my-workspace" }
```

Reorder the tables to set the default account-list order and PR request dispatch
priority across windows. Faster accounts publish results immediately; completion
order never becomes account-list order. Text relevance and the assigned/review
request group still take precedence over account order in PR search results.
Names, visible account order and preferred workspace apply live. A listing/refresh
captures its dispatch order and worker count when it starts; later edits apply to
the next listing, while the shared limit also constrains queued service requests.
The last successfully applied
config source controls application-wide preferences, matching other shared settings.

`provider` accepts `github` or `bitbucket`. `id` is required; `name` and `client_id`
are optional nonempty strings of at most 256 bytes without control characters.
Duplicate provider/ID pairs are rejected; at most 256 preference rows are accepted.
`workspace` is optional and Bitbucket-only. Choosing a workspace through account
commands writes it here. Omitting it clears the preferred workspace; it does not
restrict discovery to one workspace.

The public OAuth `client_id` selects a registration for Connect/Reconnect; its
matching client secret must already exist in the OS store. Changing the client
requires reconnecting and does not reinterpret existing tokens as belonging to a
different app. Config entries do not grant access or create a signed-in account.
Disconnected rows remain as preferences for later reconnection. Removing a row
for a still-connected account causes it to be appended again when metadata syncs.
Unknown/unconnected rows do not appear as active accounts or generate requests.

Access tokens, refresh tokens and client secrets remain in the OS credential
store. Secret fields in account config are rejected. Existing credential records
retain their authentication binding; public config controls display, ordering,
registration selection and workspace preference. Malformed configs and conflicting
edits are preserved, with an error shown while the last valid settings stay active.

## Hosted pull request reviews

Selecting a GitHub or Bitbucket Cloud PR loads its published comparison into the
existing viewer. By default no working checkout is required or changed. Review contents,
commit history and prepared diffs stay in memory; PR-list metadata retains its
existing restart cache. The file browser lists changed files only. Commit rows
open individual first-parent changes; aggregate/overview returns to the pinned
published diff. Pair and local branch comparisons require a local repository.

| Setting | Default | Behavior |
| --- | --- | --- |
| `hosting.concurrent_accounts` | `3` | Maximum independent uncached PR-account discoveries across windows, integer 1–8. Accounts are dispatched in configured order; workspaces and pages within each account remain sequential. Lowering the limit lets in-flight jobs finish and holds new starts. Applies live to new work. |
| `hosting.history_limit` | `200` | PR commit rows to acquire, 1–1000. Keeps the newest available rows; GitHub's PR endpoint exposes at most the first 250 commits of larger PRs. Partial history is marked and never changes the published diff. Applies to the next opened/updated review. |
| `hosting.max_files` | `3000` | Maximum changed files per aggregate or commit, 1–3000. Reviews beyond the cap fail explicitly instead of showing a partial diff. Applies to the next opened/updated review. |
| `hosting.max_content_mb` | `128` | MiB of successfully decoded, unique file text cached per opened review, shared across its clones and individual commit snapshots, 1–1024. Both sides count. The existing `diff.max_text_file_size_mb` also applies per file side before allocation. This bounds cached source text, not total process memory: editor, syntax, diff and navigation state require additional memory. Applies to the next opened/updated review. |

API responses, tree traversal, pagination and loading waits are bounded as well.
Binary/non-UTF-8 files, submodules and unavailable or oversized contents show an
explicit notice; they do not become empty successful diffs. Symlinks display
link-target text. In the default hosted mode, review text is not written into a Git object cache or checkout. The opt-in local modes below acquire Git objects on disk.

A PR remains pinned to its resolved source, target and published base. Refresh
in the viewer, or **Update published PR comparison** in the command palette,
explicitly reloads the latest published version. Background PR-list refresh
never replaces an open review. Back/Forward retains previously opened snapshots.
Cancellation or a failed update leaves the displayed snapshot intact. Deleted
or inaccessible endpoint repositories can prevent opening historical reviews.

## Diff algorithm

`diff.algorithm` defaults to `"myers"` and accepts `"myers"`, `"patience"`. It applies to line matching in Fluid, side-by-side, inline and overview,
for local and hosted comparisons alike. An edit reloads the displayed file and
rebuilds the overview using the same pinned contents and revisions; it performs
no Git fetch or PR version update. Intraline word highlights retain their
existing matching policy. Patience can improve alignment around unique lines
in reordered code. Myers
remains the default. LCS is excluded after poor repetitive-file trial results.
Histogram, Git's minimal policy and indentation-aware
hunk shifting are not options in the current `similar` backend. Algorithm
selection does not ignore whitespace differences.

## Read-only PR status

The hosted overview shows status for the **published PR snapshot head**, including
when an individual commit is being inspected. CI/check observations and reported
review decisions load independently after the diff opens. The status refresh icon
or **Refresh pull request status** command fetches new observations while keeping
the comparison, file selection and reading position. **Update published PR
comparison** remains the separate action that replaces pinned contents.

Status snapshots stay in memory, with up to 16 comparison entries per window and
a 60-second freshness window for revisits. Explicit status refresh bypasses that
cache; there is no automatic polling or restart persistence. A pending refresh
keeps previous observations visible. Cancellation, navigation and late responses
cannot switch the displayed comparison. An unsuccessful whole refresh retains the
previous snapshot with an error; an unavailable individual provider read is
labelled unavailable rather than reusing an old success as current data.

GitHub Checks and legacy commit statuses are read separately for the immutable
head in the target repository and, for forks, the source repository. Each read is
bounded to 500 observations; reviews are bounded to 1,000 history records plus
bounded requested reviewers/teams. Bitbucket reads commit build statuses for the
pinned head in its source repository and decisions from PR participants. Partial
results, unknown states, permission failures and an actual empty result stay
distinct. Safe provider check links open in the system browser.

Reported approvals/change requests do **not** establish merge readiness or satisfy
branch rules. Earlier reviewed GitHub commits are labelled, comment-only reviews
do not revoke an earlier decision, and dismissed decisions are distinguished.
Bitbucket does not supply a reviewed commit in the participant data used here;
its reported decisions explicitly retain that uncertainty. If the PR source or
target moves, live review decisions are unavailable for the pinned version until
the comparison is updated. Checks for the pinned head remain attributable to it.
Merge-result checks on a different SHA and required-merge-rule evaluation are
outside this slice. No approval, comment, rerun, cancellation or merge actions
are submitted to either provider.

GitHub private resources may require **Contents: read**, **Checks: read**, and
**Commit statuses: read**, in addition to Metadata/Pull requests read. Fresh
development registrations request those read-only permissions. Existing apps and
installations are not silently changed; access failures remain visible. Bitbucket
uses its existing Repository and Pull requests read grants. No new app setting is
required for this status presentation.

## Comparison and invisible-character options

These defaults apply to local, standalone and hosted comparisons. Commands override
individual values in the current viewer; they do not edit configuration or fetch a
new snapshot. Opening a new/replaced viewer uses its configuration defaults. Changing a configured
default replaces that value's override; unchanged fields leave overrides intact. Ignore rules rebuild alignment from the same pinned
source. Rendering-only options update without recomputing comparison matching.

| Setting | Default | Meaning |
| --- | --- | --- |
| `diff.whitespace` | `"exact"` | `exact`, `ignore_trailing` (spaces/tabs at line ends), `ignore_changes` (also equate nonempty runs of spaces/tabs), or `ignore_all` (remove spaces/tabs from comparison keys). Leading/interior whitespace presence remains significant in `ignore_changes`. |
| `diff.ignore_line_endings` | `false` | Compare CRLF and LF endings equally. Missing final LF remains significant; bare CR inside an LF row remains source content. |
| `diff.ignore_blank_lines` | `false` | Ignore rows containing only ASCII spaces, tabs or CR/LF when aligning. Blank rows remain present in the source and clipboard. |
| `diff.word_highlight` | `true` | Emphasize changed words within changed lines. Row backgrounds/signs remain independent. |
| `diff.show_whitespace` | `false` | Paint space dots and tab arrows without changing source characters or tab stops. |
| `diff.show_line_endings` | `false` | Paint LF/CRLF/EOF labels at row ends. A final bare CR is labeled CR · EOF; internal bare CR remains content within the LF row. |
| `diff.highlight_whitespace_errors` | `false` | Highlight trailing ASCII spaces/tabs and spaces before tabs within leading indentation. This is visual feedback, not trimming or an edit. |
| `diff.colors.whitespace_error` | `{ light = "#bc4c00", dark = "#ffb454" }` | Error-highlight hue. Accepts a shared hex string or light/dark values, like all configurable colors. |

All modes and the stacked overview use the same filtered alignment for highlighting,
counts, navigation and unchanged-range folding. Ignore filters never erase changed
file metadata or either side's original bytes. The comparison caption names active
ignore rules; files with no visible text changes remain available. Whitespace can
be meaningful in source languages, so exact comparison remains the default.

Visibility commands show markers rather than inserting glyphs into the buffer.
Copied source excludes marker labels and includes original spaces, tabs and endings.
`Toggle whitespace differences` switches between exact matching and the last chosen
ignore-whitespace mode (initially `ignore_all` when no non-exact mode was selected).
Moved-code detection is not implemented by these settings.

## Local PR review (`local_review`)

These application-level settings apply to the next PR opening or explicit **Review PR locally…** action; changing them does not switch an already-open repository. They update live in both the retained PR browser and its picker. Project settings remain separate.

| Setting | Default | Behavior |
| --- | --- | --- |
| `local_review.mode` | `"hosted"` | `hosted` uses provider-backed content. `isolated` tries a matching known repository and acquires pinned PR objects in private Jaywalk-owned temporary storage, leaving the known checkout/refs/index unchanged. `checkout_if_clean` additionally attempts a temporary detached checkout in a clean plain-Git repository, with guarded restoration. All local behavior is opt-in. |
| `local_review.fetch_missing` | `true` | Allow Git transport to fetch exact PR revisions missing from the known repository. Set false to reuse locally available objects only, with hosted fallback on missing/unavailable objects. Applies to the next local acquisition; per-repository transport credentials remain separate from provider API sign-in. |
| `local_review.max_repository_mb` | `512` | Temporary Git storage budget in MiB, 16–8192. Checked periodically while Git runs and after acquisition; transient disk usage can exceed the budget between checks. A storage-limit failure stops acquisition and retains hosted viewing. This is not a hard OS disk quota or process-memory bound. |

Normal PR opening considers the current repository and recent/opened repository history (at most 50 candidate paths). Matching recognizes Bitbucket’s `bitbucket.org`, `ssh.bitbucket.org` and `altssh.bitbucket.org` SSH remotes (including explicit ports). It uses supported Git remote provider/repository identities, including known fork transport URLs when available, rather than directory names. A missing or ambiguous match, failed Git authentication or refused checkout keeps the hosted snapshot and explains why. **Review PR locally…** lets you select a matching repository explicitly; with the default `hosted` setting that command uses isolated acquisition. With `checkout_if_clean`, it follows the clean-checkout policy instead. Git transport uses its own configured credentials/SSH agent; a provider API connection does not establish Git transport access.

The local source fetches exact published head/base objects with shallow history into an independently owned object store. It never uses a long-lived alternate pointing into a user repository and never initializes/fetches submodules. Full snapshot files become available to the file picker, All-files browser and project symbols; the published PR identity, aggregate, provider commit series and read-only status remain attached. Individual commit inspection can acquire additional pinned objects on demand. Existing per-file and decoded-content limits still apply. Review objects are ephemeral: normal cleanup follows retained snapshot lifetime; a forced process termination can leave private temporary directories for later OS/manual cleanup. PR-list metadata keeps its existing restart cache.

Clean-only checkout refuses staged, unstaged, untracked and submodule changes, in-progress Git operations, and jj/colocated repositories. It also protects ignored files from overwrite. Preflight records the checkout before acquisition and rechecks repository identity, HEAD and branch before switching. No stash, force checkout, reset, clean, branch reset, pull, merge or hook execution is used. Missing checkout objects are fetched using normal Git transport in this explicitly enabled mode, with graph connectivity verified before switching; a shallow scratch store is never used to seed a user checkout. Acquisition disables tag updates, pruning, submodule fetches and automatic maintenance, and does not update branch refs or FETCH_HEAD. An already matching clean checkout is reused without detaching it.

A temporary checkout writes and syncs a private recovery receipt before switching. Ownership is shared between viewers of the same checkout/revision. Replacing the active comparison, navigating to a different comparison, or releasing its last owning viewer schedules restoration; Back/Forward can retain pinned review objects without owning the user checkout. Temporarily opening Welcome, Accounts or the PR browser retains the active comparison and its checkout; use Restore review checkout to finish it explicitly without closing the window. Normal quit waits for guarded restoration on its worker. Restoration proceeds only if the repository is still clean at the expected detached revision and the original branch has not moved; otherwise it leaves the repository alone and retains the receipt. External Git commands and editor writes cannot be locked out by a preflight check.

**Restore review checkout…** lists receipts in the active configuration's `review-checkouts/` state directory, including receipts from interrupted runs. It verifies repository identity and current state before doing anything. Recovery is never forced or performed automatically on startup. Once a protected checkout/restore has begun, closing its picker stops waiting rather than cancelling that Git operation; the receipt and restoration worker retain ownership. Each protected Git command has a 30-second deadline. Failure, interruption or an external lock can leave a receipt requiring manual recovery; no forced repair is attempted. A later review cannot reuse an old receipt, and checkout creation stops during application quit.

These actions prepare local review sources and optional checkout only. They do not start a language server or implement Cmd/Ctrl-click Go to Definition yet. Semantic navigation must distinguish exact Before/After revisions and report unavailable context rather than using the current checkout for another revision. Explicit Fetch/Pull of the user's repository remains separate work.
