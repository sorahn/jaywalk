# native component source

`gpui-component` is the published 0.8.0 crate, retained with its Apache-2.0
license. Cargo registry metadata and the upstream lockfile are omitted.

The viewer needs fixed split columns with coordinated horizontal scrolling;
the upstream widget scrolls the entire split canvas. Local changes are limited
to that behavior, a public scroll entry point, independent wrapping for the
split panes, honoring the monospace font's pixel size independently of UI
font scaling, per-view code fonts and OpenType features, configurable tab-stop
expansion that retains source/copy mappings, and a source-line viewport anchor accessor so the host can keep
reading position across view switches and refreshes. Keep them isolated in
`src/diff/mod.rs`, `src/diff/state.rs`, and `src/diff/document.rs` when refreshing the dependency.
