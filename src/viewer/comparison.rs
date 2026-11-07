use super::*;

impl Viewer {
    pub(super) fn render_standard_input(&self, cx: &mut Context<Self>) -> AnyElement {
        let viewer = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
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
                let heading = self
                    .comparison()
                    .expect("loaded file has a recorded comparison")
                    .files[self.selected_index().expect("loaded file has an index")]
                .after_path
                .clone();
                let body = if let Err(error) = standard {
                    div().p_6().child(error.clone()).into_any_element()
                } else if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
                    Diff::new(state)
                        .header_visible(false)
                        .soft_wrap(self.wrapping.inline)
                        .soft_wrap_sides(self.wrapping.before, self.wrapping.after)
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
                .child(
                    div().px_3().py_2().bg(rgb(0x302b3d)).child(
                        self.comparison()
                            .expect("loaded file has a recorded comparison")
                            .files[self.selected_index().expect("loaded file has an index")]
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
                            self.diff_group(cx)
                                .child(
                                    resizable_panel()
                                        .size_range(
                                            px(self.settings.panels.diff.min_code_width)
                                                ..Pixels::MAX,
                                        )
                                        .child(
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
                                        .size_range(
                                            px(self.settings.panels.diff.min_code_width)
                                                ..Pixels::MAX,
                                        )
                                        .child(
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
