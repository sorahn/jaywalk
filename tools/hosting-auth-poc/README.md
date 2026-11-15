# Hosting authentication probe

A standalone Rust feasibility tool, deliberately outside Jaywalk's build and
release dependency graph. It does not access a local Git/jj repository or use
`gh`, `bb`, or another hosting CLI. Run from the repository root:

```sh
cargo build --locked --manifest-path tools/hosting-auth-poc/Cargo.toml
```

Use `tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc` for the commands
below. `--help` lists them. macOS builds need the full Xcode developer directory
as described in `docs/development.md`. On Linux the probe uses Secret Service
and needs the D-Bus development library at build time (e.g. `dbus-devel` on
Fedora, `libdbus-1-dev` on Debian) and an unlocked desktop Secret Service at
runtime. There is no plaintext or mock-store fallback.

## Browser-assisted GitHub development registration

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-register
```

This opens a temporary loopback setup page. Click **Continue to GitHub**, then
confirm the app name in GitHub. The manifest preselects a private development
app with Metadata and Pull requests read-only and webhooks disabled. GitHub
validates the disabled hook URL too, so it uses the project homepage as an
inactive placeholder; no webhook service or deliveries are enabled. GitHub
returns a temporary registration code to the probe. It exchanges that code
and saves the app ID, slug and client ID in the OS credential store. Generated
private keys, client secrets and webhook secrets are not retained or printed.

The probe then opens the app's GitHub settings. Enable **Device Flow** and save;
GitHub does not document that switch as a manifest parameter. Next:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-install
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-connect
```

Choose one test repository during installation, then complete device sign-in.
`github-connect` uses the saved client ID when its argument is omitted. You can
reopen settings with `github-setup`. `github-forget-registration` removes only
the local registration record; delete an unwanted development app in GitHub
Developer settings separately. Disconnecting the account leaves this
registration record available for reconnection.

Registration uses a random loopback port and a one-hour callback timeout,
independent of the Bitbucket fixed callback port. An existing saved registration
is never silently replaced. This is development bootstrap, not a step ordinary
Jaywalk users will need when connecting to a published Jaywalk GitHub App.

If GitHub creates the app but the local listener has already stopped, keep the
browser's callback tab open. Do not create another app. Within GitHub's manifest
code lifetime, run:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-recover-registration
```

Paste your own browser return URL at the hidden prompt. The recovery command
also accepts that URL on stdin for local automation; never put it in command
arguments, logs or chat. This is explicit developer recovery: the stopped
listener's original state is unavailable, so it does not claim to revalidate
that state. It validates the loopback URL structure, exchanges only with
GitHub's fixed API origin, checks read-only permissions and saves the returned
identifiers. Normal automatic callbacks still require the original state.
GitHub decides whether the one-time code remains valid. Recovery does not
create a second app. If the code has expired, find the existing app in GitHub
Developer settings and use its client ID with `github-connect`.

## Manual GitHub development registration

Create a **GitHub App** in your account's Developer settings:

- Give it a distinct development name and your project's homepage URL.
- Enable **Device Flow**. Keep expiring user access tokens enabled.
- Disable webhooks; no webhook server or private key is needed for this probe.
- Grant repository **Metadata: read-only** and **Pull requests: read-only**.
  Later file/check experiments may need Contents, Checks and Commit statuses
  read-only; do not add those until testing those endpoints.
- Limit installation to your account and selected test repositories. Install
  it on a disposable repository containing a pull request.
- Copy its **client ID**, not the numeric app ID. No client secret is needed.

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-connect CLIENT_ID
```

The tool opens GitHub in the default browser and displays a short device code.
Enter it yourself and confirm the named development app. It requests
`offline_access` to exercise expiring access and refresh tokens. GitHub App
permissions come from registration, not OAuth repository scopes.

## Bitbucket Cloud development registration

Choose the workspace that should own the development client, then open its
workspace settings and select **OAuth clients** (older UI/documentation calls
these OAuth consumers). The current UI uses:

```text
https://bitbucket.org/WORKSPACE/workspace/settings/oauth-clients
```

Create a client named `Jaywalk Dev`. On the Authorization tab enable
**Authorization code** and **Refresh token**; leave **Client credentials** off.
Use this callback/redirect URL:

```text
http://127.0.0.1:43761/callback
```

Grant **Account: read**, **Repositories: read**, and **Pull requests: read**
only. Copy its key/client ID. Keep its secret local:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc bitbucket-connect CLIENT_ID
```

The tool prompts for the secret without echo, opens the browser and listens
only on IPv4 loopback. It validates a fresh random state, rejects duplicate
parameters and consumes the listener after one valid callback. Ctrl+C cancels;
the browser callback expires after one hour.

**This uses your own development consumer's secret. It proves only that
consumer's documented code/refresh protocol. It does not prove that a shared
secret can ship safely in a desktop app.** Actual callback acceptance and any
public-client alternative still need testing against the registered consumer.
There is no PKCE claim for Bitbucket Cloud and no hosted service in this probe.

## Manual verification

Replace PROVIDER with `github` or `bitbucket`. Run these as separate processes:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc status PROVIDER
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc probe-review PROVIDER OWNER REPO PR_NUMBER
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc refresh PROVIDER
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc status PROVIDER
```

Status loads credentials from the OS store and makes an authenticated identity
request. Probe-review reads one PR and verifies that published source and target
commit IDs exist. Bitbucket PR metadata may contain abbreviated hashes; the
probe resolves each through its own source/destination repository's commit API
and validates a full 40-character hash matching the prefix, including when the
source is a fork. It neither prints private PR content nor establishes the
provider's diff/merge-base semantics. Refresh explicitly rotates and stores the
returned pair. The probe suppresses response bodies and never prints tokens. Non-success HTTP
responses retain the status code and whitelisted OAuth error messages only.

Also test browser denial, Ctrl+C, timeout, unavailable/locked credential store,
and revoked authorization (revoke through provider settings, then run status).
Repeat on macOS and Linux with a fresh process and after a desktop session
restart. Account switching is one connection per provider: disconnect first.

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc disconnect PROVIDER
```

Disconnect forgets the local probe entry; **it does not revoke authorization
at the provider**. Revoke in provider settings when finished. Credentials are
stored under service `jaywalk.hosting-auth-poc.v1`, accounts `github` and
`bitbucket`. The Bitbucket entry includes the development consumer secret for
refresh. No credentials are written to repository files, config, arguments or
fixtures. Run on your local desktop, without terminal/session recording of
hidden input; do not paste secrets into chat.

Synthetic credential-store testing needs no registration:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc store write
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc store read
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc store delete
```

The synthetic entry is separate from both provider connections.

## Checks and limits

```sh
cargo fmt --manifest-path tools/hosting-auth-poc/Cargo.toml -- --check
cargo test --locked --manifest-path tools/hosting-auth-poc/Cargo.toml
cargo clippy --locked --manifest-path tools/hosting-auth-poc/Cargo.toml --all-targets -- -D warnings
```

Automated checks use synthetic data and real loopback sockets. They do not
prove live authorization, refresh or provider callback policy. This is a
blocking terminal experiment, not a GPUI implementation. Cancellation is
checked between requests; in-flight HTTPS can take up to the 20-second request
timeout. No automatic retry, pagination, account manager, hosted broker,
provider abstraction or production callback registration is implemented. Browser-assisted
development registration is a separate GitHub App manifest experiment.

Protocol references:

- https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest

- https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app
- https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens
- https://developer.atlassian.com/cloud/bitbucket/rest/intro/
- https://docs.rs/keyring/3.6.3/keyring/
