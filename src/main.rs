extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::assets::IconName;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui::component::input::{Editor, EditorState};
use gpui::component::resizable::{h_resizable, resizable_panel};
use gpui::component::{Disableable, Selectable, StyledExt, Theme, ThemeMode};
use gpui::*;
use jaywalk::diff_model::FileDiff;
use jaywalk::fluid::FluidScroll;
use jaywalk::repository::Comparison;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Parser)]
#[command(about = "Jaywalk: a native Jujutsu diff viewer")]
struct Arguments {
    #[arg(default_value = ".")]
    directory: PathBuf,
    #[arg(long)]
    base: Option<String>,
    #[arg(long, default_value = "@")]
    tip: String,
    #[arg(long)]
    inspect: bool,
    #[arg(long)]
    file: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ViewMode {
    Inline,
    SideBySide,
    #[default]
    Fluid,
}

impl ViewMode {
    fn standard(self) -> DiffMode {
        match self {
            Self::Inline => DiffMode::Unified,
            Self::SideBySide | Self::Fluid => DiffMode::Split,
        }
    }
}

fn standard_patch(before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(before.lines().count().max(after.lines().count()))
        .header("a/source", "b/source")
        .to_string()
}

type CodeComparison = (Entity<EditorState>, Entity<EditorState>, FileDiff);

struct Viewer {
    arguments: Arguments,
    comparison: Result<Comparison, String>,
    selected: Option<usize>,
    active_change: Option<usize>,
    navigation_target: Option<usize>,
    code: Option<CodeComparison>,
    view_mode: ViewMode,
    standard_diff: Option<Entity<DiffState>>,
    standard_error: Option<String>,
    file_error: Option<String>,
    fluid: FluidScroll,
    editor_observers: Vec<Subscription>,
}

impl Viewer {
    fn new(arguments: Arguments, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let comparison = Comparison::load(
            &arguments.directory,
            arguments.base.as_deref(),
            &arguments.tip,
        );
        let mut viewer = Self {
            arguments,
            comparison,
            selected: None,
            active_change: None,
            navigation_target: None,
            code: None,
            view_mode: ViewMode::default(),
            standard_diff: None,
            standard_error: None,
            file_error: None,
            fluid: FluidScroll::default(),
            editor_observers: Vec::new(),
        };
        let selected_file = viewer.arguments.file.as_ref().and_then(|path| {
            viewer
                .comparison
                .as_ref()
                .ok()?
                .files
                .iter()
                .position(|file| &file.after_path == path)
        });
        if let Some(index) = selected_file {
            viewer.select_file(index, window, cx);
        }
        viewer
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(index);
        self.active_change = None;
        self.navigation_target = None;
        self.fluid = FluidScroll::default();
        self.editor_observers.clear();
        self.code = None;
        self.file_error = None;
        self.standard_diff = None;
        self.standard_error = None;
        if let Ok(comparison) = &self.comparison {
            let file = &comparison.files[index];
            match comparison.contents(file) {
                Ok((before, after)) => {
                    let model = FileDiff::new(&before, &after);
                    let extension = std::path::Path::new(&file.after_path)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("");
                    let language = match extension {
                        "ts" => "typescript",
                        "tsx" => "tsx",
                        "js" | "jsx" => "javascript",
                        _ => "plain",
                    };
                    match DiffFile::parse(&standard_patch(&before, &after)) {
                        Ok(files) if !files.is_empty() => {
                            let mode = self.view_mode.standard();
                            let files = files.into_iter().map(|file| file.with_language(language));
                            self.standard_diff = Some(cx.new(|cx| {
                                DiffState::new(files, cx)
                                    .with_context_lines(None)
                                    .with_mode(mode)
                            }));
                        }
                        Ok(_) => {}
                        Err(error) => self.standard_error = Some(error.to_string()),
                    }
                    let make_editor =
                        |text: String, window: &mut Window, cx: &mut Context<Self>| {
                            cx.new(|cx| {
                                EditorState::new(window, cx)
                                    .language(language)
                                    .soft_wrap(false)
                                    .folding(false)
                                    .default_value(text)
                            })
                        };
                    let left = make_editor(before, window, cx);
                    let right = make_editor(after, window, cx);
                    self.editor_observers = vec![
                        cx.observe(&left, |_, _, cx| cx.notify()),
                        cx.observe(&right, |_, _, cx| cx.notify()),
                    ];
                    self.code = Some((left, right, model));
                }
                Err(error) => self.file_error = Some(error),
            }
        }
        cx.notify();
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected_path = self
            .selected
            .and_then(|index| self.comparison.as_ref().ok()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.comparison = Comparison::load(
            &self.arguments.directory,
            self.arguments.base.as_deref(),
            &self.arguments.tip,
        );
        self.selected = selected_path.and_then(|path| {
            self.comparison
                .as_ref()
                .ok()?
                .files
                .iter()
                .position(|file| file.after_path == path)
        });
        self.code = None;
        if let Some(index) = self.selected {
            self.select_file(index, window, cx);
        }
        cx.notify();
    }

    fn scroll_comparison(&mut self, delta: f32, cx: &mut Context<Self>) {
        if let Some((before, after, model)) = &self.code {
            let line_height = f32::from(before.read(cx).line_height().unwrap_or(px(20.)));
            let viewport = f32::from(before.read(cx).input_bounds().size.height);
            let (before_y, after_y) = self.fluid.advance(delta, model, line_height, viewport);
            before.update(cx, |editor, cx| {
                let offset = editor.scroll_offset();
                editor.set_scroll_offset(point(offset.x, px(-before_y)), cx);
            });
            after.update(cx, |editor, cx| {
                let offset = editor.scroll_offset();
                editor.set_scroll_offset(point(offset.x, px(-after_y)), cx);
            });
        }
        cx.notify();
    }

    fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        if let Some(state) = &self.standard_diff {
            state.update(cx, |state, cx| state.set_mode(mode.standard(), cx));
        }
        cx.notify();
    }

