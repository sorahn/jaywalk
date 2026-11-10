//! Repository-free workspace. Panel preferences remain untouched underneath it.
use super::*;
use gpui::component::menu::ContextMenuExt as _;

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
        self.welcome.is_some()
    }
    pub(crate) fn report_open_error(&mut self, error: String, cx: &mut Context<Self>) {
        if let Some(welcome) = &mut self.welcome {
            welcome.error = Some(error);
        } else {
            self.live.message = Some(error);
        }
        cx.notify();
    }
    pub(crate) fn repository_directory(&self) -> Option<std::path::PathBuf> {
        (self.arguments.diff.is_none())
            .then(|| {
                self.repository
                    .ready()
                    .map(|session| session.home.root.clone())
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
            crate::windows::OpenDisposition::Reuse => crate::open_arguments(
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
    pub(super) fn render_welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        let recent = cx.global::<crate::recents::Recents>();
        let mut start = div()
            .v_flex()
            .gap_3()
            .child(div().text_2xl().font_medium().child("Jaywalk"))
            .child(
                div()
                    .text_color(muted)
                    .child("Browse Git and Jujutsu changes"),
            )
            .child(div().mt_4().text_lg().font_medium().child("Start"));
        for (id, command, icon) in [
            (
                "welcome-open",
                jaywalk::commands::CommandId::OpenRepository,
                IconName::FolderOpen,
            ),
            (
                "welcome-compare",
                jaywalk::commands::CommandId::ComparePaths,
                IconName::GitCompareArrows,
            ),
        ] {
            start = start.child(
                div().h_flex().child(
                    Button::new(id)
                        .ghost()
                        .icon(icon)
                        .label(command.label())
                        .tooltip_with_action(
                            command.label(),
                            &commands::ExecuteCommand { command },
                            None,
                        )
                        .on_click(cx.listener(move |viewer, _, window, cx| {
                            viewer.execute_command(command, window, cx)
                        })),
                ),
            );
        }
        start = start.child(
            div()
                .mt_4()
                .text_lg()
                .font_medium()
                .child("Recent repositories"),
        );
        if recent.repositories.is_empty() {
            start = start.child(
                div()
                    .text_color(muted)
                    .child("Open a repository to find it here next time."),
            );
        }
        for (index, directory) in recent
            .repositories
            .iter()
            .take(self.settings.welcome.recent_count)
            .enumerate()
        {
            let path = directory.clone();
            let name = directory
                .file_name()
                .unwrap_or(directory.as_os_str())
                .to_string_lossy()
                .into_owned();
            let parent = directory
                .parent()
                .map(crate::recents::display_path)
                .unwrap_or_default();
            start = start.child(
                div()
                    .id(("welcome-recent", index))
                    .h_flex()
                    .min_w_0()
                    .gap_3()
                    .cursor_pointer()
                    .p_1()
                    .rounded_md()
                    .hover(|style| style.bg(theme.secondary_hover))
                    .child(div().font_medium().child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(muted)
                            .child(parent),
                    )
                    .on_click(cx.listener(move |viewer, event: &ClickEvent, _, cx| {
                        let disposition = if event.modifiers().secondary() {
                            crate::windows::OpenDisposition::NewWindow
                        } else {
                            crate::windows::OpenDisposition::Reuse
                        };
                        viewer.open_repository_path(path.clone(), disposition, cx)
                    })),
            );
        }
        if !recent.repositories.is_empty() {
            start = start.child(
                div().h_flex().child(
                    Button::new("welcome-more")
                        .ghost()
                        .label(
                            if recent.repositories.len() > self.settings.welcome.recent_count {
                                "More…"
                            } else {
                                "Search recent repositories…"
                            },
                        )
                        .tooltip_with_action(
                            "Open recent repository",
                            &commands::ExecuteCommand {
                                command: jaywalk::commands::CommandId::OpenRecent,
                            },
                            None,
                        )
                        .on_click(cx.listener(|viewer, _, window, cx| {
                            viewer.execute_command(
                                jaywalk::commands::CommandId::OpenRecent,
                                window,
                                cx,
                            )
                        })),
                ),
            );
        }
        let error = self
            .welcome
            .as_ref()
            .and_then(|welcome| welcome.error.as_ref());
        if let Some(error) = error {
            start = start
                .child(
                    div()
                        .mt_4()
                        .font_medium()
                        .child("Could not open that location"),
                )
                .child(feedback::error_message("welcome-open-error", error));
        }
        if let Some(error) = &self.config.error {
            start = start.child(
                div()
                    .mt_4()
                    .child(feedback::error_message("welcome-settings-error", error)),
            );
        }
        if let Some(error) = &recent.error {
            start = start.child(
                div()
                    .mt_4()
                    .child(feedback::error_message("welcome-recent-error", error)),
            );
        }
        let focus = self.focus.clone();
        div()
            .id("welcome-workspace")
            .size_full()
            .v_flex()
            .overflow_y_scroll()
            .items_center()
            .p_8()
            .child(div().w_full().max_w(rems(48.)).py_8().child(start))
            .context_menu(move |menu, _, _| {
                context_menu::command_entries(
                    menu.action_context(focus.clone()),
                    &[
                        (jaywalk::commands::CommandId::OpenRepository, false),
                        (jaywalk::commands::CommandId::OpenRecent, false),
                        (jaywalk::commands::CommandId::ComparePaths, false),
                        (jaywalk::commands::CommandId::OpenSettings, false),
                        (jaywalk::commands::CommandId::OpenKeybindings, false),
                    ],
                )
            })
            .into_any_element()
    }
}
