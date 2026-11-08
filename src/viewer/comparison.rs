use super::*;

impl Viewer {
    pub(super) fn unchanged_file_summary(&self) -> &'static str {
        let status = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.status.as_str());
        match status {
            Some("renamed") => "Renamed · contents unchanged",
            Some("copied") => "Copied · contents unchanged",
            Some("added") => "Added empty file",
            Some("removed") => "Deleted empty file",
            _ => "No text changes",
        }
    }

    fn render_file_header(&self) -> AnyElement {
        let file = &self
            .comparison()
            .expect("loaded file has a recorded comparison")
            .files[self.selected_index().expect("loaded file has an index")];
        let text_changed = self
            .code()
            .is_some_and(|code| code.model.blocks.iter().any(|block| block.changed));
        let mut header = div()
            .v_flex()
            .px_3()
            .py_2()
            .bg(rgb(0x302b3d))
            .child(file.after_path.clone());
        let detail = match file.status.as_str() {
            "renamed" => Some(format!(
                "Renamed from {}{}",
                file.before_path,
                if text_changed {
                    ""
                } else {
                    " · contents unchanged"
                }
            )),
            "copied" => Some(format!(
                "Copied from {}{}",
                file.before_path,
                if text_changed {
                    ""
                } else {
                    " · contents unchanged"
                }
            )),
            _ if !text_changed => Some(self.unchanged_file_summary().to_owned()),
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
                        && let Some(Ok(Some(state))) = viewer.code().map(|code| &code.standard)
                    {
                        state.update(cx, |state, cx| state.scroll_to_line(position, cx));
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

    pub(super) fn render_comparison(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(selected) = &self.selected_file {
            match &selected.content {
                LoadState::Loading => return div().p_6().child("Loading file…").into_any_element(),
                LoadState::Failed(error) => {
                    return div().p_6().child(error.clone()).into_any_element();
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
                return div()
                    .v_flex()
                    .size_full()
                    .child(self.render_file_header())
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
            return div()
                .v_flex()
                .size_full()
                .child(self.render_file_header())
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
        match self.comparison_state() {
            LoadState::Loading => div().p_6().child("Loading comparison…").into_any_element(),
            LoadState::Failed(error) => div().p_6().child(error).into_any_element(),
            LoadState::Loaded(comparison) => {
                let heading = self
                    .selected_index()
                    .map(|index| comparison.files[index].after_path.clone())
                    .unwrap_or_else(|| "Changeset overview".into());
                div().v_flex().size_full().bg(rgb(0x302b3d)).gap_4().p_6()
                    .child(div().text_xl().child(heading))
                    .child(if self.is_interdiff() && comparison.files.is_empty() { "No patch changes between these versions.".to_owned() } else { format!("{} changed files", comparison.files.len()) })
                    .child(format!("Base: {}", comparison.base))
                    .child(format!("Tip: {}", comparison.tip))
                    .child(format!("Operation: {}", &comparison.operation[..12]))
                    .child("Choose a file from the list. Each refresh reads one recorded jj operation.")
                    .into_any_element()
            }
        }
    }
}
