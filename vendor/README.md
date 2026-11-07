# native component source

`gpui-component` is the published 0.8.0 crate, retained with its Apache-2.0
license. Cargo registry metadata and the upstream lockfile are omitted.

The viewer needs fixed split columns with coordinated horizontal scrolling;
the upstream widget scrolls the entire split canvas. Local changes are limited
to that behavior and a public scroll entry point. Keep them isolated in
`src/diff/mod.rs` and `src/diff/state.rs` when refreshing the dependency.
