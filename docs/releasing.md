# macOS releases

This is the operator guide for the initial Apple Silicon/macOS 15+ release path. Apple signing/notarization and Sparkle credentials are provisioned separately in protected GitHub environments. Public distribution still requires a successful production build and the acceptance checks below. Intel and Linux release channels remain separate work. Personal design/checklists live in the Obsidian packaging plan.

## Credential-free validation

GitHub's standard macOS runners are free for this public repository. Checks use `macos-15` ARM64 with explicit Xcode 26.1.1. Packaging uses `macos-26` ARM64 with explicit Xcode 26.6: the first rehearsal compiled Rust successfully but the older runner/Xcode pairing crashed in Apple's Icon Composer asset runtime. Packaging now runs `scripts/test-macos-icon.sh` before Cargo so an incompatible image fails early. The app deployment target remains macOS 15. Rust is pinned to 1.100.0; Cargo dependencies stay locked. Checks and official release jobs install checksum-pinned jj 0.47.1 into RUNNER_TEMP outside Cargo’s target/cache directory and verify Git/Clang availability before compiling. jj is required for the test suite, but remains optional for plain-Git users and is not bundled into the app. GitHub-hosted images roll forward; record the actual image/Xcode/Rust versions in the run logs and update the explicit Xcode path deliberately if images retire it.

`checks.yml` runs metadata tests, Rust formatting/tests/strict Clippy and updater-enabled compilation on PRs without signing credentials. Each main push keeps its own checks run, so a newer merge does not cancel an earlier merge's validation. Successful main-push checks trigger `automatic-release.yml` on trusted main. The pipeline records a calendar build identity, builds/signs/notarizes the exact checked source, verifies and publishes its GitHub assets/appcast, then updates Homebrew. PR checks, failed/canceled checks and foreign repositories cannot reserve a release.

`macos-release.yml` is the reusable candidate builder. Its manual unsigned rehearsal accepts an optional injected calendar version, produces artifacts with no update feed/key and never publishes or allocates a tag. Official candidates can be called only from main with an allocation, exact source and successful checks run. The signing job revalidates those identities and reuses the completed source checks rather than running them again. No `pull_request_target` or privileged fork-artifact signing is used.

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

Use a fresh output directory for each packaging rehearsal; existing versioned payloads are never overwritten. A rehearsal manifest has `official:false`; cask generation rejects it. `JAYWALK_RELEASE_VERSION=2026.11.1 python3 scripts/release-macos.py check-tag v2026.11.1` validates a tag against the injected app version. Packaging validates the native ARM64 slice, preserves framework symlinks, includes the app/CLI and a DMG Applications link. Final checksums are produced after all signing/notarization/stapling operations. Never label an ad hoc artifact an official signed release.

## Protected production configuration

Restrict `macos-release` and `release-automation` to `main`, without recurring environment-reviewer gates. A merge is the owner’s approval to release once that exact source passes checks. Keep tags, ordinary branches and forks excluded from credentials. The allocator verifies the checks workflow path, event, branch, repository, success state and full source SHA, plus source ancestry on main. The build independently revalidates the immutable allocation before using signing credentials.

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

The automatic release caller explicitly passes the four named secrets into
`macos-release.yml`, which declares them required under `workflow_call.secrets`.
GitHub requires this handoff even for secrets stored only in the called job's
protected environment; declaring `environment: macos-release` alone does not
make them available. They remain environment-scoped. The official job checks
for nonempty secrets and required variables before checkout/toolchain/cache
setup, reporting missing names only. Required declarations catch a missing
handoff, while the runtime check catches missing values. See
https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows .

Enter/export values locally and upload through secure tooling; do not send secrets in chat or commit them. Keep encrypted backups and document rotation. Development builds keep the Apple Development identity; public builds use Developer ID Application. CI uses an ephemeral Keychain and deletes its temporary key files/Keychain on exit. No ACL/trust changes are made to the developer's login Keychain.

Official signing requires an explicit identity and hardened runtime/timestamp. The app, launcher and Sparkle's nested helpers/framework are signed inside-out. Submit/staple the app, package it, sign/submit/staple the DMG, then verify signatures, Gatekeeper and tickets before computing final artifact metadata. The manifest records version, exact source commit, platform/architecture, minimum OS and final sizes/hashes.