    fn render_view_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut group = div()
            .h_flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded_md()
            .bg(rgb(0x292929));
        for (id, mode, icon, label) in [
            (
                "view-inline",
                ViewMode::Inline,
                IconName::List,
                "Inline diff",
            ),
            (
                "view-side-by-side",
                ViewMode::SideBySide,
                IconName::Columns2,
                "Side-by-side diff",
            ),
            (
                "view-fluid",
                ViewMode::Fluid,
                IconName::GitCompareArrows,
                "Fluid diff",
            ),
        ] {
            group = group.child(
                Button::new(id)
                    .ghost()
                    .compact()
                    .icon(icon)
                    .tooltip(label)
                    .selected(self.view_mode == mode)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(mode, cx))),
            );
        }
        group
    }

    fn navigate_change(&mut self, direction: i32, cx: &mut Context<Self>) {
        if self.view_mode != ViewMode::Fluid {
            if let Some(state) = &self.standard_diff {
                state.update(cx, |state, cx| {
                    if direction < 0 {
                        state.previous_change(cx);
                    } else {
                        state.next_change(cx);
                    }
                });
            }
            return;
        }
        let Some((before, _, model)) = &self.code else {
            return;
        };
        let count = model.blocks.iter().filter(|block| block.changed).count();
        if count == 0 {
            return;
        }
        let target = (self.active_change.unwrap_or(0) as i32 + direction).clamp(0, count as i32 - 1)
            as usize;
        let line_height = f32::from(before.read(cx).line_height().unwrap_or(px(20.)));
        let viewport = f32::from(before.read(cx).input_bounds().size.height);
        let mut position = 0.;
        let mut change = 0;
        for block in &model.blocks {
            let height = block.before.len().max(block.after.len()) as f32 * line_height;
            if block.changed {
                if change == target {
                    position += height / 2.;
                    break;
                }
                change += 1;
            }
            position += height;
        }
        let delta = position - viewport / 2. - self.fluid.position;
        self.navigation_target = Some(target);
        self.active_change = Some(target);
        self.scroll_comparison(delta, cx);
    }

    fn render_fluid_gutter(
        &self,
        gutter_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (before, after, model) = self.code.as_ref().unwrap();
        let before = before.clone();
        let after = after.clone();
        let model = model.clone();
        let viewer = cx.entity().downgrade();
        let navigation_target = self.navigation_target;
        canvas(
            |_, _, _| (),
            move |overlay_bounds, _, window, cx| {
                let Some(bounds) = gutter_bounds.get() else {
                    return;
                };
                let before_state = before.read(cx);
                let after_state = after.read(cx);
                let before_bounds = before_state.input_bounds();
                let after_bounds = after_state.input_bounds();
                let line_height = before_state.line_height().unwrap_or(px(20.));
                let before_origin = before_state.text_bounds().unwrap_or(before_bounds).origin.y;
                let after_origin = after_state.text_bounds().unwrap_or(after_bounds).origin.y;
                // text_bounds already includes the editor's resolved scroll.
                // This canvas paints after both editors so both geometries are
                // from this frame, including deferred/clamped scroll updates.
                let clip = Bounds::from_corners(
                    point(bounds.left(), before_bounds.top().max(after_bounds.top())),
                    point(
                        bounds.right(),
                        before_bounds.bottom().min(after_bounds.bottom()),
                    ),
                );
                let changes: Vec<_> = model.blocks.iter().filter(|block| block.changed).collect();
                let focus = clip.center().y;
                let distance = |origin: Pixels, lines: &std::ops::Range<usize>| {
                    let top = origin + line_height * lines.start as f32;
                    let bottom = origin + line_height * lines.end as f32;
                    (focus - focus.clamp(top, bottom)).abs()
                };
                let active = changes
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let score = |block: &&jaywalk::diff_model::Correspondence| {
                            distance(before_origin, &block.before)
                                + distance(after_origin, &block.after)
                        };
                        score(a)
                            .partial_cmp(&score(b))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(index, _)| index);
                let active = navigation_target.or(active);
                let text_start = |state: &EditorState, offsets: &[usize]| {
                    let row = state.visible_row_range()?.start;
                    let offset = *offsets.get(row)?;
                    Some(state.range_to_bounds(&(offset..offset))?.left() - state.scroll_offset().x)
                };
                let before_text = text_start(before_state, &model.before_offsets);
                let after_text = text_start(after_state, &model.after_offsets);
                // Full-row tint shares the connector geometry, including the
                // empty space after short lines and the line-number gutter.
                for (viewport, origin, before_side, text_start) in [
                    (
                        Bounds::from_corners(
                            point(overlay_bounds.left(), before_bounds.top()),
                            point(bounds.left(), before_bounds.bottom()),
                        ),
                        before_origin,
                        true,
                        before_text,
                    ),
                    (
                        Bounds::from_corners(
                            point(bounds.right(), after_bounds.top()),
                            point(overlay_bounds.right(), after_bounds.bottom()),
                        ),
                        after_origin,
                        false,
                        after_text,
                    ),
                ] {
                    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
                        for (index, block) in changes.iter().enumerate() {
                            let lines = if before_side {
                                &block.before
                            } else {
                                &block.after
                            };
                            let selected = active == Some(index);
                            let rgb = if block.before.is_empty() {
                                0x4ec96e00
                            } else if block.after.is_empty() {
                                0xf8514900
                            } else {
                                0x3794ff00
                            };
                            if lines.is_empty() {
                                let y = origin + line_height * lines.start as f32;
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(viewport.left(), y - px(0.5)),
                                        size(
                                            viewport.size.width,
                                            px(if selected { 2. } else { 1. }),
                                        ),
                                    ),
                                    rgba(rgb | if selected { 0xc0 } else { 0x70 }),
                                ));
                                continue;
                            }
                            let color = rgba(rgb | if selected { 0x30 } else { 0x20 });
                            let top =
                                (origin + line_height * lines.start as f32).max(viewport.top());
                            let bottom =
                                (origin + line_height * lines.end as f32).min(viewport.bottom());
                            if bottom > top {
                                window.paint_quad(fill(
                                    Bounds::from_corners(
                                        point(viewport.left(), top),
                                        point(viewport.right(), bottom),
                                    ),
                                    color,
                                ));
                                if selected && let Some(text_start) = text_start {
                                    window.paint_quad(fill(
                                        Bounds::from_corners(
                                            point(viewport.left(), top),
                                            point(
                                                text_start.clamp(viewport.left(), viewport.right()),
                                                bottom,
                                            ),
                                        ),
                                        rgba(rgb | 0x20),
                                    ));
                                }
                            }
                        }
                    });
                }
                window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                    for (index, block) in changes.iter().enumerate() {
                        let selected = active == Some(index);
                        let rgb = if block.before.is_empty() {
                            0x4ec96e00
                        } else if block.after.is_empty() {
                            0xf8514900
                        } else {
                            0x3794ff00
                        };
                        let color = rgba(rgb | if selected { 0xb0 } else { 0x70 });
                        let before_top = before_origin + line_height * block.before.start as f32;
                        let before_bottom = before_origin + line_height * block.before.end as f32;
                        let after_top = after_origin + line_height * block.after.start as f32;
                        let after_bottom = after_origin + line_height * block.after.end as f32;
                        if before_bottom.max(after_bottom) < clip.top()
                            || before_top.min(after_top) > clip.bottom()
                        {
                            continue;
                        }

                        // Most changes stay midpoint-to-midpoint. Only a block
                        // taller than the viewport needs its attachment to slide
                        // into view. The clamp is continuous during scrolling.
                        let attachment = |top: Pixels, bottom: Pixels| {
                            let middle = (top + bottom) / 2.;
                            if bottom - top <= clip.size.height {
                                middle
                            } else {
                                let margin = clip.size.height / 4.;
                                middle.clamp(clip.top() + margin, clip.bottom() - margin)
                            }
                        };
                        let before_y = attachment(before_top, before_bottom);
                        let after_y = attachment(after_top, after_bottom);
                        let left = bounds.left() + px(1.);
                        let right = bounds.right() - px(1.);
                        let mut path = PathBuilder::stroke(px(if selected { 1.5 } else { 1. }));
                        path.move_to(point(left, before_top));
                        path.line_to(point(left, before_bottom));
                        path.move_to(point(right, after_top));
                        path.line_to(point(right, after_bottom));
                        path.move_to(point(left, before_y));
                        path.cubic_bezier_to(
                            point(right, after_y),
                            point(bounds.center().x, before_y),
                            point(bounds.center().x, after_y),
                        );
                        if let Ok(path) = path.build() {
                            window.paint_path(path, color);
                        }
                    }
                });
                let _ = viewer.update(cx, |viewer, cx| {
                    if viewer.active_change != active {
                        viewer.active_change = active;
                        cx.notify();
                    }
                });
                let viewer = viewer.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture
                        || !(before_bounds.contains(&event.position)
                            || after_bounds.contains(&event.position))
                    {
                        return;
                    }
                    let delta = event.delta.pixel_delta(line_height);
                    if delta.x.abs() > delta.y.abs() {
                        return;
                    }
                    let _ = viewer.update(cx, |viewer, cx| {
                        viewer.navigation_target = None;
                        viewer.scroll_comparison(-f32::from(delta.y), cx)
                    });
                    cx.stop_propagation();
                });
            },
        )
        .size_full()
        .into_any_element()
    }

    fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .v_flex()
            .size_full()
            .bg(rgb(0x253448))
            .child(div().p_3().text_sm().child("CHANGED FILES / debug blue"));

        match &self.comparison {
            Err(error) => panel = panel.child(div().p_3().child(error.clone())),
            Ok(comparison) => {
                let mut list = div()
                    .id("changed-files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll();
                for (index, file) in comparison.files.iter().enumerate() {
                    let background = if self.selected == Some(index) {
                        0x345577
                    } else {
                        0x253448
                    };
                    let marker = match file.status.as_str() {
                        "added" => "A",
                        "removed" => "D",
                        "renamed" => "R",
                        "copied" => "C",
                        _ => "M",
                    };
                    let label = format!("{marker}  {}", file.after_path);
                    list = list.child(
                        div()
                            .id(("file", index))
                            .px_3()
                            .py_2()
                            .bg(rgb(background))
                            .text_sm()
                            .cursor_pointer()
                            .child(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_file(index, window, cx)
                            })),
                    );
                }
                panel = panel.child(list);
            }
        }
        panel.into_any_element()
    }

    fn render_comparison(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = &self.file_error {
            return div().p_6().child(error.clone()).into_any_element();
        }
        if let Some((left, right, _)) = &self.code {
            if self.view_mode != ViewMode::Fluid {
                let heading = self.comparison.as_ref().unwrap().files[self.selected.unwrap()]
                    .after_path
                    .clone();
                let body = if let Some(error) = &self.standard_error {
                    div().p_6().child(error.clone()).into_any_element()
                } else if let Some(state) = &self.standard_diff {
                    Diff::new(state)
                        .header_visible(false)
                        .soft_wrap(true)
                        .size_full()
                        .into_any_element()
                } else {
                    Editor::new(left)
                        .readonly(true)
                        .bordered(false)
                        .h_full()
                        .into_any_element()
                };
                return div()
                    .v_flex()
                    .size_full()
                    .child(div().px_3().py_2().bg(rgb(0x302b3d)).child(heading))
                    .child(div().flex_1().min_h_0().child(body))
                    .into_any_element();
            }
            let gutter_bounds = Rc::new(Cell::new(None));
            let measured_gutter = gutter_bounds.clone();
            return div()
                .v_flex()
                .size_full()
                .child(
                    div().px_3().py_2().bg(rgb(0x302b3d)).child(
                        self.comparison.as_ref().unwrap().files[self.selected.unwrap()]
                            .after_path
                            .clone(),
                    ),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(
                            h_resizable("code-panes")
                                .child(
                                    resizable_panel().child(
                                        div()
                                            .v_flex()
                                            .size_full()
                                            .child(div().px_3().py_1().child("BEFORE"))
                                            .child(
                                                Editor::new(left)
                                                    .readonly(true)
                                                    .bordered(false)
                                                    .h_full(),
                                            ),
                                    ),
                                )
                                .child(
                                    resizable_panel()
                                        .size(px(48.))
                                        .size_range(px(48.)..px(49.))
                                        .fixed()
                                        .child(
                                            canvas(
                                                move |bounds, _, _| {
                                                    measured_gutter.set(Some(bounds))
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .size_full(),
                                        ),
                                )
                                .child(
                                    resizable_panel().child(
                                        div()
                                            .v_flex()
                                            .size_full()
                                            .child(div().px_3().py_1().child("AFTER"))
                                            .child(
                                                Editor::new(right)
                                                    .readonly(true)
                                                    .bordered(false)
                                                    .h_full(),
                                            ),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .child(self.render_fluid_gutter(gutter_bounds, cx)),
                        ),
                )
                .into_any_element();
        }
        match &self.comparison {
            Err(error) => div().p_6().child(error.clone()).into_any_element(),
            Ok(comparison) => {
                let heading = self
                    .selected
                    .map(|index| comparison.files[index].after_path.clone())
                    .unwrap_or_else(|| "Changeset overview".into());
                div().v_flex().size_full().bg(rgb(0x302b3d)).gap_4().p_6()
                    .child(div().text_xl().child(heading))
                    .child(format!("{} changed files", comparison.files.len()))
                    .child(format!("Base: {}", comparison.base))
                    .child(format!("Tip: {}", comparison.tip))
                    .child(format!("Operation: {}", &comparison.operation[..12]))
                    .child("Choose a file from the list. Each refresh reads one recorded jj operation.")
                    .into_any_element()
            }
        }
    }
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .bg(rgb(0x1f1f1f))
            .text_color(rgb(0xcccccc))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .bg(rgb(0x263d35))
                    .child(format!(
                        "Jaywalk  /  {}",
                        self.arguments.directory.display()
                    ))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(Button::new("overview").ghost().label("Overview").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.selected = None;
                                    this.code = None;
                                    this.file_error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(Button::new("refresh").ghost().label("Refresh").on_click(
                                cx.listener(|this, _, window, cx| this.refresh(window, cx)),
                            )),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("viewer-layout")
                        .child(
                            resizable_panel()
                                .size(px(340.))
                                .size_range(px(200.)..px(700.))
                                .child(self.render_files(cx)),
                        )
                        .child(resizable_panel().child(self.render_comparison(cx))),
                ),
            )
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_2()
                    .bg(rgb(0x3d3226))
                    .text_sm()
                    .child("Read-only jj snapshot  |  Branch comparison")
                    .child({
                        let total = self.code.as_ref().map_or(0, |(_, _, model)| {
                            model.blocks.iter().filter(|block| block.changed).count()
                        });
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(self.render_view_toggle(cx))
                            .child(if self.code.is_some() {
                                if self.view_mode == ViewMode::Fluid {
                                    format!(
                                        "Change {} of {}",
                                        self.active_change.map_or(0, |index| index + 1),
                                        total
                                    )
                                } else {
                                    format!("{total} changes")
                                }
                            } else {
                                String::new()
                            })
                            .child(
                                Button::new("previous-change")
                                    .ghost()
                                    .label("↑")
                                    .disabled(
                                        self.code.is_none()
                                            || (self.view_mode == ViewMode::Fluid
                                                && self
                                                    .active_change
                                                    .is_none_or(|index| index == 0))
                                            || (self.view_mode != ViewMode::Fluid
                                                && self.standard_diff.is_none()),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.navigate_change(-1, cx)),
                                    ),
                            )
                            .child(
                                Button::new("next-change")
                                    .ghost()
                                    .label("↓")
                                    .disabled(
                                        self.code.is_none()
                                            || (self.view_mode == ViewMode::Fluid
                                                && self
                                                    .active_change
                                                    .is_none_or(|index| index + 1 >= total))
                                            || (self.view_mode != ViewMode::Fluid
                                                && self.standard_diff.is_none()),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.navigate_change(1, cx)),
                                    ),
                            )
                    }),
            )
    }
}

