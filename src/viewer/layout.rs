use super::*;

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("viewer-root")
            .key_context("Jaywalk")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, action: &commands::ExecuteCommand, window, cx| {
                    match action.command {
                        jaywalk::commands::CommandId::OpenRepository
                        | jaywalk::commands::CommandId::Quit
                        | jaywalk::commands::CommandId::Hide
                        | jaywalk::commands::CommandId::HideOthers
                        | jaywalk::commands::CommandId::ShowAll => cx.propagate(),
                        _ => this.execute_command(action.command, window, cx),
                    }
                }),
            )
            .relative()
            .v_flex()
            .size_full()
            .font_features(typography::features(self.settings.ui.ligatures))
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
                        self.arguments.diff.as_ref().map_or_else(
                            || self.arguments.directory.display().to_string(),
                            |paths| format!("{} ↔ {}", paths[0].display(), paths[1].display())
                        )
                    ))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(self.render_panel_toggles(cx))
                            .child(
                                Button::new("find-file")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::Search)
                                    .tooltip("Find changed file (Cmd+P)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_palette(palette::PaletteKind::Files, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("whole-branch")
                                    .ghost()
                                    .label(if self.arguments.diff.is_some() {
                                        "All files"
                                    } else if !self.supports_evolution() {
                                        "All changes"
                                    } else {
                                        "Whole branch"
                                    })
                                    .disabled(self.repository.ready().is_none())
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
                                    this.focus_evolution_file(None);
                                    cx.notify();
                                }),
                            ))
                            .child(Button::new("refresh").ghost().label("Refresh").on_click(
                                cx.listener(|this, _, window, cx| this.refresh(window, cx)),
                            )),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        self.sidebar_group(cx)
                            .child(
                                resizable_panel()
                                    .visible(!self.layout.saved.history_hidden)
                                    .size(px(self.sidebar_width(true)))
                                    .size_range(
                                        px(self.settings.panels.history.min_width)
                                            ..px(self.settings.panels.history.max_width),
                                    )
                                    .child(
                                        div()
                                            .size_full()
                                            .font(typography::panel_font(
                                                &self.settings.panels.history.font,
                                                cx,
                                            ))
                                            .child(self.render_history_sidebar(cx)),
                                    ),
                            )
                            .child(
                                resizable_panel()
                                    .visible(!self.layout.saved.files_hidden)
                                    .size(px(self.sidebar_width(false)))
                                    .size_range(
                                        px(self.settings.panels.files.min_width)
                                            ..px(self.settings.panels.files.max_width),
                                    )
                                    .child(
                                        div()
                                            .size_full()
                                            .font(typography::panel_font(
                                                &self.settings.panels.files.font,
                                                cx,
                                            ))
                                            .child(self.render_file_sidebar(cx)),
                                    ),
                            )
                            .child(
                                resizable_panel().child(
                                    div()
                                        .v_flex()
                                        .size_full()
                                        .child(self.render_evolution_comparison(cx))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_h_0()
                                                .child(self.render_comparison(cx)),
                                        ),
                                ),
                            ),
                    )
                    .child(self.sidebar_bounds_marker()),
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
                    .child(format!(
                        "{}  |  {}",
                        if self.arguments.diff.is_some() {
                            "Read-only comparison"
                        } else if !self.supports_evolution() {
                            "Read-only Git snapshot"
                        } else {
                            "Read-only jj snapshot"
                        },
                        self.scope_label()
                    ))
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
                                    .selected(self.wrapping.enabled(self.view_mode))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_wrap(window, cx)
                                    })),
                            )
                            .child(self.render_view_toggle(cx))
                            .child(if self.code().is_some() {
                                if total == 0 {
                                    self.unchanged_file_summary().to_owned()
                                } else if self.view_mode == ViewMode::Fluid {
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
            .children(self.config.error.as_ref().map(|message| {
                div()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .bg(rgb(0x542c2c))
                    .child(message.clone())
            }))
            .children(self.live.message.as_ref().map(|message| {
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .bg(rgb(0x3b3426))
                    .child(message.clone())
            }))
            .children(self.render_palette(cx))
    }
}
