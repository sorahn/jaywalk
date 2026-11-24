# Settings reference

Keyboard defaults include Linux-specific recent-repository and fullscreen bindings;
see the [keybindings guide](keybindings.md). Sparse profiles inherit the active
platform's defaults before applying their explicit overrides.

Jaywalk reads TOML settings. On first open, it creates an ordered copy of the
[generated user template](../config/user.toml). Its values come from the
[default configuration](../config/default.toml); descriptions, choices, ranges and
optional examples share the Settings UI catalogue. The
[complete generated key reference](settings-reference.md) uses those same sources.
This guide adds configuration behavior and longer usage examples.
Section headers are active; default values are commented. Uncomment just the
values you want to change; omitted values
inherit current defaults, including defaults changed by future releases.
Existing files and explicit overrides are preserved.
The schema and validation live in [src/settings.rs](../src/settings.rs).

## Native settings page

Cmd/Ctrl+, or **Show settings** opens a separate Settings window. Repeated opens reuse the window for that
configuration file and leave the review untouched. Eight UI
categories group the existing TOML keys without renaming them. A divider below
the categories separates **Accounts**, which opens account management in this
window. It is excluded from All settings, Changed from defaults, and settings
search. The Accounts command opens this destination directly. Switching preserves
the settings query and unsaved draft; Save/Discard appear only on settings pages.
Account connection actions take effect immediately and are shared across windows.
Search covers keys,
titles and descriptions globally by default; selecting a category narrows the
search. All settings, Changed from defaults, and categories are exclusive
navigation destinations; selecting one clears the previous selection. Repeated
clicks keep that destination selected. A category explicitly selected during a
search stays selected when the query is cleared. **Changed from defaults** shows
configured values that differ from the
running defaults. Compact list rows use a consistent description column and a fixed-width,
right-aligned control column. Text values are right-aligned; numeric stepper values are centered. Changed settings show a small reset icon beside their title; its
tooltip includes the default value. Hover descriptions for key/default/inheritance
details. Right-click a title to copy its settings key or remove an explicit
override, including one equal to the default. Runtime toolbar/layout memory is separate.

Categories are subdivided into related topics, such as Colors, Interface text,
History, Files and Live refresh. Advanced controls are folded beside the ordinary
settings they refine, rather than collected at the bottom of a category. Folds
include **Theme generation**,
**Detailed colors**, **Fine layout controls**, and **History loading and limits**.
Click their caret to expand them. A collapsed section shows how many of its
settings differ from defaults. Search reveals matching controls automatically;
Changed from defaults also ignores folding. Clearing search restores the previous
browsing position and expansion choices. Update consent and repository-changing
policies remain visible.

Rows use spacing 2 (8px by default) on each side and size to their content.
The content headers in Settings and Keyboard Shortcuts use the same outer padding.

Expansion is interface state, saved independently beside the configuration file:
`config.toml` uses `config.view-state.toml` (custom filenames use the same suffix).
It survives reopening/restarting and never writes a setting override or grants
update consent. The interface-state worker coalesces these writes and flushes on
normal application quit. Settings drafts survive folding; Save/Discard still
control preference changes.

