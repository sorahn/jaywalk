# Jaywalk

A native Git and Jujutsu diff viewer for macOS and Linux. Review a whole branch, an
individual commit, or the evolution of a jj change without leaving the viewer.

- Switch between inline, side-by-side, and Fluid diffs. Fluid keeps each file
  at its natural height and connects corresponding changes as you scroll.
- Browse commit history, local and remote references, and changed or unchanged
  files in a list or directory tree.
- Review recorded jj rewrites with separate change and file evolution timelines.
- Read a complete changeset as a collapsed overview, then open a file for a closer look.
- Adjust panels, fonts, wrapping, and keyboard shortcuts through live TOML settings.

Jaywalk is in early development. macOS builds use a local app bundle; Fedora
Linux builds can be installed as a local RPM, including a desktop launcher and
the `jw` command. There is no published installer or notarized macOS release yet.
See the [development guide](docs/development.md) for building and installing.

## Open a comparison

```sh
jw                                      # welcome page and recent repositories
jw .                                    # current Git or jj repository
jw /path/to/repository                  # another repository
jw . --base REV --tip REV                # explicit revision endpoints
jw diff before.txt after.txt            # two files
jw diff /path/to/before /path/to/after    # two directory trees
```

You can also open repositories or compare files from the welcome page.
Ctrl+R searches your recent repositories. Reopening a repository focuses its
existing window; Command+Enter in the recent picker opens a separate window.
On Linux, use Ctrl+Shift+R for recents and Ctrl in place of Command in shortcuts.
Other repositories open additional windows in the same application.

A repository opens to the whole-branch overview. Select a commit to review its
changes, or select a file to open the diff viewer. **Whole branch** returns to
the comparison you opened. In jj repositories, **Evolution** lets you inspect
recorded versions of a change and what changed between rewrites.

Jaywalk reads local repository data. It does not commit, snapshot your working
copy, fetch remotes, or resolve merges. jj comparisons use recorded snapshots;
plain Git comparisons include staged, unstaged, and untracked worktree files.
See the [usage guide](docs/usage.md) for comparison behavior, live updates, and
integration with `jj diff` and `git difftool`.

## Get around

| Action | macOS shortcut |
| --- | --- |
| Find any file in the selected revision | Cmd+P |
| Command palette | Cmd+Shift+P |
| Recent repositories | Ctrl+R |
| Previous / next change | Cmd+Up / Cmd+Down |
| Previous / next file | Cmd+[ / Cmd+] |
| Previous / next history entry | Cmd+Shift+[ / Cmd+Shift+] |
| Back / forward through visited views | Cmd+Option+Left / Cmd+Option+Right |
| Whole branch | Cmd+Shift+B |
| Settings | Cmd+, |

All shortcuts are remappable. The [keybindings guide](docs/keybindings.md)
lists every command, its defaults, and how to customize application and widget bindings.

## Make it yours

Cmd+, opens `~/.config/jaywalk/config.toml`. Settings apply live; invalid edits
keep the last working configuration. With `XDG_CONFIG_HOME`, configuration lives
under `$XDG_CONFIG_HOME/jaywalk/` instead.

Keybinding overrides live in a separate TOML file, selected from the config:

```toml
[keyboard]
profile = "keybindings/default.toml"
```

First use creates sparse, commented configuration files without overwriting
existing files. **Open keybindings** edits the active profile, and **Select
keybindings profile…** switches profiles from the command palette. Profile files
can live anywhere; relative paths resolve beside the config.

Panel sizes, placement, and other interface choices are remembered per repository
by default. Use **Reset repository interface choices** to restore configured defaults.

- [Settings reference](docs/settings.md)
- [Keybindings and profiles](docs/keybindings.md)
- [Usage and diff-tool integration](docs/usage.md)
- [Development](docs/development.md)

## License

Jaywalk's original code is [GPL-3.0-only](LICENSE). Third-party code retains its
own licenses, including the Apache-2.0 licensed vendored components; see
[vendor/README.md](vendor/README.md).

See [context menus](docs/context-menus.md) for panel, file and history actions.
