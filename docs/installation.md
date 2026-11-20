# Installing Jaywalk on macOS

The release pipeline is being prepared. No official notarized download or Homebrew tap is published yet. Unsigned rehearsal artifacts are for testing, not normal installation.

## Drag-and-drop installation

Open the release DMG and drag Jaywalk to Applications (or a permanent user-owned applications folder). Eject the DMG and launch the installed copy. First-open setup checks for an existing executable `jw` in the background before showing a compact two-column form. Its command-line checkbox is checked when a launcher is found; the path is shown, with a terminal-PATH reminder for common folders missing from Finder's PATH. Existing commands are inspected without executing them, and the first executable on PATH takes precedence. Setup offers CLI integration, update preferences, appearance and optional getting-started actions. At normal window sizes the choices fit without scrolling; narrow windows stack and short windows scroll. You can skip it and reopen **Set up Jaywalk…** from the palette or Jaywalk menu. Local viewing requires no hosting account.

## Install the jw command

Choose **Install or check jw command…** from setup, the command palette or the Jaywalk menu. It checks the app's PATH and common locations, recognizes links to the current app, and offers `~/.local/bin` or an existing directory you choose. It never overwrites another command, broken link or another installation. If the chosen directory is not writable, choose another location or use the manual instructions. It does not change shell configuration or ask for administrator privileges.

For the usual Applications installation, the manual equivalent is:

```sh
mkdir -p "$HOME/.local/bin"
ln -s "/Applications/Jaywalk.app/Contents/MacOS/jw" "$HOME/.local/bin/jw"
```

For a custom location, change the destination directory. If the app lives elsewhere, adjust its source path too. `ln -s` deliberately refuses an existing destination. Inspect it first rather than replacing an unrelated command. The app offers Copy manual instructions with its actual bundle path.

Put that directory on your interactive shell's PATH. For fish:

```fish
fish_add_path "$HOME/.local/bin"
```

For zsh, add this to `~/.zshrc` (bash: `~/.bashrc`):

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Open a new terminal and verify:

```sh
command -v jw
jw --version
jw .
```

Finder's inherited PATH can differ from your terminal's, so installation status alone cannot establish shell readiness. If another `jw` wins PATH precedence, inspect `type -a jw` and adjust your own PATH.

The link targets the installed bundle, so replacing that app in place updates the CLI too. Moving the app breaks the old link: inspect/remove that link and reinstall from the new location. Do not install links into a mounted DMG or App Translocation folder; the installer refuses these temporary locations.

To remove only the CLI link, first verify `test -L "$HOME/.local/bin/jw"` and inspect `readlink "$HOME/.local/bin/jw"`, then remove that link with `rm "$HOME/.local/bin/jw"`. Removing the app/CLI does not remove personal configuration. Homebrew installs create their own link; use Homebrew to manage that installation rather than replacing its link through the app.

## Updates

Update checks and downloads are off by default. First-open setup records explicit choices in the normal config. Skipping does not grant consent, even after changing an unsaved checkbox. Existing users with no recorded choice are not automatically opted in.

**Check for updates…** explicitly checks the release feed even when scheduled checks are disabled. Official macOS builds include Sparkle; ordinary development and unsigned rehearsal builds have no configured feed/key and make no update requests. Homebrew and direct installs support the same in-app flow; the cask declares `auto_updates true`. Homebrew upgrades remain available.

Automatic checking, downloading and installation are separate choices. Download only stores a raw archive without extracting it or scheduling an installer, even on quit. The initial implementation re-fetches and verifies the release through Sparkle when you later choose Install update; it does not reuse the download-only cache for installation. A staged archive is not an installed/verified update. One raw archive is retained under `~/Library/Caches/dev.jaywalk.Jaywalk/updates/`; it can be deleted while the app is closed.

Explicit installation uses Sparkle's verified progress and restart UI. Automatic installation on quit requires its own opt-in. Waiting CLI clients and active local-review checkout leases gate installation/restart, so finish those sessions and restore review checkouts first. Signed old-to-new installation, cancellation, crash recovery and Homebrew coexistence still require release acceptance; compiling the adapter does not establish those behaviors.
