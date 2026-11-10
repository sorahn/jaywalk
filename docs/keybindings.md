# Keybindings

Jaywalk uses the same commands for shortcuts, the command palette and native
menus. All application shortcuts and framework/widget bindings can be remapped
or disabled in a separate TOML keybindings profile, selected by your
[configuration](settings.md#keyboard-profiles-keyboard). Updates apply live, and
menus, command-palette hints and action-aware tooltips follow the active keymap.
The first shortcut in an array is the primary one shown in hints.

On Linux, `secondary` means Ctrl. The shipped `[platform.linux]` defaults use
Ctrl+Shift+R for refresh (leaving Ctrl+R for recent repositories) and F11 for
fullscreen. They apply before sparse user profiles, including profiles containing
only a few overrides. macOS retains Ctrl+R for recents and Ctrl+Cmd+F for fullscreen.
The tables below show the common/macOS defaults; these two Linux defaults replace
the corresponding entries. User overrides always take precedence over shipped defaults. Profiles can also
contain `[platform.linux]` or `[platform.macos]` command tables; the matching table
overrides that file's common `[keybindings]` values, and other tables are ignored.
The shipped keybindings file can be copied or selected directly as a profile.

The shipped platform-specific entries are:

| Profile key | Default | Meaning |
| --- | --- | --- |
| `platform.linux.refresh` | `["secondary-shift-r"]` | Linux refresh shortcut. |
| `platform.linux.fullscreen` | `["f11"]` | Linux fullscreen shortcut. |

## Files and profile selection

On first use, Jaywalk creates `~/.config/jaywalk/config.toml` and a sparse
`~/.config/jaywalk/keybindings/default.toml` if they do not exist. With
`XDG_CONFIG_HOME`, both live beneath `$XDG_CONFIG_HOME/jaywalk/`. The generated
files contain examples rather than copies of every shipped default, so new defaults
can take effect after updates. Existing files are never overwritten.

Keep application settings in the main config:

```toml
[keyboard]
profile = "keybindings/default.toml"
additional_directories = ["../shared/keys"]
```

The folder is a convention, not a requirement. `profile` accepts any relative or
absolute file path; relative paths and extra discovery folders resolve beside the
active config, including an explicit `--config`. Paths do not expand `~` or variables.
Put the `[keybindings]` and `[[keymap]]` examples below in that selected file, not
in `config.toml`. Unspecified shortcuts inherit [shipped defaults](../config/keybindings.toml).

Cmd+, edits the main config. **Open keybindings** edits the active profile through
`settings.editor`. **Select keybindings profile…** lists `.toml` files in the
recommended folder and additional discovery folders, plus the current profile if
it lives elsewhere. It marks the active entry; typing filters paths, Enter selects,
and Escape cancels. Create another TOML file in one of these folders to add a profile.
Invalid profiles remain listed, but cannot replace the working keymap. Switching
saves `keyboard.profile`, preserving config comments; edits to the selected file
apply live. Missing or malformed files preserve the last valid bindings and report
an error. Both new commands start unbound and can be assigned shortcuts in a profile.
For example, `open_keybindings = ["secondary-k secondary-s"]` inside `[keybindings]`.

## Default review navigation

| Action | macOS shortcut | Alternatives |
| --- | --- | --- |
| Back / forward through visited views | Cmd+Option+Left / Cmd+Option+Right | Mouse navigation buttons; toolbar arrows |
| Previous / next change | Cmd+Up / Cmd+Down | Option+Shift+F5 / Option+F5; Shift+F7 / F7 |
| Previous / next file | Cmd+[ / Cmd+] | Option+Up / Option+Down |
| Previous / next history entry | Cmd+Shift+[ / Cmd+Shift+] | Cmd+Option+Up / Cmd+Option+Down |
| Whole branch | Cmd+Shift+B | |
| Overview of the current comparison | Cmd+K, then O | |
| Find any repository file | Cmd+P | |
| Command palette | Cmd+Shift+P | F1 |
| Open recent repository | Ctrl+R | File → Open recent repository; welcome More… |
| Open settings | Cmd+, | |
| Toggle wrap | Option+Z | Cmd+K, then Z |
| Toggle line numbers | Cmd+; | |
| Inline / side-by-side / Fluid | Cmd+K, then 1 / 2 / 3 | |

The overview is the position before the first entry in the visible history
list. Cmd+Shift+] from whole-branch overview selects that first entry;
Cmd+Shift+[ from the first entry returns to whole-branch overview. Navigation
does not wrap at either end. Selecting a revision follows the graph's visible
ordering and its current remote filters.

Cmd+Up/Down review navigation overrides source movement in the read-only Fluid
editors and standard diff, including overview. Editable search/picker inputs
retain their text-navigation behavior. Copy, selection and ordinary cursor
movement retain the framework's normal context-sensitive behavior unless you
remap them explicitly. macOS-reserved shortcuts, text composition and system
application switching are outside Jaywalk's keymap.

Back and Forward retrace visited comparisons, overviews and files in this window,
including evolution version/mode selections. They are separate from next/previous
file or graph ordering. Returning restores the recorded comparison and reading
position where possible, including retained files absent from that comparison.
Overview disclosures remain shared with their retained diff model; scrolling
restores a bounded list position. Restored views stay pinned until you Refresh
or explicitly select another comparison. Scrolling, repeated selection and live
refresh do not create entries; leaving a restored location for a new destination
clears the forward path. Back during a new pending load cancels it and restores
the last displayed destination; unfinished destinations are not recorded.
History is in memory, per window, capped by `navigation.history_limit`.

Ctrl+R works from the welcome screen and repository views. It searches all saved
recent repositories, independent of the welcome preview count. An empty query preserves most-recent-first ordering; typing filters names
and paths. Enter opens the selected repository and Escape dismisses the picker.
Cmd+Enter (`secondary-enter`, remappable as `open_in_new_window`) opens the selected
result in a new window in the same app process, even if that repository is already
open. Command-click does the same on picker results and welcome recent entries.
The shortcut is active only inside the recent picker, including its Open repository
fallback; it does not change Enter in other pickers. Ordinary Enter/click keeps
the usual window reuse behavior.
Opening an already-open matching repository focuses its window without resetting
its selection. With no viewer window, the command opens a welcome window first.
The picker always offers Open repository as a fallback. Welcome, Compare files
or folders, and Clear recent repositories are also remappable commands; they
start unbound. Repository-only commands are unavailable in welcome mode.

## Key notation

Use GPUI notation in strings:

- `secondary` is Cmd on macOS and Ctrl elsewhere.
- `ctrl`, `alt` and `shift` specify those modifiers explicitly. `alt` is Option
  on macOS. This remains a macOS-first application; these aliases do not imply
  every platform integration is implemented.
- Hyphens join modifiers and a key, such as `"secondary-shift-p"`.
- Shifted punctuation uses the produced character in GPUI's macOS event format:
  `"secondary-}"` represents Cmd+Shift+] and `"secondary-{"` represents
  Cmd+Shift+[ on a US-style layout. This is why the history defaults use braces;
  do not spell these as `"secondary-shift-]"` or `"secondary-shift-["`.
  Character placement can differ with your keyboard layout.
- Spaces separate chord strokes: `"secondary-k 1"` is Cmd+K followed by 1,
  without holding Cmd for the second stroke.
- Multiple strings give alternatives, not a chord:
  `["secondary-down", "f7"]` binds either shortcut to the same command.
- A chord prefix can delay a matching single-stroke shortcut; avoid assigning
  an immediate command to the prefix of another chord unless that is intended.

## Remap application commands: `[keybindings]`

Map a command ID to an array of shortcuts. An override replaces that command's
entire array; unspecified commands inherit defaults. `[]` removes every
shortcut for that command, while leaving its menu/palette action available.
Remove the setting to restore defaults. Use `[]`, not `[""]`, to unbind.

For example, use only F7 for changes and disable the history-panel shortcut:

```toml
[keybindings]
next_change = ["f7"]
previous_change = ["shift-f7"]
toggle_history = []
```

Application shortcuts are active in repository windows, with context-specific
source overrides for change navigation. Application-level commands such as
Quit and Open repository also work without a repository window. Some commands
require suitable content: an evolution command needs jj metadata, for example.
Unbound commands remain remappable; no artificial default is assigned to every
layout/reset/remote-management command merely to fill the table.

| Setting | Default | Command |
| --- | --- | --- |
| `keybindings.about` | `[]` | About Jaywalk |
| `keybindings.open_repository` | `["secondary-o"]` | Open repository… |
| `keybindings.open_recent` | `["ctrl-r"]` | Open recent repository |
| `keybindings.open_in_new_window` | `["secondary-enter"]` | Open selected recent repository in a new window (recent picker only) |
| `keybindings.compare_paths` | `[]` | Compare files or folders |
| `keybindings.show_welcome` | `[]` | Welcome |
| `keybindings.clear_recent` | `[]` | Clear recent repositories |
| `keybindings.find_file` | `["secondary-p"]` | Find any repository file |
| `keybindings.show_commands` | `["secondary-shift-p", "f1"]` | Command palette |
| `keybindings.open_settings` | `["secondary-,"]` | Open settings |
| `keybindings.open_keybindings` | `[]` | Open active keybindings profile |
| `keybindings.select_keybindings` | `[]` | Select keybindings profile… |
| `keybindings.version_patch` | `["secondary-k p"]` | Version patch |
| `keybindings.since_previous` | `["secondary-k s"]` | Since previous version |
| `keybindings.refresh` | `["secondary-r"]` | Refresh recorded changes |
| `keybindings.whole_branch` | `["secondary-shift-b"]` | Whole branch |
| `keybindings.overview` | `["secondary-k o"]` | Changeset overview |
| `keybindings.show_references` | `["secondary-k r"]` | Browse references |
| `keybindings.toggle_reference_labels` | `[]` | Toggle reference labels |
| `keybindings.show_all_remotes` | `[]` | Show all remote history |
| `keybindings.hide_all_remotes` | `[]` | Hide all remote history |
| `keybindings.reveal_selected_revision` | `["secondary-k h"]` | Reveal inspected revision in history |
| `keybindings.toggle_history` | `["secondary-b"]` | Toggle history panel |
| `keybindings.toggle_files` | `["secondary-shift-e"]` | Toggle changed files panel |
| `keybindings.toggle_change_evolution` | `["secondary-k e"]` | Toggle change evolution |
| `keybindings.toggle_file_evolution` | `["secondary-k f"]` | Toggle file evolution |
| `keybindings.toggle_wrap` | `["alt-z", "secondary-k z"]` | Toggle word wrap |
| `keybindings.toggle_line_numbers` | `["secondary-;"]` | Toggle line numbers |
| `keybindings.view_inline` | `["secondary-k 1"]` | View: inline |
| `keybindings.view_side_by_side` | `["secondary-k 2"]` | View: side by side |
| `keybindings.view_fluid` | `["secondary-k 3"]` | View: Fluid |
| `keybindings.go_back` | `["secondary-alt-left"]` | Back through visited views |
| `keybindings.go_forward` | `["secondary-alt-right"]` | Forward through visited views |
| `keybindings.next_change` | `["secondary-down", "alt-f5", "f7"]` | Next change |
| `keybindings.previous_change` | `["secondary-up", "alt-shift-f5", "shift-f7"]` | Previous change |
| `keybindings.next_file` | `["secondary-]", "alt-down"]` | Next file |
| `keybindings.previous_file` | `["secondary-[", "alt-up"]` | Previous file |
| `keybindings.next_revision` | `["secondary-}", "secondary-alt-down"]` | Next revision |
| `keybindings.previous_revision` | `["secondary-{", "secondary-alt-up"]` | Previous revision |
| `keybindings.zoom_in` | `["secondary-="]` | Increase font size |
| `keybindings.zoom_out` | `["secondary--"]` | Decrease font size |
| `keybindings.reset_zoom` | `["secondary-0"]` | Reset font size |
| `keybindings.reset_layout` | `[]` | Reset panel layout |
| `keybindings.history_left` | `[]` | Move history to left |
| `keybindings.history_right` | `[]` | Move history to right |
| `keybindings.files_left` | `[]` | Move changed files to left |
| `keybindings.files_right` | `[]` | Move changed files to right |
| `keybindings.swap_sidebars` | `[]` | Swap sidebar order |
| `keybindings.change_evolution_sidebar` | `[]` | Dock change evolution below history |
| `keybindings.change_evolution_bottom` | `[]` | Dock change evolution at bottom |
| `keybindings.file_evolution_sidebar` | `[]` | Dock file evolution below changed files |
| `keybindings.file_evolution_bottom` | `[]` | Dock file evolution at bottom |
| `keybindings.reset_interface` | `[]` | Reset repository interface choices |
| `keybindings.quit` | `["secondary-q"]` | Quit Jaywalk |
| `keybindings.close_window` | `["secondary-w"]` | Close window |
| `keybindings.minimize` | `["secondary-m"]` | Minimize |
| `keybindings.fullscreen` | `["ctrl-secondary-f"]` | Toggle full screen |
| `keybindings.hide` | `["secondary-h"]` | Hide Jaywalk |
| `keybindings.hide_others` | `["secondary-alt-h"]` | Hide others |
| `keybindings.show_all` | `[]` | Show all |

## Remap widget shortcuts: `[[keymap]]`

Framework actions cover source editing/navigation, clipboard operations,
selection, tree navigation, palette confirmation/cancellation and other
controls. These are separate from application commands. Advanced overrides
rebind existing actions in their original contexts, preserving their action
parameters; they do not invent new actions or change a control's behavior.

| Setting | Default | Meaning |
| --- | --- | --- |
| `keymap` | `[]` | Advanced widget-action overrides in the profile. Entries require `action` and `keys`; optional `context` and `from` select original bindings. |

Each `[[keymap]]` entry has these fields:

| Field | Required | Meaning |
| --- | --- | --- |
| action | Yes | Exact framework action name, such as `"input::Copy"` or `"ui::SelectDown"`. |
| keys | Yes | Replacement shortcut array. `[]` disables matching defaults. |
| context | No | Match one exact original context predicate. Omitted matches every original context for this action; `""` selects global bindings. |
| from | No | Select one original shortcut, including a complete chord. Useful for actions whose original shortcuts carry different parameters. |

If you copied the complete shipped keyboard defaults, remove their top-level `keymap = []`
before adding `[[keymap]]` entries; TOML cannot define the same array twice.
For a normal overrides-only file, just add the entries below. To restore every
widget default, remove all `[[keymap]]` entries or set `keymap = []` at the root
of the file, before any section headers. The array replaces defaults as a whole.

### Copy without changing where it works

This replaces the default Copy shortcuts in every original context, including
source, diff and text input. It keeps the focus predicates rather than making
Copy a global action everywhere:

```toml
[[keymap]]
action = "input::Copy"
keys = ["secondary-insert"]
```

### Tree-only navigation

Limit plain J/K keys to the tree so they do not consume text typed into a search
field or palette:

```toml
[[keymap]]
action = "ui::SelectDown"
context = "Tree"
keys = ["j"]

[[keymap]]
action = "ui::SelectUp"
context = "Tree"
keys = ["k"]
```

### Replace one parameterized shortcut

Some actions have several defaults with different parameters. Selecting the
original shortcut preserves its payload and leaves other variants unchanged:

```toml
[[keymap]]
action = "ui::Confirm"
context = "Command"
from = "enter"
keys = ["ctrl-enter"]
```

Use a matching original predicate, not a new desired scope. `context = "Input"`
selects defaults already bound in `Input`; it does not move bindings from
`Diff` into `Input`. Predicates use GPUI expressions (`&&`, `||`, `!`, `>` and
context values), but a selector must normalize to an existing predicate.
`from` refers to the pristine default shortcut, even after earlier reloads.

### Common framework actions and contexts

| Actions | Typical original context | Purpose |
| --- | --- | --- |
| `input::Copy`, `input::SelectAll` | `Input`, `Diff`, and other registered contexts | Clipboard copy and select all; omit context to update every default. |
| `input::MoveLeft`, `input::MoveRight`, `input::MoveUp`, `input::MoveDown` | `Input` | Cursor navigation. |
| `input::MoveToStart`, `input::MoveToEnd` | `Input` | Start/end of source or text. Review change bindings take precedence in read-only source. |
| `input::Search` | `Input` | The input component's search action where supported. This does not add search to views without that feature. |
| `ui::SelectUp`, `ui::SelectDown` | `Tree`, `Command` and other controls | Navigate rows. |
| `ui::SelectLeft`, `ui::SelectRight` | `Tree` | Collapse/expand or navigate tree hierarchy. |
| `ui::Confirm` | `Command`, `Tree` and other controls | Confirm a choice; use `from` for parameterized variants. |
| `ui::Cancel` | `Command` and other controls | Cancel/dismiss. |
| `diff::ScrollUp`, `diff::ScrollDown`, `diff::ScrollLeft`, `diff::ScrollRight` | `Diff` | Scroll the standard/overview diff. |
| `diff::PageUp`, `diff::PageDown`, `diff::First`, `diff::Last` | `Diff` | Page or endpoint navigation. |
| `diff::ExtendUp`, `diff::ExtendDown` | `Diff` | Extend source-line selection. |

Framework action names and original predicates follow the bundled GPUI Kit
version. A rule that matches no original binding is an error, not a silently
ignored customization. Other registered widget actions can be remapped using
the same mechanism; the list above is a guide to common controls, not a whitelist.

## Validation, priority and live updates

- Invalid keystrokes/context expressions, unknown fields, empty shortcut strings
  and duplicate application shortcuts assigned to different commands reject the
  settings update. Unknown application command IDs are errors too.
- Advanced rules are resolved against the app's actual toolkit defaults before
  installing them. Unknown action/context/from combinations, overlapping rules,
  or conflicting replacement keys in an identical context produce an error.
  Unbind the competing action or narrow the rule with `context`/`from`.
- Contexts can intentionally reuse keys. A plain arrow in a search input need
  not do the same thing as that arrow in a tree. More specific/deeper widget
  contexts take priority over application-wide commands. Review-specific
  bindings override normal movement only in read-only source/diff contexts.
- A rejected live update keeps the previous configuration and keymap active.
  A startup error is reported through launch diagnostics; use `--foreground`
  when investigating it. New-window requests with invalid rules are rejected
  without replacing the active keymap.
- Reload starts from retained pristine widget defaults, not the already-remapped
  map. Removing a rule restores the original binding. Later component bindings
  are incorporated when the keymap version changes; the app does not discard
  them when rebuilding application shortcuts.
- The keymap is application-wide. Opening with another config or saving changed
  bindings applies that map to the shared app process. It is not part of the
  remembered per-repository interface state.

## Why these defaults

The navigation grouping is deliberate: Cmd+arrows moves within a diff,
Cmd+brackets moves between files, and adding Shift moves through history.
The overview is the position before history entry one. These are Jaywalk's
chosen defaults, not a claim that every diff app agrees.

The comparison checked shipped resources from Fork 2.71.1, Kaleidoscope 3.9 and
Sublime Merge Build 2132 without launching them, plus official shortcut references.
This compares shipped defaults, not personal overrides or every dynamic menu.

- Fork, Kaleidoscope and Sublime Merge share Cmd+F / Cmd+G / Cmd+Shift+G for
  find/next/previous match. Cmd+, is the familiar settings shortcut.
- Fork and Sublime Merge use Cmd+P for quick launch/palette-style access, while
  Jaywalk keeps the VS Code/Zed distinction: Cmd+P for files, Cmd+Shift+P for
  commands.
- Kaleidoscope documents Cmd+Down/Up for change navigation. VS Code uses
  Option+F5 / Option+Shift+F5, with F7-family accessible diff navigation. Jaywalk
  uses Cmd+arrows as primary and retains the existing F5/F7 alternatives.
- File/history navigation varies. Kaleidoscope's installed menu assigns
  Ctrl+Cmd+Up/Down to files; Sublime Merge assigns Cmd+Shift+brackets to file
  tabs and Option+Up/Down to commit ancestry. Jaywalk follows its explicit
  grouping above instead of treating either convention as universal.
- Zed's Cmd+; for line numbers and Cmd+K, Z for wrapping are useful precedents;
  Option+Z remains the VS Code-style wrap shortcut. The Cmd+K number/evolution
  chords are Jaywalk-specific choices. Less frequent placement/reset commands
  stay unbound until the user assigns them.

References:

- [Fork's official macOS shortcuts](https://github.com/fork-dev/Docs/blob/master/keyboard-shortcuts-mac.md).
- [Kaleidoscope change navigation](https://blog.kaleidoscope.app/2024/12/30/navigating-changes-and-conflicts/).
- [Sublime Merge key bindings](https://www.sublimemerge.com/docs/key_bindings), including where its packaged defaults live.
- [VS Code's default macOS keybindings](https://github.com/microsoft/vscode-docs/blob/main/build/keybindings/doc.keybindings.osx.json).
- [Zed's default macOS keymap](https://github.com/zed-industries/zed/blob/main/assets/keymaps/default-macos.json).

## Panel and context commands

These commands are unbound by default and can be assigned in a profile. File
actions use the active file from keyboard/palette; row menus retain their own
subject. See [context menus](context-menus.md).

| Profile key | Default | Meaning |
| --- | --- | --- |
| `keybindings.open_file` | `[]` | Open file diff |
| `keybindings.copy_path` | `[]` | Copy working-tree path |
| `keybindings.copy_relative_path` | `[]` | Copy relative path |
| `keybindings.copy_file_name` | `[]` | Copy file name |
| `keybindings.reveal_in_file_manager` | `[]` | Reveal working-tree location |
| `keybindings.collapse_files` | `[]` | Collapse all folders |
| `keybindings.expand_files` | `[]` | Expand all folders |
| `keybindings.reveal_file` | `[]` | Reveal active file in sidebar |
| `keybindings.file_list` | `[]` | Show file list |
| `keybindings.file_tree` | `[]` | Show directory tree |
| `keybindings.tree_changed` | `[]` | Show changed files |
| `keybindings.tree_all` | `[]` | Show all repository files |