Settings controls edit a window-local draft. **Save** (Cmd/Ctrl+S) validates and
writes validated changes, with atomic replacement of each affected file.
**Discard** restores the latest saved values. Closing Settings or choosing Quit
with unsaved changes offers **Save / Discard / Cancel**; a failed or invalid save
keeps the window and draft open. Valid presentation changes preview across viewers
using this settings file, without writing settings or session layout. Discard or
closing with Discard restores the latest saved configuration. Update consent,
repository action policies, persistence policy and other nonpresentation settings
take effect only after Save; these controls are marked accordingly. Numeric fields have −/+ buttons with setting-specific step sizes and
bounds, including limits derived from other settings. Enter or leaving a text,
numeric, or color field validates it into the draft; valid typing previews immediately and incomplete values retain the last valid preview. Save also includes an active
field that has not blurred. Invalid drafts stay local and show a validation error.
Update consent becomes effective only after saving it. The TOML schema remains
authoritative for validation, including relationships between settings.
Closed dropdowns fit their selected label and stay anchored to the right; menu labels stay left aligned. Numeric fields reserve space for their bounds and decimal step instead of filling the control column. Color inputs use a compact hex-sized field with a swatch on the left. Click the swatch to open the native toolkit color picker: choose a palette color or use hue/saturation/lightness controls and exact hex entry. Picker selections preview through the same draft; Save writes them and Discard restores the saved color. Merely opening or dismissing the picker creates no override. Colors are RGB; opacity controls are hidden. The accent field also previews the resolved system accent when set to `system`. The sun-moon icon toggles separate light/dark colors (accent when on, neutral when off); its tooltip describes the state and action. The field shows and edits only the active appearance, preserving the other value, without a repeated appearance label. Enabling it copies the shared color into both appearances (a system accent becomes its currently resolved RGB color). Disabling it uses the displayed appearance’s color for both. Existing pairs start separated. The dedicated light/dark shell seed fields already select their appearance and do not need this toggle.

Font families use searchable dropdowns of installed fonts. Use default removes
the override; an existing configured family remains selectable even if it is not
installed on this machine. Escape reverts the focused field to its staged value, clears a search, or
requests closing the Settings window. Reset stages removal of the explicit override
and inherits the current default. Update prerequisite revocation is part of the
same atomic Save as its dependent settings.
Appearance exposes the light/dark shell seed colors, accent, and added/removed
row backgrounds. Detailed diff colors are grouped under **Advanced diff colors**
with **Edit TOML…**; fine color overrides remain supported in TOML. Editing one
shell seed mode preserves the other, including when the previous value was shared.

Advanced tuning values are omitted from ordinary category browsing. Search by
key, label or description to find a documented **Edit TOML…** row; Changed from
defaults also shows configured TOML-only values. They retain their existing keys,
defaults, validation and live-file-reload behavior. Their scope includes loading
and polling timing, initial history batching, account-request concurrency, layout
bounds and startup dimensions, individual bar/control heights, graph lane/badge
geometry, indent-guide offset and the overview message-height limit. Fonts,
spacing, row height, panel placement, diff/gutter behavior, history page size,
file/repository resource limits and theme-generation controls keep their widgets.

Three everyday structured values have focused editors:

- **Settings file editor:** choose System default, Application or Command. Application
  accepts a name or a selection from the folder icon; Command has an executable and one literal
  argument per field. Use Add new, the × buttons and drag handles to manage rows. The config path is appended as
  one argument. These fields never execute a command while being edited.
- **Additional profile folders:** expand the folder count, then use Add new to create a row.
  Type a path or choose one with its folder icon; drag the handle to reorder or use ×
  to remove a row. Relative paths
  resolve beside the config file. The folder order and argument boundaries are preserved.
- **Remote visibility overrides:** expand the override count for Default/Show/Hide
  per name. Suggestions come from the repository that opened Settings, read locally
  without fetching; saved and manually added names remain available. Rules are global
  settings for those names. Default removes the explicit rule and follows
  `history.remotes.default_visible`.

These editors share Save/Discard, invalid-input and external-edit conflict handling.
Only reversible remote-visibility presentation previews live; editor commands and
profile-folder changes take effect after Save. Cancelled or stale path-chooser
responses do not modify the draft.

Other advanced lists/maps remain searchable with **Edit TOML…**. **Keyboard Shortcuts**
and **Accounts** are separate sidebar destinations below the regular settings,
in that order, excluded from All/Changed and settings search. Keyboard profile
selection and application-command remapping share the settings Save/Discard draft;
account actions remain immediate. **Show default settings** presents a read-only
reference. Project settings remain deferred.

