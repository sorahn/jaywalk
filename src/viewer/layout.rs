use super::*;
use gpui::component::tooltip::Tooltip;

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("viewer-root")
            .key_context("Jaywalk")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, action: &commands::ExecuteCommand, window, cx| {
                    match action.command {
                        jaywalk::commands::CommandId::About
                        | jaywalk::commands::CommandId::OpenRepository
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
            .bg(gpui::component::Theme::global(cx).background)
            .text_color(gpui::component::Theme::global(cx).foreground)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .h(px(self
                        .settings
                        .ui
                        .toolbar_height
                        .max(self.settings.ui.font_size * 1.75)))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(gpui::component::Theme::global(cx).border)
                    .bg(gpui::component::Theme::global(cx).muted)
                    .child(div().flex_1().min_w_0().truncate().child(format!(
                        "Jaywalk  /  {}",
                        self.arguments.diff.as_ref().map_or_else(
                            || self.arguments.directory.display().to_string(),
                            |paths| format!("{} ↔ {}", paths[0].display(), paths[1].display())
                        )
                    )))
                    .child(
                        div()
                            .h_flex()
                            .flex_shrink_0()
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
                                    this.remember_reading(cx);
                                    this.clear_file();
                                    this.rebuild_browser(cx);
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
                    .child(self.render_workbench(f32::from(window.bounds().size.width), cx))
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
                    .gap_3()
                    .flex_shrink_0()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_3()
                            .flex_1()
                            .min_w_0()
                            .child(div().min_w_0().truncate().child(format!(
                                "{}  |  {}",
                                if self.arguments.diff.is_some() {
                                    "Read-only comparison"
                                } else if !self.supports_evolution() {
                                    "Read-only Git snapshot"
                                } else {
                                    "Read-only jj snapshot"
                                },
                                self.scope_label()
                            )))
                            .children(self.status_notice()),
                    )
                    .child({
                        let total = self.code().map_or(0, |CodeComparison { model, .. }| {
                            model.blocks.iter().filter(|block| block.changed).count()
                        });
                        div()
                            .h_flex()
                            .items_center()
                            .flex_shrink_0()
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
            .children(self.render_palette(cx))
    }
}

impl Viewer {
    fn status_notice(&self) -> Option<AnyElement> {
        let (message, color) = if let Some(message) = &self.config.error {
            (message, 0xf48771)
        } else {
            let message = self.live.message.as_ref()?;
            if message == "Updating recorded changes…" && !self.loading_visible(LoadingKind::Live)
            {
                return None;
            }
            (message, 0xe5c07b)
        };
        let tooltip = message.clone();
        Some(
            div()
                .id("status-notice")
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(rgb(color))
                .truncate()
                .child(message.clone())
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .into_any_element(),
        )
    }
}
