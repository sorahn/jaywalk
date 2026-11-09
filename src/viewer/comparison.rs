use super::*;
use gpui::component::{Icon, Sizable};

impl Viewer {
    pub(super) fn unchanged_file_summary(&self) -> &'static str {
        let status = self
            .selected_file
            .as_ref()
            .map(|selected| &selected.file)
            .and_then(|file| file.status());
        match status {
            Some("renamed") => "Renamed · contents unchanged",
            Some("copied") => "Copied · contents unchanged",
            Some("added") => "Added empty file",
            Some("removed") => "Deleted empty file",
            _ => "No text changes",
        }
    }

    fn render_file_header(&self, cx: &App) -> AnyElement {
        let selected = self
            .selected_file
            .as_ref()
            .expect("loaded file has metadata");
        let file = &selected.file;
        let contents_unchanged = self
            .code()
            .is_some_and(|code| code.model.blocks.iter().all(|block| !block.changed));
        let updating = self.loading_visible(LoadingKind::Comparison)
            || self.loading_visible(LoadingKind::File)
            || self.loading_visible(LoadingKind::Live);
        let mut header = div()
            .v_flex()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(gpui::component::Theme::global(cx).border)
            .bg(gpui::component::Theme::global(cx).sidebar)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .children(
                        (self.is_retained_preview()
                            || matches!(self.comparison_state(), LoadState::Failed(_)))
                        .then(|| {
                            div().flex_shrink_0().child(
                                Icon::new(IconName::Eye).size(px(self.settings.ui.font_size)),
                            )
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(file.path().to_owned()),
                    )
                    .children(updating.then(|| div().flex_shrink_0().text_sm().child("Updating…"))),
            );
        let detail = match file.status().unwrap_or("unchanged") {
            "renamed" => Some(format!(
                "Renamed from {}{}",
                file.before_path(),
                if contents_unchanged {
                    " · contents unchanged"
                } else {
                    ""
                }
            )),
            "copied" => Some(format!(
                "Copied from {}{}",
                file.before_path(),
                if contents_unchanged {
                    " · contents unchanged"
                } else {
                    ""
                }
            )),
            _ if contents_unchanged => Some(self.unchanged_file_summary().to_owned()),
            _ => None,
        };
        if let Some(detail) = detail {
            header = header.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xb6c4d8))
                    .whitespace_normal()
                    .child(detail),
            );
        }
        header.into_any_element()
    }

    pub(super) fn render_standard_input(&self, cx: &mut Context<Self>) -> AnyElement {
        let viewer = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                // Restore after the widget has updated its font/wrap layout.
                // Scrolling before remeasurement can reinterpret old row heights.
                let _ = viewer.update(cx, |viewer, cx| {
                    if let Some(position) = viewer.config.pending_standard.take()
                        && let Some(Ok(Some(state))) =
                            viewer.code().map(|code| code.standard.clone())
                    {
                        state.update(cx, |state, cx| state.scroll_to_line(position, cx));
                    }
                    if let Some(horizontal) = viewer.config.pending_horizontal.take()
                        && let Some(Ok(Some(state))) =
                            viewer.code().map(|code| code.standard.clone())
                    {
                        state.update(cx, |state, cx| state.scroll_horizontal(px(horizontal), cx));
                        viewer.horizontal_position = horizontal;
                    }
                });
                let viewer = viewer.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || !bounds.contains(&event.position) {
                        return;
                    }
                    if let Some(delta) = horizontal_wheel(event, px(20.)) {
                        let _ = viewer.update(cx, |viewer, cx| viewer.scroll_horizontal(delta, cx));
                        cx.stop_propagation();
                    }
                });
            },
        )
        .size_full()
        .into_any_element()
    }

    pub(super) fn render_view_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut group = div()
            .h_flex()
            .items_center()
            .gap_1()
            .rounded_sm()
            .bg(gpui::component::Theme::global(cx).secondary);
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
                    .with_size(px(self.settings.ui.footer_control_height))
                    .compact()
                    .icon(icon)
                    .tooltip(label)
                    .selected(self.view_mode == mode)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(mode, cx))),
            );
        }
        group
    }

    pub(super) fn render_comparison(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let file_background = if self.view_mode == ViewMode::Fluid {
            theme
                .highlight_theme
                .style
                .editor_background
                .unwrap_or_else(|| theme.input_background())
        } else {
            theme.background
        };
        let file_surface = div().v_flex().size_full().bg(file_background);
        if let Some(selected) = &self.selected_file {
            match &selected.content {
                LoadState::Loading => {
                    return file_surface
                        .child(self.render_file_header(cx))
                        .child(div().flex_1().min_h_0().p_6().child(
                            if self.loading_visible(LoadingKind::File) {
                                "Loading file…"
                            } else {
                                ""
                            },
                        ))
                        .into_any_element();
                }
                LoadState::Failed(error) => {
                    return file_surface
                        .child(self.render_file_header(cx))
                        .child(div().flex_1().min_h_0().p_6().child(error.clone()))
                        .into_any_element();
                }
                LoadState::Loaded(_) => {}
            }
        }
        if let Some(CodeComparison {
            before: left,
            after: right,
            standard,
            ..
        }) = self.code()
        {
            if self.view_mode != ViewMode::Fluid {
                let body = if let Err(error) = standard {
                    div().p_6().child(error.clone()).into_any_element()
                } else if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
                    Diff::new(state)
                        .code_font(
                            gpui::component::Theme::global(cx).mono_font_family.clone(),
                            px(self.code_font_size),
                        )
                        .line_number(self.line_numbers)
                        .font_features(typography::features(self.settings.editor.ligatures))
                        .header_visible(false)
                        .soft_wrap(self.wrapping.inline)
                        .soft_wrap_sides(self.wrapping.before, self.wrapping.after)
                        .size_full()
                        .into_any_element()
                } else {
                    Editor::new(left)
                        .text_size(px(self.code_font_size))
                        .font_features(typography::features(self.settings.editor.ligatures))
                        .readonly(true)
                        .bordered(false)
                        .h_full()
                        .into_any_element()
                };
                return file_surface
                    .child(self.render_file_header(cx))
                    .child(
                        div().relative().flex_1().min_h_0().child(body).child(
                            div()
                                .absolute()
                                .inset_0()
                                .child(self.render_standard_input(cx)),
                        ),
                    )
                    .into_any_element();
            }
            let gutter_bounds = Rc::new(Cell::new(None));
            let measured_gutter = gutter_bounds.clone();
            return file_surface
                .child(self.render_file_header(cx))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(
                            self.diff_group(cx)
                                .child(
                                    resizable_panel()
                                        .size(self.initial_code_width(true))
                                        .size_range(
                                            px(self.settings.panels.diff.min_code_width)
                                                ..Pixels::MAX,
                                        )
                                        .child(
                                            div()
                                                .v_flex()
                                                .size_full()
                                                .child(div().px_3().py_1().child(
                                                    if self.is_interdiff() {
                                                        "PREVIOUS (rebased)"
                                                    } else {
                                                        "BEFORE"
                                                    },
                                                ))
                                                .child(
                                                    Editor::new(left)
                                                        .text_size(px(self.code_font_size))
                                                        .font_features(typography::features(
                                                            self.settings.editor.ligatures,
                                                        ))
                                                        .readonly(true)
                                                        .bordered(false)
                                                        .h_full(),
                                                ),
                                        ),
                                )
                                .child(
                                    resizable_panel()
                                        .size(px(self.settings.panels.diff.gutter_width))
                                        .size_range(
                                            px(self.settings.panels.diff.gutter_width)
                                                ..px(self.settings.panels.diff.gutter_width + 1.),
                                        )
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
                                    resizable_panel()
                                        .size(self.initial_code_width(false))
                                        .size_range(
                                            px(self.settings.panels.diff.min_code_width)
                                                ..Pixels::MAX,
                                        )
                                        .child(
                                            div()
                                                .v_flex()
                                                .size_full()
                                                .child(div().px_3().py_1().child(
                                                    if self.is_interdiff() {
                                                        "SELECTED VERSION"
                                                    } else {
                                                        "AFTER"
                                                    },
                                                ))
                                                .child(
                                                    Editor::new(right)
                                                        .text_size(px(self.code_font_size))
                                                        .font_features(typography::features(
                                                            self.settings.editor.ligatures,
                                                        ))
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
        // Keep the overview surface identical in pending, failed, and ready
        // states instead of exposing the dark parent for a loading frame.
        let overview = div()
            .v_flex()
            .size_full()
            .bg(gpui::component::Theme::global(cx).background)
            .gap_4()
            .p_6();
        match self.comparison_state() {
            LoadState::Loading => overview
                .child(if self.loading_visible(LoadingKind::Comparison) {
                    "Loading comparison…"
                } else {
                    ""
                })
                .into_any_element(),
            LoadState::Failed(error) => overview.child(error).into_any_element(),
            LoadState::Loaded(comparison) => {
                let heading = self
                    .selected_path()
                    .unwrap_or_else(|| "Changeset overview".into());
                overview
                    .child(div().text_xl().child(heading))
                    .child(if self.is_interdiff() && comparison.files.is_empty() {
                        "No patch changes between these versions.".to_owned()
                    } else {
                        format!("{} changed files", comparison.files.len())
                    })
                    .child(format!("Base: {}", comparison.base))
                    .child(format!("Tip: {}", comparison.tip))
                    .children(comparison.supports_evolution().then(|| {
                        div().child(format!(
                            "Operation: {}",
                            &comparison.operation[..comparison.operation.len().min(12)]
                        ))
                    }))
                    .child(if comparison.supports_evolution() {
                        "Choose a file from the list. Each refresh reads one recorded jj operation."
                    } else {
                        "Choose a file from the list. Use Refresh to reread the comparison."
                    })
                    .into_any_element()
            }
        }
    }
}