Save patches only the edited keys in the latest validated file, preserves
comments/permissions and symlinks, and rejects the entire batch on stale same-field
edits. External updates refresh untouched values without replacing pending intents.
Discard clears the draft after a conflict; unrelated external edits are preserved. Committed documents publish through
the shared source service with revisions; manual edits continue to reload live.
The configured external editor remains `settings.editor`, independent of the
`editor` section for code typography. Explicit old `open_settings` keybindings
keep opening the file; see [keybindings](keybindings.md).

## Configuration file and reload

- Default: `~/.config/jaywalk/config.toml`.
- With a nonempty `XDG_CONFIG_HOME`: `$XDG_CONFIG_HOME/jaywalk/config.toml`.
- `jw . --config /path/to/config.toml` selects a separate file, which must exist.
- **Open settings file…** opens the active file. A missing default file is
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
are keyed by canonical root. Git difftool uses the root recorded in the capture,
so its windows share choices with `jw .`, the recent-repository picker, and other
difftool invocations for that repository. Difftool continues to display the
supplied diff bytes and comparison endpoints. Anonymous path-to-path comparisons
start with the configured defaults and keep interface changes in the window.
Jaywalk manages the interface state file automatically.

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

For an exhaustive list generated from the running schema's source files, see the
[complete key reference](settings-reference.md). The tables below accompany the
longer usage explanations.

Each table uses full dotted keys. In TOML, put the short key below the matching
section header, for example `[editor]` followed by `font_size = 14`.

### `application`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `application.quit_on_last_window` | `true` | Quit when the final actual window closes, including About. `false` keeps the app available with no windows. This is application-wide: opening another config or editing this value updates the shared policy; CLI waiters are notified before shutdown. |
| `application.single_window_mode` | `false` | Normal opens from `jw`, Ctrl+R/recent repositories, welcome, and Open actions replace the originating viewer, or the active/most recently used viewer for a CLI launch, even for a different repository, range, or config. `false` opens a new window for normal opens once a repository or PR is loaded. An empty startup window is reused on Setup or Welcome so those pages do not leave an extra window behind. Accounts opens within the separate Settings window. Explicit Open in new window and the recent-picker modifier always create a new window. Applies on the next open; uses the application-wide last-applied config-source policy. |

### `picker`

| Setting | Default | Behavior |
|---|---|---|
| `picker.adaptive_command_order` | `true` | Window-local preference controlling command usage learning and ordering. With an empty command query, up to two frequent/recent commands precede suggestions for the current view; textual relevance takes priority for typed queries. Edits apply live. The usage snapshot stays fixed during an opening so activity in another window does not reorder the selected results. A new opening reads the latest shared history. `false` keeps deterministic context suggestions and textual/alphabetical ordering and stops recording commands in that viewer; existing saved history remains available if re-enabled. |

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
| `ui.loading_delay_ms` | `400` | Delay before showing loading feedback; 0–5000. `0` shows it immediately; fast operations never expose a loading label. |
| `ui.row_height` | `26` | Shared minimum for history, tree and list rows; 12–160. Per-view overrides below take precedence; fonts establish a readable minimum. |
| `ui.panel_header_height` | `30` | Minimum panel-header height; 24–120. |
| `ui.toolbar_height` | `28` | Minimum review-control height within the single comparison header; 28–120. The header fits the greater of this value, `ui.comparison_bar_height`, and its font minimum. No separate application-toolbar row. Applies live. |
| `ui.footer_height` | `28` | Minimum application footer height; 20–120. Also sets the minimum History/Files footer height, which grows to fit their own font and evolution control. Effective height also fits its control height and at least 1.5 × footer font size. |
| `ui.footer_font_size` | `12` | Footer label font size; 8–48. |
| `ui.footer_control_height` | `22` | Footer control height; 16–80. |
| `ui.comparison_bar_height` | `30` | Reserved comparison/evolution toolbar height, including compact back/forward controls; 24–120. Grows to fit the UI font. |
| `ui.font_size` | `16` | Application control font size; 8–48. Does not replace independent source/sidebar sizes. |
| `ui.spacing.unit` | `4` | Logical pixels per spacing unit; 1–16. Independent of UI/source font sizes. Applies live to app chrome, pages, and floating controls. |
| `ui.spacing.edge` | `2` | Unit multiplier for edges, section gaps, and default tree indentation; 0–8. Default 8px. |
| `ui.spacing.interactive` | `1` | Unit multiplier for padding within hover/selection targets and tight control gaps; 0–8. Default 4px. |
| `ui.ligatures` | `true` | Enable ligatures and contextual alternates for application text. |
| `ui.font_family` | omitted | Installed family name; defaults to the theme/system UI font. Must be nonempty when supplied. |

