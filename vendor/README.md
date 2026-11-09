# native component source

`gpui-component` is the published 0.8.0 crate, retained with its Apache-2.0
license. Cargo registry metadata and the upstream lockfile are omitted.

The viewer needs fixed split columns with coordinated horizontal scrolling;
the upstream widget scrolls the entire split canvas. Local changes are limited
to that behavior, a public scroll entry point, independent wrapping for the
split panes, honoring the monospace font's pixel size independently of UI
font scaling, per-view code fonts and OpenType features, configurable tab-stop
expansion that retains source/copy mappings, and source-line viewport and horizontal-offset accessors so the host can keep
reading position across view switches and refreshes. Keep them isolated in
`src/diff/mod.rs`, `src/diff/state.rs`, and `src/diff/document.rs` when refreshing the dependency.

The input highlighter adapter also exposes `PreparedInputHighlighter` in
`src/highlighter/input_adapter.rs` and `src/highlighter/mod.rs`. It lets the host
parse immutable comparison text on a background thread and install the complete
parse before displaying it, avoiding a plain-text frame and delayed re-highlighting.

The diff widget also supports opt-in sticky file headers (`Diff::sticky_headers`)
and exposes `DiffState::viewport_file` for host reading indicators. Sticky header
placement uses current-frame list geometry in `src/diff/sticky.rs`, reusing the
normal interactive header without duplicating file annotations. The next file's
header pushes it away; ordinary single-file views keep this feature disabled.

The input facade scopes code bodies as `Editor` or `ReadonlyEditor` in
`src/input/input.rs`, leaving the editable search panel outside that scope.
This lets host review shortcuts override source navigation without intercepting
normal text movement in search inputs.

The diff widget's `render_preamble` slot lets overview metadata scroll with the
first file. The first file header's bounds are recorded during list prepaint and
consumed by the sticky layer in the same frame. `DiffState::preamble_scrolled`
lets the host fold the subject into its existing comparison bar; the host
prepaints that bar after the diff to avoid a deferred caption update. Empty
comparisons keep their standalone host header.

`DiffState::remeasure_preamble` invalidates the first list item's measured height
when host-provided message disclosures change, retaining the list's scroll anchor.
