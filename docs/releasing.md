# macOS releases

This is the operator guide for the initial Apple Silicon/macOS 15+ release path. No public release, tap, feed or production signing credentials have been provisioned by these changes. Intel and Linux release channels remain separate work. Personal design/checklists live in the Obsidian packaging plan.

## Credential-free validation

GitHub's standard macOS runners are free for this public repository. Workflows use `macos-15` ARM64 and an explicit Xcode 26.1.1 path because the runner's default Xcode 16.4 cannot compile the Icon Composer asset. Rust is pinned to 1.100.0; Cargo dependencies stay locked. Checks and official release jobs install checksum-pinned jj 0.47.1 into a job-local tool directory and verify Git/Clang availability before compiling. jj is required for the test suite, but remains optional for plain-Git users and is not bundled into the app. GitHub-hosted images roll forward; record the actual image/Xcode/Rust versions in the run logs and update the explicit Xcode path deliberately if images retire it.

`checks.yml` runs metadata tests, Rust formatting/tests/strict Clippy and updater-enabled compilation on PRs without signing credentials. `macos-release.yml` is manually dispatched: Rehearsal produces visibly unsigned artifacts with no update feed or key. Official mode requires a `v<Cargo version>` tag reachable from main, protected `macos-release` environment and the inputs below. It rechecks source and creates a **draft** release only. Neither workflow updates Homebrew or the feed host automatically. No `pull_request_target` or privileged fork-artifact signing is used.

Local checks:

```sh
just check
python3 -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/fetch-sparkle.py target/sparkle
JAYWALK_SPARKLE_DIR="$PWD/target/sparkle" just test-updater
JAYWALK_SPARKLE_DIR="$PWD/target/sparkle" DYLD_FRAMEWORK_PATH="$PWD/target/sparkle" just test --features macos-updater
```

SDK downloads are build-tool traffic explicitly invoked by the developer/CI, unrelated to app update consent. The official SDK version/URL/SHA256 live in `packaging/macos/sparkle.json`; the fetcher verifies the archive before extraction. Sparkle is BSD licensed. Ordinary development builds do not require it. Opt in through `JAYWALK_MACOS_UPDATER=1`; Cargo's `macos-updater` feature also requires the SDK directory at build time.

An unsigned rehearsal without replacing the installed development app:

```sh
JAYWALK_SPARKLE_DIR="$PWD/target/sparkle" \
JAYWALK_MACOS_UPDATER=1 JAYWALK_SIGNING_IDENTITY=- \
JAYWALK_BUNDLE_OUTPUT="$PWD/target/rehearsal/Jaywalk.app" \
JAYWALK_REGISTER_BUNDLE=0 just bundle
python3 scripts/release-macos.py package target/rehearsal/Jaywalk.app target/rehearsal-assets
python3 scripts/release-macos.py manifest target/rehearsal/Jaywalk.app target/rehearsal-assets/*.zip target/rehearsal-assets/*.dmg
```

Use a fresh output directory for each packaging rehearsal; existing versioned payloads are never overwritten. A rehearsal manifest has `official:false`; cask generation rejects it. `scripts/release-macos.py check-tag v0.1.1` validates the tag against Cargo.toml. Packaging validates the native ARM64 slice, preserves framework symlinks, includes the app/CLI and a DMG Applications link. Final checksums are produced after all signing/notarization/stapling operations. Never label an ad hoc artifact an official signed release.

## Provisioning later

Create a GitHub environment named `macos-release`, restrict it to version tags and require owner approval before running the official job. Review the exact workflow and tagged code before granting access. Do not enable production credentials for arbitrary branches/forks.

