use super::*;
use gpui::component::Sizable;
use gpui::component::menu::DropdownMenu as _;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::FluentBuilder;

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .id("viewer-root")
            .key_context("Jaywalk")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, action: &commands::ExecuteCommand, window, cx| {
                    match action.command {
                        jaywalk::commands::CommandId::About
                        | jaywalk::commands::CommandId::Quit
                        | jaywalk::commands::CommandId::Hide
                        | jaywalk::commands::CommandId::HideOthers
                        | jaywalk::commands::CommandId::ShowAll => cx.propagate(),
                        _ => this.execute_command(action.command, window, cx),
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| {
                    this.navigate_view(false, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| {
                    this.navigate_view(true, window, cx);
                    cx.stop_propagation();
                }),
            )
            .relative()
            .v_flex()
            .font_features(typography::features(self.settings.ui.ligatures))
            .bg(gpui::component::Theme::global(cx).background)
            .text_color(gpui::component::Theme::global(cx).foreground);
        if self.is_welcome() {
            return root
                .size_full()
                .child(self.render_welcome(cx))
                .children(self.render_palette(cx))
                .into_any_element();
        }
        root.size_full()
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
                    .child(self.render_navigation_buttons(cx))
                    .child(div().flex_1().min_w_0().truncate().child(format!(
                        "Jaywalk  /  {}",
                        self.arguments.diff.as_ref().map_or_else(
                            || self.arguments.directory().display().to_string(),
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
                                    .tooltip_with_action(
                                        "Find any repository file",
                                        &commands::ExecuteCommand {
                                            command: jaywalk::commands::CommandId::FindFile,
                                        },
                                        None,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_palette(palette::PaletteKind::Files, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("current-work")
                                    .ghost()
                                    .label("Current work")
                                    .disabled(self.arguments.diff.is_some())
                                    .tooltip_with_action(
                                        "Return to the latest workspace comparison",
                                        &commands::ExecuteCommand {
                                            command: jaywalk::commands::CommandId::CurrentWork,
                                        },
                                        None,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.execute_command(
                                            jaywalk::commands::CommandId::CurrentWork,
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new("whole-branch")
                                    .ghost()
                                    .label(if self.arguments.diff.is_some() {
                                        "All files"
                                    } else {
                                        "All changes"
                                    })
                                    .disabled(self.repository.ready().is_none())
                                    .selected(matches!(
                                        self.repository.ready().map(|session| &session.scope),
                                        Some(ComparisonScope::Aggregate)
                                    ))
                                    .tooltip_with_action(
                                        "All changes",
                                        &commands::ExecuteCommand {
                                            command: jaywalk::commands::CommandId::AllChanges,
                                        },
                                        None,
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.all_review_changes(None, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("review-source")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::ChevronDown)
                                    .tooltip("Choose review branch and base")
                                    .disabled(self.arguments.diff.is_some())
                                    .dropdown_menu(|menu, _, _| {
                                        context_menu::command_entries(
                                            menu,
                                            &[
                                                (jaywalk::commands::CommandId::ChooseReview, false),
                                                (
                                                    jaywalk::commands::CommandId::ChooseReviewBase,
                                                    false,
                                                ),
                                                (jaywalk::commands::CommandId::CurrentWork, false),
                                                (
                                                    jaywalk::commands::CommandId::OpenedComparison,
                                                    false,
                                                ),
                                            ],
                                        )
                                    }),
                            )
                            .child(Button::new("overview").ghost().label("Overview").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.show_overview(window, cx);
                                }),
                            ))
                            .child(Button::new("refresh").ghost().label("Refresh").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.invalidate_overview();
                                    this.refresh(window, cx);
                                }),
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
                    .px_2()
                    .h(px(self.settings.footer_height()))
                    .border_t_1()
                    .border_color(gpui::component::Theme::global(cx).border)
                    .bg(gpui::component::Theme::global(cx).muted)
                    .text_size(px(self.settings.ui.footer_font_size))
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
                            .children(self.status_notice(cx)),
                    )
                    .child({
                        let total = self.code().map_or(0, |CodeComparison { model, .. }| {
                            model.blocks.iter().filter(|block| block.changed).count()
                        });
                        div()
                            .h_flex()
                            .items_center()
                            .flex_shrink_0()
                            .when(self.selected_file.is_none(), |controls| controls.hidden())
                            .gap_2()
                            .child(
                                Button::new("word-wrap")
                                    .ghost()
                                    .with_size(px(self.settings.ui.footer_control_height))
                                    .compact()
                                    .icon(crate::appearance::active_icon(
                                        IconName::TextWrap,
                                        self.wrapping.enabled(self.view.effective()),
                                        cx,
                                    ))
                                    .tooltip("Word wrap")
                                    .toggled(self.wrapping.enabled(self.view.effective()))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_wrap(window, cx)
                                    })),
                            )
                            .child(self.render_view_toggle(cx))
                            .child(self.render_change_counter(total, window, cx))
                            .child(
                                Button::new("previous-change")
                                    .ghost()
                                    .with_size(px(self.settings.ui.footer_control_height))
                                    .label("↑")
                                    .disabled(
                                        self.code().is_none()
                                            || total == 0
                                            || (self.view.effective() != ViewMode::Fluid
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
                                    .with_size(px(self.settings.ui.footer_control_height))
                                    .label("↓")
                                    .disabled(
                                        self.code().is_none()
                                            || total == 0
                                            || (self.view.effective() != ViewMode::Fluid
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
            .into_any_element()
    }
}

impl Viewer {
    fn render_change_counter(&self, total: usize, window: &Window, cx: &App) -> AnyElement {
        if self.view.effective() == ViewMode::Fluid && total > 0 && self.code().is_some() {
            let mut features = typography::features(self.settings.ui.ligatures)
                .0
                .as_ref()
                .clone();
            features.push(("tnum".into(), 1));
            let features = FontFeatures(std::sync::Arc::new(features));
            let font = Font {
                family: gpui::component::Theme::global(cx).font_family.clone(),
                features: features.clone(),
                ..Default::default()
            };
            let font_size = px(self.settings.ui.footer_font_size);
            let digits = total.to_string().len();
            // Measure every repeated digit rather than assuming 8 is widest:
            // custom UI fonts may lack tabular figures or use different widths.
            let width = ('0'..='9')
                .map(|digit| {
                    let label = format!("Change {} of {total}", digit.to_string().repeat(digits));
                    let run = TextRun {
                        len: label.len(),
                        font: font.clone(),
                        ..Default::default()
                    };
                    window
                        .text_system()
                        .shape_line(label.into(), font_size, &[run], None)
                        .width
                })
                .fold(px(0.), Pixels::max);
            return div()
                .w(px(f32::from(width).ceil()))
                .flex_shrink_0()
                .font_features(features)
                .child(format!(
                    "Change {} of {total}",
                    self.active_change.map_or(0, |index| index + 1).min(total)
                ))
                .into_any_element();
        }
        let label = if self.code().is_none() {
            String::new()
        } else if total == 0 {
            self.unchanged_file_summary().to_owned()
        } else {
            format!("{total} changes")
        };
        div()
            .max_w(px(self.settings.ui.footer_font_size * 18.))
            .truncate()
            .child(label)
            .into_any_element()
    }
    fn status_notice(&self, cx: &App) -> Option<AnyElement> {
        let theme = gpui::component::Theme::global(cx);
        let (message, color): (&str, _) = if let Some(message) = &self.config.error {
            (message, theme.danger)
        } else if let Some(message) = self
            .overview
            .error
            .as_ref()
            .filter(|_| self.selected_file.is_none())
        {
            (message, theme.warning)
        } else if let Some(message) = &self.layout.error {
            (message, theme.warning)
        } else if self.selected_file.is_none()
            && (self.loading_visible(LoadingKind::Overview)
                || self.loading_visible(LoadingKind::Comparison))
        {
            ("Updating overview…", theme.warning)
        } else {
            let message = self.live.message.as_ref()?;
            if message == "Updating recorded changes…" && !self.loading_visible(LoadingKind::Live)
            {
                return None;
            }
            (message, theme.warning)
        };
        let tooltip = message.to_owned();
        // Truncation ellipsizes each line; explicit diagnostic newlines still
        // create multiple rows unless the visible label is projected first.
        let summary = message
            .split(['\n', '\r'])
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("")
            .to_owned();
        Some(
            div()
                .id("status-notice")
                .flex_1()
                .min_w_0()
                .text_size(px(self.settings.ui.footer_font_size))
                .text_color(color)
                .truncate()
                .child(summary)
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .into_any_element(),
        )
    }
}
