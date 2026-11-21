# macOS releases

This is the operator guide for the initial Apple Silicon/macOS 15+ release path. Apple signing/notarization and Sparkle credentials are provisioned separately in protected GitHub environments. Public distribution still requires a successful production build and the acceptance checks below. Intel and Linux release channels remain separate work. Personal design/checklists live in the Obsidian packaging plan.

## Credential-free validation

GitHub's standard macOS runners are free for this public repository. Checks use `macos-15` ARM64 with explicit Xcode 26.1.1. Packaging uses `macos-26` ARM64 with explicit Xcode 26.6: the first rehearsal compiled Rust successfully but the older runner/Xcode pairing crashed in Apple's Icon Composer asset runtime. Packaging now runs `scripts/test-macos-icon.sh` before Cargo so an incompatible image fails early. The app deployment target remains macOS 15. Rust is pinned to 1.100.0; Cargo dependencies stay locked. Checks and official release jobs install checksum-pinned jj 0.47.1 into a job-local tool directory and verify Git/Clang availability before compiling. jj is required for the test suite, but remains optional for plain-Git users and is not bundled into the app. GitHub-hosted images roll forward; record the actual image/Xcode/Rust versions in the run logs and update the explicit Xcode path deliberately if images retire it.

`checks.yml` runs metadata tests, Rust formatting/tests/strict Clippy and updater-enabled compilation on PRs without signing credentials. Merging main runs checks only. Pushing a `v<year>.<month>.<release-number>` tag starts `tagged-release.yml`: validate the selected source, build/sign/notarize, verify and publish the GitHub release, then update Homebrew. `macos-release.yml` is the reusable candidate builder; its manual rehearsal remains unsigned with no feed/key. Manual official mode creates a draft only. No `pull_request_target` or privileged fork-artifact signing is used.

Rust jobs use the commit-pinned `Swatinem/rust-cache` action after selecting Rust 1.100.0. It retains Cargo downloads and compiled dependency artifacts, excludes the application/workspace outputs and incremental artifacts, and leaves all tests/checks enabled. Cache keys separate checks, rehearsal and official packaging; they include the toolchain/lockfiles, Apple developer directory/deployment target and a hash of vendored source. Root application build provenance is regenerated for each checkout. The first successful run seeds its cache; subsequent runs may reuse it, including default-branch caches available to PRs. Dependency/toolchain/SDK/vendor changes can cause misses; cache restore/save time counts toward the actual benefit. Signed bundles, notarization credentials and distribution assets are not cached as build dependencies. Cargo-bin caching is disabled because CI tooling is installed explicitly.

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

Use a fresh output directory for each packaging rehearsal; existing versioned payloads are never overwritten. A rehearsal manifest has `official:false`; cask generation rejects it. `scripts/release-macos.py check-tag v2026.11.1` validates the tag against Cargo.toml. Packaging validates the native ARM64 slice, preserves framework symlinks, includes the app/CLI and a DMG Applications link. Final checksums are produced after all signing/notarization/stapling operations. Never label an ad hoc artifact an official signed release.

## Protected production configuration

Restrict `macos-release` and `release-automation` to `main` and version tags. Tag creation is the owner’s release approval; remove recurring environment-reviewer gates after reviewing this pipeline. Keep ordinary branches/forks excluded. The selector checks that the tag resolves to a commit reachable from main and that Cargo.toml and the root Cargo.lock version both match. The build also checks its exact source SHA before using signing credentials.

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

`generate_appcast` signs the archive enclosure and feed with the separate EdDSA key. Only the canonical ZIP is fed to it; the DMG is a separate install artifact. Preserve existing published artifacts for rollback. The candidate job creates draft assets and a generated `jaywalk.rb`. The promotion job downloads and independently verifies their checksums, Developer ID certificate, notarization tickets, ZIP/DMG sealed-app identity, executable provenance, update configuration, Sparkle key pair and signed feed/archive, then checks cask style before publication. Fulfill license/source/notice requirements for redistributed dependencies before shipping. The bundle collects resolved dependency metadata and available license/notice files under Resources/licenses, without local source paths; this inventory is an audit aid, not a claim that every dependency's redistribution obligations have been fulfilled.

## Calendar versions and explicit release tags

Use `year.month.release-number`, without leading zeroes: `2026.11.1`, `2026.11.2`, then `2026.12.1`. The last number counts releases within the month; it does not imply compatibility or a day of the month. Cargo retains its required three numeric components. Set Cargo.toml and the local `jaywalk` package entry in Cargo.lock through the normal review path. The app retains its development marker and short source hash for build provenance. No workflow calculates a version, creates a tag or edits main.

After merging, select the exact main commit to release and push only its intended version tag. The tag can identify an earlier main commit: the build checks out that snapshot, not today’s main. A push trigger requires the workflow to exist in that snapshot. For older snapshots, dispatch **tagged release** on main with the existing `tag` input instead. The selected source must already contain the required bundle/build hooks and use calendar numbering. Current trusted main signing/metadata tooling is loaded separately under ignored `target/`; it does not alter the tagged source or its provenance.

For this jj-enabled checkout, after fetching merged main, choose its exact commit:

