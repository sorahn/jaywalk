use super::*;

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("viewer-root")
            .key_context("Jaywalk")
            .on_action(cx.listener(|this, _: &ShowWholeBranch, window, cx| {
                this.show_whole_branch(window, cx)
            }))
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
                            .child(
                                Button::new("whole-branch")
                                    .ghost()
                                    .label("Whole branch")
                                    .selected(matches!(
                                        self.repository.ready().map(|session| &session.scope),
                                        Some(ComparisonScope::WholeBranch)
                                    ))
                                    .tooltip("Whole branch (Cmd+Shift+B)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_whole_branch(window, cx)
                                    })),
                            )
                            .child(Button::new("overview").ghost().label("Overview").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.clear_file();
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
                                .size(px(280.))
                                .size_range(px(180.)..px(700.))
                                .child(self.render_history(cx)),
                        )
                        .child(
                            resizable_panel()
                                .size(px(260.))
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
                    .child(format!("Read-only jj snapshot  |  {}", self.scope_label()))
                    .child({
                        let total = self.code().map_or(0, |CodeComparison { model, .. }| {
                            model.blocks.iter().filter(|block| block.changed).count()
                        });
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("word-wrap")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::TextWrap)
                                    .tooltip("Word wrap")
                                    .selected(self.wrap)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_wrap(window, cx)
                                    })),
                            )
                            .child(self.render_view_toggle(cx))
                            .child(if self.code().is_some() {
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
                                        self.code().is_none()
                                            || (self.view_mode == ViewMode::Fluid
                                                && self
                                                    .active_change
                                                    .is_none_or(|index| index == 0))
                                            || (self.view_mode != ViewMode::Fluid
                                                && self.code().is_none_or(|code| {
                                                    !matches!(code.standard, Ok(Some(_)))
                                                })),
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
                                        self.code().is_none()
                                            || (self.view_mode == ViewMode::Fluid
                                                && self
                                                    .active_change
                                                    .is_none_or(|index| index + 1 >= total))
                                            || (self.view_mode != ViewMode::Fluid
                                                && self.code().is_none_or(|code| {
                                                    !matches!(code.standard, Ok(Some(_)))
                                                })),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.navigate_change(1, cx)),
                                    ),
                            )
                    }),
            )
    }
}
