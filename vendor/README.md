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

The default diff file header folds from its filename/status/statistics area as
well as its caret. Header prefix/suffix controls and file annotations remain
outside that click target, so actions such as opening a file do not also fold it.
Both natural and sticky headers use the same rendering path.

The diff state exposes a virtualized viewport offset and restoration method for
retained overview views in the host's Back/Forward history.

`Diff::header_context_menu` lets the host add right-click actions to natural and
sticky file headers without replacing their folding controls, metadata, rename
labels or statistics. The hook is isolated in `src/diff/mod.rs`.

`ManagedTooltipExt` is public so host rows can use the existing press-dismissed
tooltip manager instead of leaving a GPUI hover tooltip above an open context menu.
Managed tooltips dismiss on right presses as well as left presses.

Diff text bodies expose Copy/Select all through their existing focused actions,
with Copy disabled for empty selections. Copy snapshots source text at menu-open
because clicking a popup item can clear shared selection before action dispatch.
Inner file-header menus take precedence.

`Diff::colors` accepts a per-view `DiffColors` palette for line marks, row tints,
word highlights and conflict colors. Without it, the widget preserves its
original toolkit-theme defaults. The host resolves adaptive colors centrally and
passes the same palette to individual file diffs and the overview; status
success/danger colors elsewhere in the toolkit are unaffected.

Row rebuilds retain a file-header viewport anchor and its offset instead of
retargeting it to the first source line. This keeps the overview preamble visible
when toggling full-file context. Code-row anchors continue to follow source lines.
Headless host regressions live in `tests/overview_scroll.rs`.

`DiffState::change_navigation_position` exposes the last navigated changed group
and projection count so standard diff footers can report the same position as
Fluid. Row rebuilds clear that selection. Range disclosure buttons expose debug
selectors for simulated-window folding regressions in both standard modes.

The diff body's vertical scrollbar uses its positioned overlay bounds rather
than the list's bounds. Unwrapped lines can widen the list beyond the visible
panel; the scrollbar must stay at the visible panel edge.

`DiffState::restore_viewport_position` maps automatic source-line reading anchors
to existing rows without expanding hidden context. Explicit `scroll_to_line`
continues to reveal its target. The host uses restoration for mode switches,
reflow, and refresh so folded standard diffs stay folded.
