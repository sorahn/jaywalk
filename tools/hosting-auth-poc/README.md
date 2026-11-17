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
and saves the app ID, slug, client ID and browser client secret in the OS
credential store. Generated private keys and webhook secrets are not retained;
no secrets are printed. The manifest preconfigures the browser authorization
callback separately from the temporary app-registration callback.

Browser sign-in is the default. No Device Flow checkbox is required. Next:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-install
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-connect
```

Choose one test repository during installation, then authorize in the browser.
`github-connect` uses the saved development client credentials when its client
ID argument is omitted. You can reopen publisher settings with `github-setup`. `github-forget-registration` removes only
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
Developer settings and use its client ID with `github-connect` and enter its client secret at the hidden
prompt. The app's Redirect URI must include `http://127.0.0.1:43762/callback`.

## Manual GitHub development registration

Create a **GitHub App** in your account's Developer settings:

- Give it a distinct development name and your project's homepage URL.
- Add `http://127.0.0.1:43762/callback` as a **Redirect URI** under
  **General → Identifying and authorizing users**, and save. GitHub's API docs
  call this a Callback URL. Client-secret controls are under **Credentials**.
  Keep expiring user access tokens enabled. Device Flow is optional.
- Disable webhooks; no webhook server or private key is needed for this probe.
- Grant repository **Metadata: read-only** and **Pull requests: read-only**.
  Later file/check experiments may need Contents, Checks and Commit statuses
  read-only; do not add those until testing those endpoints.
- Limit installation to your account and selected test repositories. Install
  it on a disposable repository containing a pull request.
- Copy its **client ID**, not the numeric app ID. Enter its client secret only
  at the probe's hidden local prompt. It is stored in the OS credential store
  after successful browser authorization, never in config, arguments or logs.

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc github-connect CLIENT_ID
```

The tool opens GitHub in the default browser and displays a short device code.
Enter it yourself and confirm the named development app. It requests
`offline_access` to exercise expiring access and refresh tokens. GitHub App
permissions come from registration, not OAuth repository scopes.

## Browser and device sign-in experiments

`github-connect` opens GitHub directly using authorization code + PKCE (S256),
fresh state and a loopback-only callback. There is no device code to copy.
The callback listener, timeout, cancellation and duplicate/replay checks are
shared with the existing protocol experiment. GitHub App permissions remain
read-only; `offline_access` requests rotating credentials without broadening
repository permissions.

The previous device path is available with `github-connect --flow device`.
Only that path requires Device Flow enabled in publisher settings and can
refresh without a client secret. Browser authorization and refresh use the
developer registration's client secret, as GitHub documents. This tests the
developer's own credentials; it does not decide how a public Jaywalk build
would provision its app credentials.

For a registration created by the earlier device-only probe, run `github-setup`,
add the callback above and save, then generate a client secret if necessary.
Enter it in the local hidden prompt. There is no need to delete or register the
app again, change permissions, or turn Device Flow off. This is a one-time
publisher setup, not ordinary user onboarding.

Use `github-connect --reauthorize` to test browser sign-in while retaining an
existing connection until the replacement passes authorization and identity
validation. A cancelled or rejected attempt leaves the prior saved session
alone. `refresh github` uses the authentication method recorded in that
session, so existing device sessions remain secret-free.

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
only. This is one-time developer registration, separate from ordinary sign-in.
Copy its key/client ID and enter the secret only at the hidden local prompt:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc bitbucket-setup CLIENT_ID
```

If this probe already has a Bitbucket session, recover its client credentials
without copying or entering the secret again:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc bitbucket-setup --from-session
```

Setup saves only client credentials in the separate OS-store account
`bitbucket-development-registration`. It does not copy account tokens.
Normal sign-in then uses that registration without prompting for a secret:

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc bitbucket-connect
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc bitbucket-workspaces
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc bitbucket-select-workspace WORKSPACE
```

The browser callback listens only on IPv4 loopback, validates fresh random
state, rejects duplicate parameters and consumes the listener after one valid
callback. Ctrl+C cancels; the browser callback expires after one hour.
Use `bitbucket-connect --reauthorize` to retain an existing session until new
browser authorization and identity verification succeed. Replacement clears
workspace selection because the account may have changed. Cancellation or
failure leaves the previously saved session untouched.

Workspace discovery uses the current account's paginated `/2.0/user/workspaces`
results, not the workspace owning the OAuth client. Select any returned slug;
the selection is stored with that account session and survives token refresh.
Selection is a browsing preference, not an additional permission grant or an
API-access restriction. This standalone probe has command-line selection;
there is no native Accounts picker yet.

`disconnect bitbucket` removes the local account session and selection but
retains developer setup. `bitbucket-forget-registration` removes that setup
locally, without deleting the online client or the existing session. For a fresh
consent test, revoke only the user's authorization under Personal Bitbucket
settings > App authorizations, then reconnect. Keep the workspace OAuth client.

This uses the developer client's secret for code exchange and refresh. It does
not decide how app credentials will be provisioned in a public desktop build.
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
restart. Account switching is one connection per provider; use `--reauthorize` to replace
a session only after successful browser authorization, or disconnect first.

```sh
tools/hosting-auth-poc/target/debug/jaywalk-hosting-auth-poc disconnect PROVIDER
```

Disconnect forgets the local probe entry; **it does not revoke authorization
at the provider**. Revoke in provider settings when finished. Credentials are
stored under service `jaywalk.hosting-auth-poc.v1`, accounts `github` and
`bitbucket`. Browser-authorized GitHub sessions and the Bitbucket entry include their
development client secret for refresh. The development registration entry can
also retain GitHub's client secret; Bitbucket developer setup has its own
`bitbucket-development-registration` entry; it is never passed through chat or logs. No credentials are written to repository files, config, arguments or
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


## Live GitHub browser result

Verified on macOS on 2026-11-17 in both an existing-registration test and a
fresh-registration test:

- For the existing development app, the publisher added its Redirect URI and
  entered a newly generated client secret locally. Browser authorization,
  fresh-process recovery, PR metadata access and refresh succeeded.
- The publisher then deleted that development app remotely. Only the probe's
  local GitHub session and development registration were cleared for the clean
  test; the Bitbucket connection was retained.
- A fresh manifest-created app preconfigured the authorization redirect and
  returned client credentials directly to the local probe. The secret was saved
  in the OS store, with no copying or developer-setting edits.
- The user installed the new app and completed browser authorization without
  configuring Device Flow or entering a device code or client secret.
- Fresh-process status, PR metadata/commit-identity checks, token-pair rotation
  and another fresh-process status all succeeded for this new app.

App registration is a publisher setup step. Ordinary users authorize/install
an already configured app. The terminal probe does not claim a native GPUI
Connect UI or establish public-build credential provisioning. Its developer
client-secret test is not secret-free browser OAuth.
