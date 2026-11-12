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

### `welcome`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `welcome.recent_count` | `5` | Number of recent repositories shown on the welcome page; 1–50. Applies live. This changes only the preview count, not the saved repository history. |

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
| `ui.accent_color` | `{ light = "#0066b8", dark = "#3794ff" }` | Application-wide accent: a shared `#RRGGBB` string or a light/dark pair (see below), case-insensitive hex. Updates active icons, primary controls (including checkboxes/switches), focus rings, links and selection tints live in all windows; filled controls choose a light/dark foreground. Git statuses, diff colors and graph lanes stay semantic. Invalid edits retain the last valid config. |
| `ui.loading_delay_ms` | `200` | Delay before showing loading feedback; 0–5000. `0` shows it immediately; fast operations never expose a loading label. |
| `ui.row_height` | `26` | Shared minimum for history, tree and list rows; 12–160. Per-view overrides below take precedence; fonts establish a readable minimum. |
| `ui.panel_header_height` | `30` | Minimum panel-header height; 24–120. |
| `ui.toolbar_height` | `36` | Minimum main-toolbar height; 28–120. |
| `ui.footer_height` | `28` | Minimum footer height; 20–120. Effective height also fits its control height and at least 1.5 × footer font size. |
| `ui.footer_font_size` | `12` | Footer label font size; 8–48. |
| `ui.footer_control_height` | `22` | Footer control height; 16–80. |
| `ui.comparison_bar_height` | `36` | Reserved comparison/evolution toolbar height; 24–120. Grows to fit the UI font. |
| `ui.font_size` | `16` | Application control font size; 8–48. Does not replace independent source/sidebar sizes. |
| `ui.ligatures` | `true` | Enable ligatures and contextual alternates for application text. |
| `ui.font_family` | omitted | Installed family name; defaults to the theme/system UI font. Must be nonempty when supplied. |

All configurable colors use the same format. A string applies in both modes;
a table selects a color for the active appearance. Omitted table members inherit
their shipped defaults. These examples are alternatives:

```toml
[ui]
appearance = "system"
accent_color = { light = "#0066b8", dark = "#3794ff" }
```

```toml
[ui]
accent_color = "#8b5cf6"
```

The currently configurable color is `ui.accent_color`. Opacity settings remain
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
| `panels.diff.shared_scrollbar_height` | `12` | Fixed shared thumb height in pixels; 1–1000, clipped to the track. Spans the full fluid gutter width. Applies live. |
| `panels.diff.shared_scrollbar_opacity` | `0.12` | Shared thumb opacity; 0–1, brighter on hover/drag. Zero hides the thumb while keeping the gutter interactive. Applies live. |
| `panels.diff.gutter_width` | `48` | Fluid connector-gutter width; 8–240. |
| `panels.diff.min_code_width` | `100` | Minimum width of each Fluid code pane; 32–2000. The initial before/after split is even. |

### `history`

| Setting | Default | Meaning and allowed values |
| --- | --- | --- |
| `history.stripe_opacity` | `0.08` | Alternate-row text-tint opacity; 0–0.2. `0` disables zebra stripes. |
| `history.lane_spacing` | `8` | Distance between graph lanes; 4–48. The graph column adapts to visible lane sprawl. |
| `history.show_reference_labels` | `true` | Show compact bookmark/branch badges. Does not hide history from any remote. |
| `history.reference_badge_width` | `110` | Maximum compact reference-badge width; 40–300. Full names remain available in reference details. |
| `history.row_height` | omitted | Override ui.row_height for graph rows; 12–200. Font size can raise the effective minimum. |

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
| `live.enabled` | `true` | Follow recorded repository updates in the background. Does not enable fetching, import external Git changes into jj, or snapshot working files. Independent of settings reload. |
| `live.poll_interval_ms` | `1500` | Recorded repository refresh polling interval; integer 250–60000. |

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
