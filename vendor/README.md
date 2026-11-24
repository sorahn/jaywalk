# native component source

`gpui-component` is the published 0.10.0 crate, retained with its Apache-2.0
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
Both natural and sticky headers use the same rendering path. Pinned headers
reserve the natural top separator's height as padding: the comparison bar owns
the viewport separator, while the file header keeps one bottom border. This
avoids doubled separators on the second and subsequent files without changing
pinning/push-off geometry.

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

The 0.10.0 refresh preserves these host extensions on the published crate
(upstream revision `f697c3133d55c64fa8a5b67926147fb3dadd276d`,
`crates/component` in `longbridge/gpui-kit`). GPUI Kit,
Base and Component must move together; the headless test alias must match
Kit's exact `gpui-pre` version (0.3.12). The Base dependency includes the
fractional scroll-delta fix: unclamped input is consumed in full rather than
turning floating-point subtraction residue into a false overscroll.
`tests/trackpad_scroll.rs` guards that behavior through the public wrapper.

`Command::description` adds a non-selectable context slot between search and
results in `src/command/command.rs` and `src/command/state.rs`. It lets the host
show compact status without putting controls above search or inserting fake
results. Host interaction tests verify slot order and keyboard confirmation.

`Command::whole_row_viewport` is an opt-in host extension for floating palettes.
It caps the list height at the last complete measured item within its maximum
(including variable heights/headings), and keeps the existing vertical inset
outside the scrolling viewport so clipped rows cannot hover into that strip.
Continuous scrolling and oversized first rows remain available; default toolkit
lists keep their original behavior. This does not change the spacing tokens.

`CommandItem::key` supplies an optional stable identity. Command selection follows
that identity across label changes and reordered refresh results; unkeyed models
retain their previous index-path behavior. A removed keyed result falls back to
normal selection rather than transferring its identity to an unrelated row.

The local diff patch adds validated `DiffFile::with_alignment` for complete source
texts. Alignment indices must cover both sides exactly once, in source order;
sparse multi-hunk and conflict documents are rejected. This lets hosts suppress
ignored differences while retaining distinct source strings, selections and copy
addresses. Counts, hunk ranges and counterparts are rebuilt from the supplied
alignment. `Diff::display_options` controls word highlights, painted space/tab
markers, row-ending labels and cached whitespace-error spans. Marker painting uses
original-to-display mappings (including expanded tabs) and does not mutate source
or clipboard text. Configurable whitespace-error color is part of `DiffColors`.

`Theme::popup_outline` is an optional runtime color override for floating surfaces.
Popover/menu shadow rings and existing tooltip edges use it without changing
layout metrics; `None` retains the upstream neutral edges. Jaywalk resolves this
from its live accent setting and `ui.accent_floating_outlines`.

`Theme::host_spacing` optionally supplies a logical-pixel unit and edge/interactive
roles. Jaywalk projects its live `[ui.spacing]` settings into this policy.
Command rows/measurement/insets, popovers, menus, tooltips, inputs/search and named
button sizes consume it while preserving toolkit defaults when absent. Explicit
control size/style overrides still win. Spacing changes invalidate measured
command rows independently of typography. The existing spacing-token projection
uses the same unit; application files and history keep full-width square rows.

Shortcut tags retain every keystroke in chord bindings, so picker, menu and
tooltip hints show the complete configured sequence rather than its first key.

NumberInput passes explicit text alignment through to its inner Input while
retaining centered text by default; hosts can explicitly choose another alignment.

Host-spaced buttons use the edge inset horizontally (spacing 2 by default) and
the interactive inset vertically (spacing 1);
`.compact()` uses the interactive inset horizontally (spacing 1) and half that
vertically (spacing 0.5), including text toolbar
buttons. Explicit custom sizes and padding overrides remain authoritative.

Select renders its trigger through the shared Button widget while BaseSelect
retains combobox semantics, focus, keyboard behavior and searchable menus.
Dropdown carets trail the label; padding belongs only to the Button frame. Compact
buttons (including dropdowns) use line height 1.2 and matching minimum heights.

NumberInput supports readonly edits/stepping without disabled-opacity changes,
so short settings writes can preserve the control's appearance and selection.

`gpui-base` is the published 0.10.0 crate with its Apache-2.0 license; registry
metadata and its standalone lockfile are omitted. Upstream provenance is
`longbridge/gpui-kit`, revision `f697c3133d55c64fa8a5b67926147fb3dadd276d`,
`crates/base`. Its only local change makes
the passive plugin-layer host transparent to inspector picking by using default
interactivity before styling it. The component overlay hosts use the same rule.
Their children retain source locations and normal input behavior. This prevents
empty full-window notification/plugin layers from hiding individual controls
from the debug inspector. The host regression requires a control-sized highlight.

The color picker state (`gpui-base/src/color_picker.rs`) adds opt-in
`ColorPickerState::opaque(true)` for RGB-only settings. It normalizes committed,
previewed and slider/hex values to alpha 1 before emission. The component facade
hides the alpha slider in that mode; existing alpha-aware consumers keep their
original defaults. Jaywalk retains each picker beside its hex input and stages
only committed selections through the shared Save/Discard draft.
