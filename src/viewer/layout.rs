use super::*;
use gpui::component::Sizable;
use gpui::component::menu::DropdownMenu as _;
use gpui::component::tooltip::Tooltip;

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
                        .max(self.settings.ui.font_size * 1.5)))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(gpui::component::Theme::global(cx).border)
                    .bg(gpui::component::Theme::global(cx).muted)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .w_full()
                            .flex_shrink_0()
                            .gap_1()
                            .child(div().flex_1())
                            .child(
                                Button::new("find-file")
                                    .ghost()
                                    .small()
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
                                    .small()
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
                                    .small()
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
                                    .small()
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
                            .child(
                                Button::new("overview")
                                    .ghost()
                                    .small()
                                    .label("Overview")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.show_overview(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("refresh")
                                    .ghost()
                                    .small()
                                    .label("Refresh")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.invalidate_overview();
                                        this.refresh(window, cx);
                                    })),
                            ),
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
            .child(self.render_footer(window, cx))
            .children(self.render_palette(cx))
            .into_any_element()
    }
}

impl Viewer {
    pub(super) fn render_change_counter(
        &self,
        total: usize,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
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
    pub(super) fn status_notice(&self, cx: &App) -> Option<AnyElement> {
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