| Environment input | Kind | Purpose |
| --- | --- | --- |
| `MACOS_CERTIFICATE_P12_BASE64` | Secret | Exported Developer ID Application certificate **and private key** |
| `MACOS_CERTIFICATE_PASSWORD` | Secret | Password for that export |
| `MACOS_SIGNING_IDENTITY` | Variable | Intended Developer ID certificate fingerprint |
| `APPLE_NOTARY_KEY_BASE64` | Secret | Notarization API key `.p8` contents, base64 encoded |
| `APPLE_NOTARY_KEY_ID` | Variable | API key identifier |
| `APPLE_NOTARY_ISSUER_ID` | Variable | API key issuer |
| `SPARKLE_PRIVATE_KEY` | Secret | Sparkle EdDSA private key export (file content, not a command-line argument) |
| `SPARKLE_PUBLIC_KEY` | Variable | Matching public EdDSA key included in the app |
| `UPDATE_FEED_URL` | Variable | HTTPS URL of the signed public appcast |

Enter/export values locally and upload through secure tooling; do not send secrets in chat or commit them. Keep encrypted backups and document rotation. Development builds keep the Apple Development identity; public builds use Developer ID Application. CI uses an ephemeral Keychain and deletes its temporary key files/Keychain on exit. No ACL/trust changes are made to the developer's login Keychain.

Official signing requires an explicit identity and hardened runtime/timestamp. The app, launcher and Sparkle's nested helpers/framework are signed inside-out. Submit/staple the app, package it, sign/submit/staple the DMG, then verify signatures, Gatekeeper and tickets before computing final artifact metadata. The manifest records version, exact source commit, platform/architecture, minimum OS and final sizes/hashes.

`generate_appcast` signs the archive enclosure and feed with the separate EdDSA key. Only the canonical ZIP is fed to it; the DMG is a separate install artifact. Preserve existing published artifacts for rollback. A release job creates draft assets and a generated `jaywalk.rb`; it does not create a tap or deploy a feed. Before publishing, review signed assets/manifest/feed/cask and fulfill license/source/notice requirements for redistributed dependencies. The bundle collects resolved dependency metadata and available license/notice files under Resources/licenses, without local source paths; this inventory is an audit aid, not a claim that every dependency's redistribution obligations have been fulfilled.

## Native update integration

The small Objective-C bridge uses the pinned Sparkle framework; Rust owns public TOML preferences and opt-in. It does not read OAuth credentials. Automatic checks require both `setup.completed=true` and `updates.automatic_checks=true`. Manual checks require an explicit action. No telemetry/profile or release-note fetches are enabled. Update enclosures come from this repository's GitHub release paths; download-only redirects are limited to GitHub's release asset CDN.

Download-only deliberately avoids Sparkle's combined automatic-download/install setting: that setting can schedule installation on quit. Its raw cache never extracts or executes content; later installation goes through Sparkle verification and currently re-fetches the payload. The consent UI documents this distinction. Automatic installation is enabled only with its own opt-in and restart guard. A live query checks waiting CLI clients and local checkout leases at install/relaunch boundaries. The ordinary guarded restoration path still runs on app quit; interrupted receipts remain recoverable.

The no-window native probe verifies absence of consent/configuration does not instantiate the updater and checks the download URL restrictions. It does **not** verify a signed real update, progress UI, download cache, restart/termination hooks or credentials across a replacement.

## First promotion checklist

- Run the workflow on an owner-reviewed version tag and inspect the draft outputs.
- Validate signed app/DMG with `codesign`, `stapler` and `spctl` on a clean Mac.
- Exercise first-run consent (including skip/offline/disable), CLI setup and custom paths, Finder/terminal Git/jj discovery, paths containing spaces/non-ASCII and existing-command conflicts.
- Exercise old-to-new updates under every check/download/install combination, including download-only followed by quit/relaunch, active CLI waiters and local checkout receipts, canceled/interrupted downloads, and rejected/tampered feeds/archives.
- Create the public tap (proposed `sorahn/homebrew-tap`) and use the generated cask; run `brew style`, `brew audit --cask`, install/upgrade/reinstall/uninstall. Confirm app replacement preserves CLI links/config/accounts and that `auto_updates true` coexists with Homebrew's recorded versions.
- Deploy the reviewed signed appcast to its stable HTTPS URL only after its assets are public; do not rebuild or rewrite already signed payloads.
- Promote the reviewed draft release and tap/feed changes explicitly. Public hosting OAuth registrations must also be resolved before claiming those connections work for arbitrary users.
