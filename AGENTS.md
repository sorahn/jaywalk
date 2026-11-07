# Jaywalk

This is a personal native Rust/GPUI application.

## Rust coding standards

Read and follow `CONTRIBUTING.md` before implementing or reviewing Rust code.
Apply the standards to the scope being changed; keep unrelated prototype
cleanup in separate, reviewable changes.

## Development history

The user opted `viewer-prototype` into the mutable jj workflow. Use jj for
history changes until the initial PR opens. Keep `main` as the initial Rust
baseline and keep the prototype bookmark at the series tip. Amend the logical
change that owns a revision and replay descendants. Keep descriptions short,
lowercase, and without prefixes. Opening the initial PR ends mutable history;
append review fixes normally afterward. Do not push or open a PR without
authorization.

Record the turn-start operation and full feature-tip commit ID before edits;
record the end tip and offer a reproducible net-code diff at handoff. Preserve
these points through jj operation history. Agent-turn browsing is outside the
viewer product scope.

## Product and fixtures

Use GPUI Kit's existing text and panel components. Do not build a new UI
framework. The app is native, not a browser, Electron, or Tauri application.

External checkouts may be used only as read-only local test data. Do not
snapshot or change their working copies or history. Use operation-pinned jj
queries and synthetic committed test fixtures. Never copy private source or
project-specific details into this repository or its documentation.

Personal plans and research belong in Homelab/Bases/plans and
Homelab/Bases/notes in the Obsidian vault, not this repository.

Run cargo fmt, cargo test, and cargo clippy for relevant Rust changes. Check
selection/copy and scrolling in the actual native window before claiming UI
behavior is verified.
