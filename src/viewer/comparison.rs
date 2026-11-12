use super::*;
use gpui::base::ElementExt as _;
use gpui::component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui::component::{Icon, Sizable};
use gpui::prelude::FluentBuilder;

impl Viewer {
    pub(super) fn render_find_file_button(&self, cx: &Context<Self>) -> Button {
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
                this.execute_command(jaywalk::commands::CommandId::FindFile, window, cx);
            }))
    }

    pub(super) fn render_review_source(&self, caption: AnyElement) -> AnyElement {
        use jaywalk::commands::CommandId;
        let repository = self.arguments.diff.is_none();
        let aggregate = matches!(
            self.repository.ready().map(|session| &session.scope),
            Some(ComparisonScope::Aggregate)
        );
        let focus = self.focus.clone();
        Button::new("review-source")
            .ghost()
            .small()
            .compact()
            .w_full()
            .min_w_0()
            .justify_start()
            .text_xs()
            .icon(IconName::ChevronDown)
            .child(div().flex_1().min_w_0().truncate().child(caption))
            .tooltip("Choose comparison: all changes, current work, branch or base")
            .disabled(self.repository.ready().is_none())
            .dropdown_menu(move |menu, _, _| {
                let mut entries = vec![
                    (CommandId::AllChanges, aggregate),
                    (CommandId::OpenedComparison, false),
                ];
                if repository {
                    entries.extend([
                        (CommandId::CurrentWork, false),
                        (CommandId::ChooseReview, false),
                        (CommandId::ChooseReviewBase, false),
                    ]);
                }
                context_menu::command_entries(menu.action_context(focus.clone()), &entries)
            })
            .into_any_element()
    }

    pub(super) fn render_review_actions(&self, cx: &Context<Self>) -> AnyElement {
        use jaywalk::commands::CommandId;
        let overview = self.selected_file.is_none();
        let mut actions = div()
            .h_flex()
            .items_center()
            .h_full()
            .px_1()
            .flex_shrink_0()
            .border_l_1()
            .border_color(gpui::component::Theme::global(cx).border);
        if self.layout.saved.files_hidden {
            actions = actions.child(self.render_find_file_button(cx));
        }
        for (id, icon, command, active) in [
            (
                "overview",
                IconName::LayoutDashboard,
                CommandId::Overview,
                overview,
            ),
            ("refresh", IconName::RefreshCw, CommandId::Refresh, false),
        ] {
            actions = actions.child(
                Button::new(id)
                    .ghost()
                    .small()
                    .compact()
                    .icon(crate::appearance::active_icon(icon, active, cx))
                    .toggled(active)
                    .tooltip_with_action(
                        command.label(),
                        &commands::ExecuteCommand { command },
                        None,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.execute_command(command, window, cx)
                    })),
            );
        }
        actions.into_any_element()
    }

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

    fn render_file_header(&self, cx: &Context<Self>) -> AnyElement {
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
            _ if contents_unchanged && file.kind().is_some() => {
                Some(self.unchanged_file_summary().to_owned())
            }
            _ => None,
        };
        let detail = file
            .type_change_notice()
            .or_else(|| match file.content_kind() {
                jaywalk::repository_file::FileKind::Regular => detail,
                jaywalk::repository_file::FileKind::Symlink => {
                    Some("Symbolic link · target".into())
                }
                jaywalk::repository_file::FileKind::Conflict => Some("Conflicted file".into()),
                jaywalk::repository_file::FileKind::Submodule => Some("Git submodule".into()),
                jaywalk::repository_file::FileKind::Other(kind) => {
                    Some(format!("Unsupported entry: {kind}"))
                }
            });
        let theme = gpui::component::Theme::global(cx);
        let split = self.view.effective() == ViewMode::Fluid && self.code().is_some();
        let font_size = self.settings.ui.font_size * 0.85;
        let height = self
            .settings
            .ui
            .panel_header_height
            .max(self.settings.ui.font_size * 1.5);
        let title = div()
            .debug_selector(|| "before-header".into())
            .h_flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .h_full()
            .px(px(super::chrome::PANEL_INSET))
            .when(split, |title| {
                title.w(self.measured_before_width(cx)).flex_shrink_0()
            })
            .when(!split, |title| title.flex_1())
            .children(
                (self.is_retained_preview()
                    || matches!(self.comparison_state(), LoadState::Failed(_)))
                .then(|| Icon::new(IconName::Eye).size(px(font_size))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(file.path().to_owned()),
            )
            .children(detail.map(|detail| {
                div()
                    .max_w(rems(24.))
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(detail)
            }))
            .children(updating.then(|| div().flex_shrink_0().child("Updating…")))
            .when(split, |title| {
                title.child(
                    div()
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground)
                        .child(if self.is_interdiff() {
                            "Previous (rebased)"
                        } else {
                            "Before"
                        }),
                )
            });
        let header = div()
            .h_flex()
            .items_center()
            .min_w_0()
            .flex_shrink_0()
            .h(px(height))
            .text_size(px(font_size))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(title)
            .when(split, |header| {
                header
                    .child(
                        div()
                            .w(px(self.settings.panels.diff.gutter_width))
                            .h_full()
                            .flex_shrink_0()
                            .border_l_1()
                            .border_r_1()
                            .border_color(theme.border),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .px(px(super::chrome::PANEL_INSET))
                            .text_color(theme.muted_foreground)
                            .child(if self.is_interdiff() {
                                "Selected version"
                            } else {
                                "After"
                            }),
                    )
            });
        let target = self.current_file_target();
        let owner = cx.entity().downgrade();
        let focus = self.focus.clone();
        let view = self.view.effective();
        let wrap = self.wrapping.enabled(view);
        let line_numbers = self.line_numbers;
        let collapsed = self.standard_context_collapsed();
        header
            .id("diff-file-header")
            .debug_selector(|| "file-header".into())
            .context_menu(move |menu, _, _| {
                let menu = menu.action_context(focus.clone());
                let menu = match &target {
                    Some(target) => {
                        context_menu::file_menu(menu, target.clone(), owner.clone()).separator()
                    }
                    None => menu,
                };
                context_menu::diff_commands(menu, view, wrap, line_numbers, collapsed)
            })
            .into_any_element()
    }

    pub(super) fn render_standard_input(&self, cx: &mut Context<Self>) -> AnyElement {
        let viewer = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                // Restore after the widget has updated its font/wrap layout.
                // Scrolling before remeasurement can reinterpret old row heights.
                let _ = viewer.update(cx, |viewer, cx| {
                    if viewer.view.is_single_file() {
                        if let Some((_, line, horizontal)) = viewer.live.pending_position.take()
                            && let Some(code) = viewer.code()
                        {
                            let geometry = comparison_geometry(
                                code.before.read(cx),
                                code.after.read(cx),
                                &code.model,
                            );
                            let y = geometry.before[line.min(geometry.before.len() - 1)];
                            // Let the editor finish publishing/clearing its paint state
                            // before setting the deferred scroll for the next layout.
                            let editor = code.before.clone();
                            window.defer(cx, move |_, cx| {
                                editor.update(cx, |editor, cx| {
                                    editor.set_scroll_offset(point(px(-horizontal), px(-y)), cx)
                                })
                            });
                        }
                        if let Some(position) = viewer.config.pending_standard.take()
                            && let Some(code) = viewer.code()
                        {
                            let geometry = comparison_geometry(
                                code.before.read(cx),
                                code.after.read(cx),
                                &code.model,
                            );
                            let y = geometry.before[position
                                .line()
                                .saturating_sub(1)
                                .min(geometry.before.len() - 1)];
                            let editor = code.before.clone();
                            window.defer(cx, move |_, cx| {
                                editor.update(cx, |editor, cx| {
                                    let offset = editor.scroll_offset();
                                    editor.set_scroll_offset(point(offset.x, px(-y)), cx);
                                })
                            });
                        }
                    }

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
                    viewer.reveal_initial_change(window, cx);
                    // Navigation records only the viewport after layout restoration.
                    cx.defer_in(window, |viewer, _, cx| viewer.sync_navigation(cx));
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
                    .icon(crate::appearance::active_icon(
                        icon,
                        self.view.effective() == mode,
                        cx,
                    ))
                    .tooltip(label)
                    .toggled(self.view.effective() == mode)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(mode, cx))),
            );
        }
        group
    }

    pub(super) fn render_comparison(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let file_background = if self.view.effective() == ViewMode::Fluid {
            theme
                .highlight_theme
                .style
                .editor_background
                .unwrap_or_else(|| theme.input_background())
        } else {
            theme.background
        };
        let file_surface = div()
            .key_context("JaywalkDiff")
            .v_flex()
            .size_full()
            .bg(file_background);
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
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .p_6()
                                .child(feedback::error_message("file-error", error)),
                        )
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
            if self.view.effective() != ViewMode::Fluid {
                let body = if let Err(error) = standard {
                    div()
                        .p_6()
                        .child(feedback::error_message("diff-error", error))
                        .into_any_element()
                } else if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
                    Diff::new(state)
                        .colors(crate::appearance::diff_colors(cx))
                        .border_0()
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
            let before_viewport = Rc::new(Cell::new(None));
            let after_viewport = Rc::new(Cell::new(None));
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
                                                .on_prepaint(self.measure_before_width(cx))
                                                .child(
                                                    div()
                                                        .relative()
                                                        .v_flex()
                                                        .flex_1()
                                                        .min_h_0()
                                                        .child(
                                                            Editor::new(left)
                                                                .text_size(px(self.code_font_size))
                                                                .font_features(
                                                                    typography::features(
                                                                        self.settings
                                                                            .editor
                                                                            .ligatures,
                                                                    ),
                                                                )
                                                                .readonly(true)
                                                                .bordered(false)
                                                                .flex_1()
                                                                .min_h_0(),
                                                        )
                                                        .child(fluid_view::measure_viewport(
                                                            before_viewport.clone(),
                                                        )),
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
                                            div()
                                                .size_full()
                                                .border_l_1()
                                                .border_r_1()
                                                .border_color(
                                                    gpui::component::Theme::global(cx).border,
                                                )
                                                .debug_selector(|| "fluid-gutter".into())
                                                .cursor(CursorStyle::ResizeUpDown)
                                                .child(
                                                    canvas(
                                                        move |bounds, _, _| {
                                                            measured_gutter.set(Some(bounds))
                                                        },
                                                        |_, _, _, _| {},
                                                    )
                                                    .size_full(),
                                                ),
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
                                            div().v_flex().size_full().child(
                                                div()
                                                    .relative()
                                                    .v_flex()
                                                    .flex_1()
                                                    .min_h_0()
                                                    .child(
                                                        Editor::new(right)
                                                            .text_size(px(self.code_font_size))
                                                            .font_features(typography::features(
                                                                self.settings.editor.ligatures,
                                                            ))
                                                            .readonly(true)
                                                            .bordered(false)
                                                            .flex_1()
                                                            .min_h_0(),
                                                    )
                                                    .child(fluid_view::measure_viewport(
                                                        after_viewport.clone(),
                                                    )),
                                            ),
                                        ),
                                ),
                        )
                        .child(div().absolute().inset_0().child(self.render_fluid_gutter(
                            gutter_bounds,
                            before_viewport,
                            after_viewport,
                            cx,
                        ))),
                )
                .into_any_element();
        }
        self.render_overview(cx)
    }
}