`generate_appcast` signs the archive enclosure and feed with the separate EdDSA key. Only the canonical ZIP is fed to it; the DMG is a separate install artifact. Preserve existing published artifacts for rollback. The candidate job creates draft assets and a generated `jaywalk.rb`. The promotion job downloads and independently verifies their checksums, Developer ID certificate, notarization tickets, ZIP/DMG sealed-app identity, executable provenance, update configuration, Sparkle key pair and signed feed/archive, then checks cask style before publication. Fulfill license/source/notice requirements for redistributed dependencies before shipping. The bundle collects resolved dependency metadata and available license/notice files under Resources/licenses, without local source paths; this inventory is an audit aid, not a claim that every dependency's redistribution obligations have been fulfilled.

## Automatic calendar builds

The owner merges a PR. Successful checks for that exact main commit start everything else automatically. No version edit, manual tag, release dispatch, approval gate or version-bump commit is required. The first merge containing this workflow enables the path; branch/PR validation cannot publish.

Release numbers use `year.month.release-number`, without leading zeroes: `2026.11.1`, `2026.11.2`, then `2026.12.1`. The number counts reservations within the month, including failed builds; gaps are expected. CI chooses the current UTC month when first reserving a build. It considers every page of existing tags and never allocates below an already reserved calendar month. Legacy non-calendar tags are left untouched.

Before building, the release App creates an immutable annotated `v<version>` tag at the checked source. Its JSON message records schema, version, full source commit and successful checks run ID. This tag is the durable allocation record: retries find the same source and reuse its version and original checks, even after a month changes. Competing reservations retry only verified ref collisions; permission/network errors stop. Existing tags are never moved. GitHub's tag list provides peeled commit IDs; the recorded annotation and source checks are revalidated before reuse.

Cargo.toml omits package.version and disables crates.io publication. Cargo.lock contains Cargo's default internal `0.0.0`; that value is not the shipped application version. The build script reads `JAYWALK_RELEASE_VERSION` and invalidates cached app identity when it changes. Local builds, including release-profile builds without injection, display `dev` plus source provenance. Their numeric bundle version is `0.0.0`. CI supplies the allocated version to the binaries, bundle, manifest and feed without modifying the source tree. Manifest metadata additionally records the allocation tag and checks run. The source hash remains independently verified.

The complete release cycle uses GitHub's `queue: max`, with cancellation disabled. Up to 100 runs can wait without replacing each other; GitHub cancels additional runs if that queue is exhausted. Processing is FIFO by time entering the queue, so checks finishing out of commit order can affect build numbering. Every successful checked main commit receives its own build; there is no deliberate merge coalescing. Within a cycle, source selection and release automation are pinned independently: all automation checkouts use the same trusted workflow commit, while the bundle builds the recorded source SHA.

Actionlint 1.7.12 predates GitHub's documented `concurrency.queue` field. With that version, suppress only its specific `unexpected key "queue" for "concurrency" section` diagnostic when linting; all other syntax/expression/shell checks remain enabled. Remove that compatibility exception when the tool gains support. GitHub reference: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency .

### Recovery and publication

Rerun a failed automatic release workflow. Alternatively dispatch **automatic release** on main with the successful checks run ID; leave it empty to select successful checks for the current main tip. Recovery of older source requires that source to contain the injected-version build hooks. Both entry points validate the same allocation. The old manual tag-push release trigger is removed so App-created tags cannot recursively start another release.

A complete verified draft resumes at promotion; an incomplete upload can be rebuilt and its unpublished draft replaced. Public releases/assets are never replaced. If Homebrew delivery fails after publication, rerunning the same checked source repairs delivery without publishing another version. Tap updates use compare-and-swap with bounded retries and refuse same-version metadata conflicts. No workflow writes main.

Checks can finish out of commit order. A late build of an ancestor is published as a historical release without marking it latest, even if its reservation number is larger. Updating the default channel requires both a higher version and a descendant source commit. Homebrew delivery follows only the release currently marked latest and also refuses numeric downgrades. A recovered lower-version build stays historical; a subsequent main merge allocates the next version. The update feed is the signed `appcast.xml` asset at:

