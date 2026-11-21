//! Repository-free workspace. Panel preferences remain untouched underneath it.
use super::*;
use gpui::component::menu::ContextMenuExt as _;
use gpui::prelude::FluentBuilder as _;

#[derive(Default)]
pub(super) struct WelcomeState {
    error: Option<String>,
}
impl WelcomeState {
    pub(super) fn failed(error: String) -> Self {
        Self { error: Some(error) }
    }
}
impl Viewer {
    pub(crate) fn is_welcome(&self) -> bool {
        self.page == pages::Page::Welcome
    }
    pub(crate) fn report_open_error(&mut self, error: String, cx: &mut Context<Self>) {
        if self.page != pages::Page::Comparison {
            self.welcome.get_or_insert_default().error = Some(error);
            self.page = pages::Page::Welcome;
        } else {
            self.live.notice = Some(super::live::LiveNotice::Diagnostic(error));
        }
        cx.notify();
    }
    pub(crate) fn repository_directory(&self) -> Option<std::path::PathBuf> {
        (self.arguments.diff.is_none())
            .then(|| {
                self.repository
                    .ready()
                    .filter(|session| session.home.hosted().is_none())
                    .map(|session| session.home.root().to_path_buf())
            })
            .flatten()
    }
    pub(super) fn open_repository_path(
        &mut self,
        directory: std::path::PathBuf,
        disposition: crate::windows::OpenDisposition,
        cx: &mut Context<Self>,
    ) {
        if let Some(welcome) = &mut self.welcome {
            welcome.error = None;
        }
        match disposition {
            crate::windows::OpenDisposition::Configured => crate::open_arguments(
                self.arguments.for_repository(directory),
                Some(cx.entity().downgrade()),
                true,
                cx,
            ),
            crate::windows::OpenDisposition::NewWindow => crate::open_arguments_in_new_window(
                self.arguments.for_repository(directory),
                Some(cx.entity().downgrade()),
                true,
                cx,
            ),
        }
        cx.notify();
    }
    pub(super) fn render_welcome(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        use jaywalk::commands::CommandId;
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        // Leave enough room for readable action labels and repository paths,
        // including when the user increases the application font size.
        let wide = width >= self.settings.ui.font_size * 54.;
        let recent = cx.global::<crate::recents::Recents>();
        let header = div()
            .id("welcome-header")
            .debug_selector(|| "welcome-header".into())
            .h_flex()
            .items_center()
            .gap(crate::spacing::space(4., cx))
            .pb(crate::spacing::space(4., cx))
            .border_b_1()
            .border_color(theme.border)
            .child(
                gpui::component::Icon::new(IconName::GitCompareArrows)
                    .size(px(36.))
                    .text_color(theme.primary)
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .v_flex()
                    .min_w_0()
                    .gap(crate::spacing::edge(cx))
                    .child(div().text_2xl().font_medium().child("Welcome to Jaywalk"))
                    .child(div().text_color(muted).child(
                        "Explore Git and Jujutsu history, compare files, and review pull requests.",
                    )),
            );
        let mut start = div()
            .id("welcome-start")
            .debug_selector(|| "welcome-start".into())
            .v_flex()
            .min_w_0()
            .gap(crate::spacing::edge(cx))
            .when(wide, |el| el.flex_1())
            .child(div().text_lg().font_medium().child("Getting started"));
        for (id, command, description) in [
            (
                "welcome-open",
                CommandId::OpenRepository,
                "Browse a local repository’s history and working changes.",
            ),
            (
                "welcome-compare",
                CommandId::ComparePaths,
                "Compare two files or folders without opening a repository.",
            ),
            (
                "welcome-pull-requests",
                CommandId::OpenPullRequest,
                "Find a pull request and open it in the diff viewer.",
            ),
            (
                "welcome-accounts",
                CommandId::Accounts,
                "Connect GitHub or Bitbucket to find your pull requests.",
            ),
        ] {
            start = start.child(
                div()
                    .v_flex()
                    .gap(crate::spacing::interactive(cx))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap(crate::spacing::edge(cx))
                            .child(
                                Button::new(id)
                                    .ghost()
                                    .label(command.label())
                                    .tooltip_with_action(
                                        command.label(),
                                        &commands::ExecuteCommand { command },
                                        None,
                                    )
                                    .on_click(cx.listener(move |viewer, _, window, cx| {
                                        viewer.execute_command(command, window, cx)
                                    })),
                            )
                            .child(super::palette::ShortcutHint(command)),
                    )
                    .child(
                        div()
                            .px(crate::spacing::edge(cx))
                            .text_sm()
                            .text_color(muted)
                            .child(description),
                    ),
            );
        }
        start = start.child(
            div()
                .mt(crate::spacing::edge(cx))
                .pt(crate::spacing::space(4., cx))
                .border_t_1()
                .border_color(theme.border)
                .v_flex()
                .gap(crate::spacing::edge(cx))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("From your terminal, run jw . to open the current repository."),
                ),
        );
        for (label, command) in [
            ("Find a command", CommandId::ShowCommands),
            ("Explore picker modes (?)", CommandId::PickerHelp),
        ] {
            start = start.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::edge(cx))
                    .child(
                        Button::new(format!("welcome-picker-{}", command.key()))
                            .ghost()
                            .label(label)
                            .on_click(cx.listener(move |viewer, _, window, cx| {
                                viewer.execute_command(command, window, cx)
                            })),
                    )
                    .child(super::palette::ShortcutHint(command)),
            );
        }
        start = start.child(
            div()
                .id("welcome-file-shortcut")
                .debug_selector(|| "welcome-file-shortcut".into())
                .h_flex()
                .items_center()
                .flex_wrap()
                .gap(crate::spacing::edge(cx))
                .px(crate::spacing::edge(cx))
                .child(
                    div()
                        .text_color(muted)
                        .child("Find files in an open repository"),
                )
                .child(super::palette::ShortcutHint(CommandId::FindFile)),
        );
        let mut recents = div()
            .id("welcome-recents")
            .debug_selector(|| "welcome-recents".into())
            .v_flex()
            .min_w_0()
            .gap(crate::spacing::interactive(cx))
            .when(wide, |el| el.flex_1())
            .child(
                div()
                    .mb(crate::spacing::interactive(cx))
                    .text_lg()
                    .font_medium()
                    .child("Recent repositories"),
            );
        if recent.repositories.is_empty() {
            recents = recents.child(
                div()
                    .py(crate::spacing::edge(cx))
                    .text_color(muted)
                    .child("Your recently opened repositories will appear here."),
            );
        }
        for (index, directory) in recent
            .repositories
            .iter()
            .take(self.settings.welcome.recent_count)
            .enumerate()
        {
            let path = directory.clone();
            let full_path = crate::recents::display_path(directory);
            let name = directory
                .file_name()
                .unwrap_or(directory.as_os_str())
                .to_string_lossy()
                .into_owned();
            let parent = directory
                .parent()
                .map(crate::recents::display_path)
                .unwrap_or_default();
            recents = recents.child(
                div()
                    .id(("welcome-recent", index))
                    .debug_selector(move || format!("welcome-recent-{index}"))
                    .h_flex()
                    .items_center()
                    .min_w_0()
                    .gap(crate::spacing::edge(cx))
                    .cursor_pointer()
                    .px(crate::spacing::edge(cx))
                    .py(crate::spacing::interactive(cx))
                    .rounded_md()
                    .hover(|style| style.bg(theme.secondary_hover))
                    .child(
                        gpui::component::Icon::new(IconName::FolderOpen)
                            .size(px(18.))
                            .text_color(muted)
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .flex_1()
                            .min_w_0()
                            .gap(crate::spacing::edge(cx))
                            .child(
                                div()
                                    .max_w(rems(16.))
                                    .min_w_0()
                                    .truncate()
                                    .font_medium()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(parent),
                            ),
                    )
                    .tooltip(move |window, cx| {
                        gpui::component::tooltip::Tooltip::new(full_path.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |viewer, event: &ClickEvent, _, cx| {
                        let disposition = if event.modifiers().secondary(cx) {
                            crate::windows::OpenDisposition::NewWindow
                        } else {
                            crate::windows::OpenDisposition::Configured
                        };
                        viewer.open_repository_path(path.clone(), disposition, cx)
                    })),
            );
        }
        if !recent.repositories.is_empty() {
            recents = recents.child(
                div()
                    .mt(crate::spacing::interactive(cx))
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::edge(cx))
                    .child(
                        Button::new("welcome-more")
                            .ghost()
                            .label(
                                if recent.repositories.len() > self.settings.welcome.recent_count {
                                    "More repositories…"
                                } else {
                                    "Search recent repositories…"
                                },
                            )
                            .on_click(cx.listener(|viewer, _, window, cx| {
                                viewer.execute_command(CommandId::OpenRecent, window, cx)
                            })),
                    )
                    .child(super::palette::ShortcutHint(CommandId::OpenRecent)),
            );
        }
        let columns = div()
            .id("welcome-columns")
            .flex()
            .gap(crate::spacing::space(6., cx))
            .when(wide, |el| el.flex_row().gap(crate::spacing::space(6., cx)))
            .when(!wide, |el| el.flex_col())
            .child(start)
            .child(recents);
        let mut content = div()
            .id("welcome-content")
            .debug_selector(|| "welcome-content".into())
            .w_full()
            .max_w(rems(68.))
            .flex_shrink_0()
            .v_flex()
            .gap(crate::spacing::space(4., cx))
            .child(header)
            .child(columns);
        if let Some(error) = self
            .welcome
            .as_ref()
            .and_then(|welcome| welcome.error.as_ref())
        {
            content = content.child(
                div()
                    .v_flex()
                    .gap(crate::spacing::edge(cx))
                    .child(div().font_medium().child("Could not open that location"))
                    .child(feedback::error_message("welcome-open-error", error, cx)),
            );
        }
        for (id, error) in [
            ("welcome-settings-error", self.config.error.as_ref()),
            ("welcome-recent-error", recent.error.as_ref()),
        ] {
            if let Some(error) = error {
                content = content.child(feedback::error_message(id, error, cx));
            }
        }
        let focus = self.focus.clone();
        div()
            .id("welcome-workspace")
            .debug_selector(|| "welcome-page".into())
            .size_full()
            .v_flex()
            .overflow_y_scroll()
            .items_center()
            .p(crate::spacing::space(4., cx))
            .when(wide, |el| el.p(crate::spacing::space(6., cx)))
            .child(content)
            .context_menu(move |menu, _, _| {
                context_menu::command_entries(
                    menu.action_context(focus.clone()),
                    &[
                        (CommandId::OpenRepository, false),
                        (CommandId::OpenRecent, false),
                        (CommandId::ComparePaths, false),
                        (CommandId::ShowSetup, false),
                        (CommandId::InstallCli, false),
                        (CommandId::OpenSettings, false),
                        (CommandId::OpenKeybindings, false),
                    ],
                )
            })
            .into_any_element()
    }
}