```sh
jj tag set v2026.11.1 -r <full-commit-id>
jj git push --remote origin --tag v2026.11.1
```

These commands create/push only the requested tag. They are examples, not commands run by CI. Confirm the target Cargo version before pushing.

Use an annotated tag when your client supports it. Lightweight tags also resolve to the same exact commit. Do not move/delete a reserved release tag. Failed builds can be retried at the same source; source fixes require a fresh version/tag. The legacy failed `v0.1.1` candidate stays reserved and is outside the calendar release channel.

The full pipeline is serialized and never cancels an active build. Push one release tag at a time and wait for completion: GitHub concurrency can replace an older pending run if several are queued. Rerun a canceled tag explicitly. A complete verified draft resumes at promotion; an incomplete upload can be rebuilt and its unpublished draft replaced. Public releases/assets are never replaced. If Homebrew delivery fails after publication, rerun the same tag: publication is idempotent and tap updates use compare-and-swap with bounded retries. Nothing bumps main afterward.

A release older than the latest calendar version is published without marking it latest; Homebrew also refuses downgrades. The update feed is the signed `appcast.xml` asset at:

`https://github.com/sorahn/jaywalk/releases/latest/download/appcast.xml`

The first public stable release makes that URL available; no separate web server is required. Every feed enclosure refers to its exact tagged ZIP. Preserve feed compatibility when adding platform/OS channels. A tag push made by a person or GitHub App triggers Actions; a tag created using a workflow’s ordinary `GITHUB_TOKEN` does not. If automating tag creation later, use the App or explicitly dispatch the pipeline.

### Homebrew automation identity

Use the dedicated release GitHub App, separate from the app’s hosting-login registration. Install it for only `sorahn/jaywalk` and `sorahn/homebrew-tap`. Store its public ID as `RELEASE_APP_ID` and private PEM as `RELEASE_APP_PRIVATE_KEY` in `release-automation`. Tokens are short lived and narrowed to Contents write. The selector checks tap availability before building; the final job updates only the generated `Casks/jaywalk.rb` with the verified public DMG version/checksum. An owner-edited same-version mismatch is refused.

The tap must be public, initialized on main, and writable by that installation before the first tag. Its initial README, license and credential-free cask audit workflow can exist before the first cask. After publication, install with `brew install --cask sorahn/tap/jaywalk`; use `brew upgrade --cask --greedy jaywalk` to include the auto-updating cask in Homebrew upgrades. Preserve config and credentials on uninstall; the generated cask has no `zap` stanza.

Tag creation remains owner controlled; `v*` tag updates/deletion remain blocked. Remove the release App’s former main-update/tag-creation bypasses: this pipeline needs neither. Main deletion/non-fast-forward protections remain unchanged. Retain only the selected-repository App permissions needed for tap delivery; no automated version commits require an Actions-write permission anymore.

## Native update integration

The small Objective-C bridge uses the pinned Sparkle framework; Rust owns public TOML preferences and opt-in. It does not read OAuth credentials. Automatic checks require both `setup.completed=true` and `updates.automatic_checks=true`. Manual checks require an explicit action. No telemetry/profile or release-note fetches are enabled. Update enclosures come from this repository's GitHub release paths; download-only redirects are limited to GitHub's release asset CDN.

Download-only deliberately avoids Sparkle's combined automatic-download/install setting: that setting can schedule installation on quit. Its raw cache never extracts or executes content; later installation goes through Sparkle verification and currently re-fetches the payload. The consent UI documents this distinction. Automatic installation is enabled only with its own opt-in and restart guard. A live query checks waiting CLI clients and local checkout leases at install/relaunch boundaries. The ordinary guarded restoration path still runs on app quit; interrupted receipts remain recoverable.

The no-window native probe verifies absence of consent/configuration does not instantiate the updater and checks the download URL restrictions. It does **not** verify a signed real update, progress UI, download cache, restart/termination hooks or credentials across a replacement.

## First promotion checklist

- Review and merge the pipeline, then push the intended calendar version tag. Validate a real signed/notarized candidate and inspect the final published artifacts.
- Validate signed app/DMG with `codesign`, `stapler` and `spctl` on a clean Mac.
- Exercise first-run consent (including skip/offline/disable), CLI setup and custom paths, Finder/terminal Git/jj discovery, paths containing spaces/non-ASCII and existing-command conflicts.
- Exercise old-to-new updates under every check/download/install combination, including download-only followed by quit/relaunch, active CLI waiters and local checkout receipts, canceled/interrupted downloads, and rejected/tampered feeds/archives.
- Create the public tap (proposed `sorahn/homebrew-tap`) and use the generated cask; run `brew style`, `brew audit --cask`, install/upgrade/reinstall/uninstall. Confirm app replacement preserves CLI links/config/accounts and that `auto_updates true` coexists with Homebrew's recorded versions.
- Verify the stable GitHub latest-release appcast URL resolves after publication; do not rebuild or rewrite signed payloads.
- Verify automatic GitHub publication and tap delivery end to end. Public hosting OAuth registrations must also be resolved before claiming those connections work for arbitrary users.