Spacing uses one scale: `space(1)` = 4px, `space(2)` = 8px,
`space(3)` = 12px, `space(4)` = 16px, and `space(6)` = 24px at defaults.
Edges normally use two units and interactive content one. Larger page margins
use multiples of the same unit. History/file row backgrounds span their panels
with square edges; floating selection targets and toggle groups are rounded. Toggle groups share
their buttons' hover boundary: the interactive inset is applied once, inside
each button, without additional group padding.
Bars grow beyond their configured minimum when controls/padding need more room.
Border widths, icon sizes, and corner radii keep their own geometry.

```toml
[ui.spacing]
unit = 4
edge = 2
interactive = 1
```

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
| `files.tree_scope` | `"changed"` | `"changed"` or `"all"`. Filters tree mode only. `all` includes unchanged files in the selected comparison/revision; The file picker searches the entire captured comparison regardless of this filter. |
| `files.indent_guides` | `true` | Show vertical ancestor guides in the directory tree. Applies live. |
| `files.indent_guide_offset` | `-2` | Horizontal guide offset from ancestor icon centers in pixels; −48–48. Negative moves left. Applies live. |
| `files.indent_guide_opacity` | `0.12` | Guide opacity using the theme foreground; 0–1. Applies live. |
| `files.file_icons` | `true` | File-type glyphs in list/tree views; unknown types use a generic file. Folder icons always indicate open/closed state. Applies live. |
| `files.indent_width` | omitted (8px derived) | Directory-tree indentation follows `ui.spacing.unit × ui.spacing.edge`. An explicit 1–128px override remains supported. |
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
| `keyboard.additional_directories` | `[]` | Additional folders searched by **Select keybindings profile…**. Accepts an array of nonempty relative or absolute directory paths without NUL characters. Relative paths resolve beside the active config. Applies when Keyboard Shortcuts next loads profiles. |

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

GitHub's Accounts page offers **Connect with browser…**, **Paste token…**, and
**Use GitHub CLI token**. The CLI option is disabled while detection is pending or
`gh` is unavailable. Detection checks executable metadata on a worker; importing
runs only after selection. Finder launches also check common Homebrew locations.
Reopening Accounts repeats detection after installing `gh`.

Pasted tokens use a masked field. Both token paths validate the GitHub identity
before saving to the OS credential store. For an existing account, use **Use token…**
or **Replace token…** on its row, then explicitly replace the connection. A failed
validation/save keeps the previous connection. One credential is stored per
provider/user identity; token scopes and repository/organization policies determine
available features. `gh` import copies its saved github.com bearer credential,
ignores ambient `GH_TOKEN`/`GITHUB_TOKEN` overrides, and does not share the CLI's
refresh lifecycle. Imported tokens can expire or be revoked; replace them when
needed. Browser sign-in remains available, including when switching back from a
token connection. Local disconnect deletes only Jaywalk's copy.

Public account configuration lives in ordered `[[accounts]]` tables in the active
config file. Once account features restore the OS credential store, Jaywalk appends
newly connected accounts to that file. It preserves existing rows, comments and
unrelated settings. This stays lazy: opening a local diff alone does not read Keychain.