fn main() {
    let arguments = Arguments::parse();
    if arguments.inspect {
        match Comparison::load(
            &arguments.directory,
            arguments.base.as_deref(),
            &arguments.tip,
        ) {
            Ok(comparison) => println!(
                "{} changed files\nbase {}\ntip {}\noperation {}",
                comparison.files.len(),
                comparison.base,
                comparison.tip,
                comparison.operation
            ),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    gpui::application()
        .with_assets(gpui::assets::AllAssets)
        .run(move |cx| {
            gpui::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            configure_code_theme(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Jaywalk".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1500.), px(950.)),
                    cx,
                ))),
                ..Default::default()
            };
            gpui::open_window(options, cx, |window, cx| {
                cx.new(|cx| Viewer::new(arguments, window, cx))
            })
            .expect("could not open viewer window");
            cx.activate(true);
        });
}

fn configure_code_theme(cx: &mut App) {
    use gpui::component::highlighter::{HighlightTheme, ThemeStyle};
    let mut highlight = (*HighlightTheme::default_dark()).clone();
    highlight.name = "VS Code default style".into();
    highlight.style.editor_background = Some(rgb(0x1f1f1f).into());
    highlight.style.editor_foreground = Some(rgb(0xcccccc).into());
    highlight.style.editor_line_number = Some(rgb(0x6e7681).into());
    let token = |hex: &str| -> Option<ThemeStyle> {
        Some(serde_json::from_value(serde_json::json!({"color": hex})).expect("valid theme color"))
    };
    highlight.style.syntax.comment = token("#6a9955");
    highlight.style.syntax.comment_doc = token("#6a9955");
    highlight.style.syntax.keyword = token("#569cd6");
    highlight.style.syntax.string = token("#ce9178");
    highlight.style.syntax.number = token("#b5cea8");
    highlight.style.syntax.function = token("#dcdcaa");
    highlight.style.syntax.type_ = token("#4ec9b0");
    Theme::global_mut(cx).highlight_theme = std::sync::Arc::new(highlight);
}

#[cfg(test)]
mod tests {
    use super::{DiffFile, standard_patch};

    #[test]
    fn standard_widget_parses_full_context_and_unterminated_unicode_changes() {
        for (before, after, additions, deletions) in [
            ("same\nhéllo", "same\nworld", 1, 1),
            ("", "added", 1, 0),
            ("removed", "", 0, 1),
        ] {
            let files = DiffFile::parse(&standard_patch(before, after)).unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].additions(), additions);
            assert_eq!(files[0].deletions(), deletions);
        }
    }
}
