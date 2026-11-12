# Rust coding standards

These standards prioritize readable, idiomatic Rust and maintainable native
application code.

Apply these standards to new and revised code. Existing prototype code is not
assumed to comply; improve it in coherent changes without mixing an unrelated
repository-wide refactor into a feature.

## Make ownership and intent clear

- Accept `&str`, `&[T]`, and `&Path` when a function only needs a borrowed view.
  Take owned values when ownership is actually transferred. Use `&mut` only
  when the operation mutates the caller's value.
- Solve lifetime problems by deciding who owns the data. Do not reach for
  `clone`, `Box::leak`, `unsafe`, or shared mutable state merely to silence the
  borrow checker. A deliberate owned value can be simpler than extra lifetimes.
- Cloning is a design choice, not a forbidden operation. Distinguish cheap
  `Entity`/`Rc`/`Arc` handle clones from copying source text or diff models.
  Keep repeated deep copies out of scroll and paint paths; measure before
  adding caching or more complicated ownership.
- Use ordinary Rust naming and standard traits. Prefer a clear loop to an
  obscure iterator chain, and a clear iterator to a manual bookkeeping loop.
  Neither style is a goal by itself.

## Put invariants in the model

- Use enums for mutually exclusive states and named structs for meaningful
  groups of values. Avoid combinations of booleans and unrelated `Option`s
  that permit states the application cannot handle.
- Distinguish byte offsets, source lines, wrapped rows, and pixel positions.
  Introduce a small newtype when confusing units is a realistic bug, rather
  than adding wrappers mechanically. Validate external values at boundaries.
- Keep fields private unless callers need them. Expose operations that
  preserve invariants instead of asking every caller to remember the rules.
- Keep generics and abstractions proportional to actual reuse. Prefer the
  standard library and existing GPUI components to a new framework or macro.

## Handle failures where they can be understood

- Return `Result` for failures from files, jj, parsing, and user-controlled
  input. Preserve the underlying error and add useful operation/path/revision
  context. Do not silently turn an error into empty content or a successful
  default with `.ok()` or `unwrap_or_default()`.
- Use typed errors when callers need to distinguish failure kinds. SNAFU is
  a useful reference for error context, not a mandatory dependency. A small
  boundary message is fine when callers only need to display it.
- Reserve panics for broken internal invariants. An `expect` must explain why
  its condition is guaranteed, not just say that an operation failed. Panics
  are reasonable in tests; prototype shortcuts should remain obvious and be
  replaced when that path is hardened.
- Keep diagnostics actionable and separate from the user-facing UI. Do not
  expose private file contents or command output unnecessarily.

## Keep rendering small and predictable

- Keep repository access, diff calculation, geometry, and application state
  in focused modules; UI callbacks should connect these pieces. Extract by
  responsibility, not an arbitrary function-size limit.
- Keep filesystem access, subprocesses, and source parsing out of render and
  paint. Move potentially slow work off the UI thread. Paint from a coherent
  comparison and current-frame layout; avoid per-frame copies of whole files.
- Model scrolling and geometry as testable calculations where practical.
  Document coordinate systems, clamping, and subtle framework contracts.
  Comments should explain a constraint or decision, not narrate the syntax.
- Add a dependency or local dependency patch only for a concrete need. Record
  compatibility constraints and patch provenance, and preserve licenses.

## Verify behavior, not just compilation

- Use focused unit tests for domain logic and integration tests for repository
  boundaries. Regression tests should demonstrate the failure or invariant,
  rather than repeat the implementation. Use synthetic repository fixtures.
- Exercise relevant Unicode, empty-file, final-newline, endpoint, resizing,
  and wrapping cases for diff/geometry changes. Verify interaction and visual
  behavior in the native window; passing unit tests cannot establish UI feel.
- `just check` runs the required background checks with the project build environment.
  Alternatively, run `cargo fmt --check`, `cargo test`, and
  `cargo clippy --all-targets -- -D warnings` for relevant Rust changes. On
  macOS, use the full Xcode developer directory as described in
  [the development guide](docs/development.md).
  Documentation-only changes need a content/diff check, not a full rebuild.
- Prefer fixing a diagnostic. Scope any justified lint allowance narrowly and
  explain it; do not suppress warnings across the crate or enable every
  pedantic lint as a substitute for review.

## Public references

- The Rust Programming Language: ownership,
  borrowing, error handling, testing, and readable Rust.
  https://doc.rust-lang.org/book/
- The book's guidance on recoverable errors and justified panics:
  https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html
- Shepmaster's Stack Overflow explanation of borrowed API inputs:
  https://stackoverflow.com/a/40006220
- SNAFU error context:
  https://docs.rs/snafu/latest/snafu/
- Rust API Guidelines, used selectively for interfaces this app exposes:
  https://rust-lang.github.io/api-guidelines/checklist.html

These are references for reasoning, not a claim that every project needs the
same architecture, dependencies, compatibility policy, or library-level API
surface.

## Settings documentation

When adding or changing a setting, update `docs/settings.md` together with
`config/default.toml` and the typed settings schema/validation. Document its
default, accepted values or range, scope, and live-application behavior. Include
new configurable commands and shortcut/context changes in `docs/keybindings.md`.
Use `ColorSetting` for every configurable color so it accepts both a shared
hex string and light/dark values. Appearance must update live with the theme.
The shipped TOML remains
the canonical source of defaults; optional values and dynamic maps also need
reference entries.

## Versions

`Cargo.toml` is the single source of the release version. Update its version
and Cargo.lock together. Release notes will live in GitHub Releases when
releases begin; a repository changelog is not maintained.

Development builds display the upcoming version with `-dev`, a source revision,
and `-dirty` when applicable. Release-profile builds display the release version.
The CLI and About window use the same compiled identity; the macOS bundle reads
its numeric version from the built executable. Source archives without Git
metadata display unknown provenance rather than inventing a revision.