```toml
[[accounts]]
provider = "github"
id = "12345"                 # Stable provider account ID, not the login name.
name = "Personal"            # Optional display name; provider queries keep the real login.
authentication = "oauth"
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

`authentication` is `"oauth"` (default for existing entries) or `"token"`.
It records the connection method; editing it cannot import a token or convert a
stored credential. Token connections omit `client_id`; specifying both is invalid.
Account actions synchronize the binding while preserving names, order and comments.
The new token connection choices are currently GitHub-only.

`provider` accepts `github` or `bitbucket`. `id` is required; `name` and `client_id`
are optional nonempty strings of at most 256 bytes without control characters.
Duplicate provider/ID pairs are rejected; at most 256 preference rows are accepted.
`workspace` is optional and Bitbucket-only. Choosing a workspace through account
commands writes it here. Omitting it clears the preferred workspace; it does not
restrict discovery to one workspace.

For browser connections, the public OAuth `client_id` selects a registration for Connect/Reconnect; its
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
| `local_review.mode` | `"hosted"` | `hosted` uses provider-backed content. `isolated` tries a matching known repository and acquires pinned PR objects in private Jaywalk-owned temporary storage, leaving the known checkout/refs/index unchanged. `checkout_if_clean` additionally attempts a temporary checkout in a clean plain-Git repository, with guarded restoration. Branch selection follows `checkout_branch`. All local behavior is opt-in. |
| `local_review.fetch_missing` | `true` | Allow Git transport to fetch exact PR revisions missing from the known repository. Set false to reuse locally available objects only, with hosted fallback on missing/unavailable objects. Applies to the next local acquisition; per-repository transport credentials remain separate from provider API sign-in. |
| `local_review.checkout_branch` | `"create_from_remote"` | In `checkout_if_clean` mode: `detached` uses the pinned commit; `existing` reuses the PR’s named local branch only at that exact commit; `create_from_remote` also creates that local name when a matching PR-source remote-tracking branch points to the pinned commit. No `origin/` prefix is added to the local name. A conflicting local branch is never reset; missing or mismatched refs fall back to detached checkout. Applies to the next acquisition. |
| `local_review.cleanup_created_branch` | `false` | After guarded restoration, optionally delete a branch created by that review, only if its commit and creation ownership marker are unchanged and no worktree uses it. Existing branches are never deleted. Creation uses an atomic absent-ref check and a receipt-specific reflog marker; deletion checks the expected commit atomically; skipped cleanup retains the recovery receipt for retry. This choice is saved in the receipt at acquisition, so later config edits do not alter an active review’s cleanup policy. |
| `local_review.max_repository_mb` | `512` | Temporary Git storage budget in MiB, 16–8192. Checked periodically while Git runs and after acquisition; transient disk usage can exceed the budget between checks. A storage-limit failure stops acquisition and retains hosted viewing. This is not a hard OS disk quota or process-memory bound. |

Normal PR opening considers the current repository and recent/opened repository history (at most 50 candidate paths). Matching recognizes Bitbucket’s `bitbucket.org`, `ssh.bitbucket.org` and `altssh.bitbucket.org` SSH remotes (including explicit ports). It uses supported Git remote provider/repository identities, including known fork transport URLs when available, rather than directory names. A missing or ambiguous match, failed Git authentication or refused checkout keeps the hosted snapshot and explains why. **Review PR locally…** lets you select a matching repository explicitly; with the default `hosted` setting that command uses isolated acquisition. With `checkout_if_clean`, it follows the clean-checkout policy instead. Git transport uses its own configured credentials/SSH agent; a provider API connection does not establish Git transport access.

Opening first pins the provider-published comparison and paths. In local modes, a unique matching repository is acquired before per-file provider metadata: file types, executable bits and blob identities come from the exact local Git trees. The published base, source/target identities, file list, renames and commit history remain authoritative; Jaywalk does not substitute a guessed merge base. The PR endpoints are verified again after acquisition and before checkout. A changed PR aborts the open; local matching/acquisition/checkout failures use hosted metadata with an explanatory notice. Local review refresh uses the same fast metadata path, without initiating a new checkout. The picker shows the current stage while opening.

The local source fetches exact published head/base objects with shallow history into an independently owned object store. It never uses a long-lived alternate pointing into a user repository and never initializes/fetches submodules. Full snapshot files become available to the file picker, All-files browser and project symbols; the published PR identity, aggregate, provider commit series and read-only status remain attached. Individual commit inspection can acquire additional pinned objects on demand. Existing per-file and decoded-content limits still apply. Review objects are ephemeral: normal cleanup follows retained snapshot lifetime; a forced process termination can leave private temporary directories for later OS/manual cleanup. PR-list metadata keeps its existing restart cache.

Clean-only checkout refuses staged, unstaged, untracked and submodule changes, in-progress Git operations, and jj/colocated repositories. It also protects ignored files from overwrite. Preflight records the checkout before acquisition and rechecks repository identity, HEAD and branch before switching. No stash, force checkout, reset, clean, branch reset, pull, merge or hook execution is used. Missing checkout objects are fetched using normal Git transport in this explicitly enabled mode, with graph connectivity verified before switching; a shallow scratch store is never used to seed a user checkout. Acquisition disables tag updates, pruning, submodule fetches and automatic maintenance, and does not update branch refs or FETCH_HEAD. An already matching clean checkout is reused without detaching it. Newly created local review branches are not assigned an upstream and remain after restoration unless `cleanup_created_branch` is enabled.

A temporary checkout writes and syncs a private recovery receipt before switching. Ownership is shared between viewers of the same checkout/revision. Replacing the active comparison, navigating to a different comparison, or releasing its last owning viewer schedules restoration; Back/Forward can retain pinned review objects without owning the user checkout. Opening Accounts in Settings leaves the review untouched. Temporarily opening Welcome or the PR browser retains the active comparison and its checkout; use Restore review checkout to finish it explicitly without closing the window. Normal quit waits for guarded restoration on its worker. Restoration proceeds only if the repository is still clean at the expected revision and branch/detached identity and the original branch has not moved; otherwise it leaves the repository alone and retains the receipt. External Git commands and editor writes cannot be locked out by a preflight check.

**Restore review checkout…** lists receipts in the active configuration's `review-checkouts/` state directory, including receipts from interrupted runs. It verifies repository identity and current state before doing anything. Recovery is never forced or performed automatically on startup. Once a protected checkout/restore has begun, closing its picker stops waiting rather than cancelling that Git operation; the receipt and restoration worker retain ownership. Each protected Git command has a 30-second deadline. Failure, interruption or an external lock can leave a receipt requiring manual recovery; no forced repair is attempted. A later review cannot reuse an old receipt, and checkout creation stops during application quit.

The explicit **Checkout branch…** and **Checkout this branch** commands perform permanent plain-Git checkouts, independently of `local_review.mode`. Local branches are listed first, then known remote-tracking refs; remote HEAD aliases are omitted. A remote branch creates or reuses a local branch without the remote prefix, but an existing local name at another commit is refused. New branches have no upstream assigned and are not covered by `cleanup_created_branch`. No automatic restoration is scheduled for these explicit commands.

Checkout this branch targets the aggregate review's named tip or captured PR source, not an individually inspected history commit. PR checkout uses a matching known repository, asks inside the picker if multiple match, and honors `fetch_missing` when acquiring a missing captured commit. It does not update branch refs, tags or FETCH_HEAD during acquisition. Arbitrary unnamed ranges require choosing a branch explicitly. Both commands retain the open comparison; use Refresh to resume workspace following. The same clean-state, ignored-file, identity, operation and hook protections apply; an existing temporary review checkout/recovery receipt must be restored first. jj checkout is deferred.

These actions prepare local review sources and optional checkout only. They do not start a language server or implement Cmd/Ctrl-click Go to Definition yet. Semantic navigation must distinguish exact Before/After revisions and report unavailable context rather than using the current checkout for another revision. Explicit Fetch/Pull of the user's repository remains separate work.

## First-open setup and updates

The app creates a missing default settings file before showing Setup once. File
existence is the first-open marker: later launches skip automatic Setup even if
you skipped or closed it. Empty and older existing files count too. No `[setup]`
section is needed; old `seen`/`completed` entries are ignored and removed the next
time Setup saves preferences. Explicit `--config` files must already exist.
**Set up Jaywalk…** remains available from the command palette at any time.

New files contain empty sections and commented defaults, so creating one grants no update consent.
Update checkboxes save individual choices immediately using ordinary TOML
sections; Save and continue saves appearance, and Skip leaves unsaved appearance
unchanged. Update consent comes only from the saved update options, independently
of whether Setup was shown. Appearance uses `ui.appearance`.

| `[updates]` key | Default | Accepted values / behavior |
| --- | --- | --- |
| `automatic_checks` | `false` | Boolean; explicit checkbox/config opt-in, independent of setup completion |
| `automatic_downloads` | `false` | Boolean; requires automatic checks; downloading alone never authorizes installation |
| `automatic_installation` | `false` | Boolean; requires automatic downloads; explicitly permits installation on quit when restart guards allow |
| `check_interval_hours` | `24` | Integer 1–168; automatic-check cadence |

These public settings apply live application-wide, using the same active configuration-source policy as other shared settings. Disabled update options cancel background raw downloads and prevent new automatic checks; an already running feed request can complete but its result cannot start another download after opt-out. Explicit manual checks/downloads are separately authorized. Updating is unavailable on builds without the macOS updater feature or configured public feed/key; preferences are still saved. CLI installation has an explicit chosen destination and does not edit shell startup files. See [installation](installation.md) and [release operations](releasing.md).

The account `client_id` identifies the OAuth app registration, not the user
access token. In builds with the public GitHub client configured, a fresh
installation uses that client automatically. A stored session keeps resolving
its own client ID after restart; explicit development registrations keep
working. Publisher build credentials do not go in user settings. Repository
installation is available from **Choose GitHub repositories…** on Accounts.

## Shell themes

On macOS, window content has an explicit 1px divider below the native title bar,
using the Header border color. It applies to main, Settings and About windows.

The shell palette resolves once when settings load, for both light and dark
appearance. Changing colors live preserves the open comparison. Appearance
still uses `ui.appearance = "system"`, `"light"`, or `"dark"`; the system accent
and `ui.accent_color` remain independent of the shell seed.

The neutral default follows the selected Layered shell prototype: a dark viewer
at `#121212`, History at `#1f1f1f`, Files at `#181818`, and app bars at `#2b2b2b`.
The soft neutral light counterpart uses `#f7f7f7`, `#e4e4e4`, `#eeeeee`,
and `#d4d4d4`, respectively, with the same Layered 1.4 offsets in reverse. Panel colors follow History/Files identity when moved.
No focused docked-panel outlines are added.