`https://github.com/sorahn/jaywalk/releases/latest/download/appcast.xml`

The first public stable release makes that URL available; no separate web server is required. Every feed enclosure refers to its exact tagged ZIP. Preserve feed compatibility when adding platform/OS channels.

### Homebrew automation identity

Use the dedicated release GitHub App, separate from the app’s hosting-login registration. It is installed for exactly `sorahn/jaywalk` and `sorahn/homebrew-tap`. Store its public ID as `RELEASE_APP_ID` and private PEM as `RELEASE_APP_PRIVATE_KEY` in `release-automation`. Allocation and completion tokens request Contents write and Actions read so both can revalidate the recorded checks. Publication uses the workflow token with Contents write and Actions read. The selector checks tap availability before reserving a version. The final job updates only the generated `Casks/jaywalk.rb` with the verified public DMG version/checksum. An owner-edited same-version mismatch is refused.

The public tap is initialized on main with README, license and a credential-free cask audit workflow. The first verified public release adds its cask. Install with `brew install --cask sorahn/tap/jaywalk`; use `brew upgrade --cask --greedy jaywalk` to include the auto-updating cask in Homebrew upgrades. Preserve config and credentials on uninstall; the generated cask has no `zap` stanza.

Allow the release App to bypass only the `v*` tag-creation restriction. Keep version-tag update/deletion blocked with no App bypass. Main updates remain owner controlled; the App needs no main-update bypass. Main deletion/non-fast-forward protections remain unchanged. App registration permissions may exceed a job's requested token permissions; the workflow narrows its tokens explicitly. Tag creation does not trigger this automatic workflow, and ordinary `GITHUB_TOKEN` publication does not create a recursive release event chain.

## Native update integration

The small Objective-C bridge uses the pinned Sparkle framework; Rust owns public TOML preferences and opt-in. It does not read OAuth credentials. Automatic checks require both `setup.completed=true` and `updates.automatic_checks=true`. Manual checks require an explicit action. No telemetry/profile or release-note fetches are enabled. Update enclosures come from this repository's GitHub release paths; download-only redirects are limited to GitHub's release asset CDN.

Download-only deliberately avoids Sparkle's combined automatic-download/install setting: that setting can schedule installation on quit. Its raw cache never extracts or executes content; later installation goes through Sparkle verification and currently re-fetches the payload. The consent UI documents this distinction. Automatic installation is enabled only with its own opt-in and restart guard. A live query checks waiting CLI clients and local checkout leases at install/relaunch boundaries. The ordinary guarded restoration path still runs on app quit; interrupted receipts remain recoverable.

The no-window native probe verifies absence of consent/configuration does not instantiate the updater and checks the download URL restrictions. It does **not** verify a signed real update, progress UI, download cache, restart/termination hooks or credentials across a replacement.

## First promotion checklist

- Review and merge the pipeline; successful checks should automatically allocate, build and publish its first calendar release. Validate the real signed/notarized candidate and inspect its outputs.
- Validate signed app/DMG with `codesign`, `stapler` and `spctl` on a clean Mac.
- Exercise first-run consent (including skip/offline/disable), CLI setup and custom paths, Finder/terminal Git/jj discovery, paths containing spaces/non-ASCII and existing-command conflicts.
- Exercise old-to-new updates under every check/download/install combination, including download-only followed by quit/relaunch, active CLI waiters and local checkout receipts, canceled/interrupted downloads, and rejected/tampered feeds/archives.
- Create the public tap (proposed `sorahn/homebrew-tap`) and use the generated cask; run `brew style`, `brew audit --cask`, install/upgrade/reinstall/uninstall. Confirm app replacement preserves CLI links/config/accounts and that `auto_updates true` coexists with Homebrew's recorded versions.
- Verify the stable GitHub latest-release appcast URL resolves after publication; do not rebuild or rewrite signed payloads.
- Verify automatic GitHub publication and tap delivery end to end. Public hosting OAuth registrations must also be resolved before claiming those connections work for arbitrary users.