`[theme.shell_auto]` controls the versioned `tonal-v1` OKLCH recipe. Every numeric
knob accepts either one number or `{ light = ..., dark = ... }`. Seeds and exact
color overrides accept one `"#rrggbb"` string for both modes, or separate
`{ light = "#rrggbb", dark = "#rrggbb" }` colors.

| Setting | Default | Range / effect |
| --- | --- | --- |
| `enabled` | `true` | `false` uses fixed built-in shell colors; exact overrides still apply |
| `algorithm` | `"tonal-v1"` | Versioned recipe; unknown versions are rejected |
| `seed` | `{ light = "#575757", dark = "#121212" }` | Independent primary colors; colored seeds tint the actual surfaces |
| `layer_strength` | `1.4` | 0–2.5; separation between viewer, sidebars and bars |
| `hue_shift` | `0.0` | −180–180 degrees |
| `chroma_scale` | `1.0` | 0–3; saturation multiplier |
| `tint_strength` | `1.0` | 0–1; blend toward a neutral palette |
| `contrast_spread` | `1.0` | 0.2–2.5; spread of reference surface lightness |
| `lightness_shift` | `0.0` | −0.15–0.15; overall OKLCH lightness adjustment |
| `seed_lightness_influence` | `1.0` | 0–1; how strongly seed lightness shifts the foundation |

Out-of-gamut colors reduce chroma before conversion to sRGB. Foregrounds and
muted text are chosen against each resolved surface with a 4.5:1 contrast
target. Exact foreground overrides are honored even at lower contrast; the
resolved palette retains contrast warnings for diagnostics. Semantic diff and
graph colors are independent of the seed.

Per-surface adjustments live under `[theme.shell_auto.roles."ROLE.background"]`:
`lightness_shift` (−0.2–0.2, default 0), `hue_shift` (−180–180, default 0), and
`chroma_scale` (0–3, default 1). Adjustments apply after global controls. Exact
colors in `[theme.overrides]` apply last; foregrounds automatically adapt to
an overridden background unless they are explicitly overridden too.

```toml
[theme.shell_auto]
seed = { light = "#575757", dark = "#153049" }
layer_strength = 1.4

[theme.shell_auto.roles."shell.sidebar.files.background"]
lightness_shift = { light = 0.0, dark = 0.02 }

[theme.overrides]
"shell.sidebar.history.background" = { light = "#e6e6e6", dark = "#202832" }
"shell.sidebar.files.background" = { light = "#efefef", dark = "#19212b" }
"graph.lanes.0" = "#729dcc"
```

Surface prefixes are `shell.window`, `shell.header`, `shell.footer`,
`shell.sidebar.history`, `shell.sidebar.files`, each sidebar's `.header` and
`.footer`, `shell.floating`, `editor`, and `editor.header`. Each has
`.background`, `.foreground`, `.foreground.muted`, `.border`, and `.group.background`.
Toggle groups derive their fill by blending that surface background 6% toward its
foreground; per-surface exact `.group.background` overrides are supported. The window
foreground/muted/border names are shortened to `shell.foreground`,
`shell.foreground.muted`, and `shell.border`.

Additional exact roles are `graph.lanes.0` through `graph.lanes.4`,
`graph.evolution.current`, `graph.evolution.muted`, `status.success`,
`status.warning`, `status.error`, `status.info`, `diff.renamed.marker`,
`diff.copied.marker`, `shell.control.background`, `shell.control.hover`,
`shell.control.foreground`, and `shell.backdrop`. The default backdrop is
translucent black; an explicit RGB override is opaque. Existing `[diff.colors]`
settings continue to control added/removed/modified markers and diff highlights.
Unknown role names are rejected to catch typos. Floating borders follow the accent
while `ui.accent_floating_outlines` is enabled; disable it to use
`"shell.floating.border"` instead.

This shell slice retains the existing syntax palette. VS Code theme imports,
independent diff-theme files, theme selection/preview commands, and custom
native titlebar coloring remain separate work.

Standard buttons use spacing 2 horizontally and spacing 1 vertically. Compact toolbar
buttons use spacing 1 horizontally and spacing 0.5 vertically. These follow the configured shared
spacing scale; explicit control dimensions remain unchanged.

Dropdown triggers use the shared Button widget, with a trailing chevron and
spacing 2 at their horizontal edges. All insets belong to the button container;
labels and chevrons have no additional padding or compensating margins. Compact buttons and compact
dropdowns use line height 1.2 and retain their smaller vertical inset; menu rows
remain left aligned.

Numeric stepper values are centered between their decrement/increment buttons;
ordinary editable text fields remain right aligned.

Dropdowns keep a faint neutral border, strengthen it on hover, and use the accent
outline for focus/open state. Clicking outside a focused field releases its
focus while preserving Settings keyboard navigation. Save status shares the
settings-file path row, so successful saves do not change the header height.

Settings writes serialize control edits without briefly dimming every control.
Unchanged retained input values are not reset when a new snapshot arrives.

Visible settings summaries use at most two lines, with an ellipsis when needed.
Their tooltips retain the full explanation, key, default and inheritance details.
Search matches both the summary and full description.
