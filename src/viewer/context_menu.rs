//! Shared menu composition. Keep commands on the same action path as shortcuts.
use super::*;
use gpui::component::menu::{PopupMenu, PopupMenuItem};
use jaywalk::commands::CommandId;
use jaywalk::repository::ComparisonIdentity;
use std::path::{Path, PathBuf};

pub(super) fn command_entries(mut menu: PopupMenu, entries: &[(CommandId, bool)]) -> PopupMenu {
    for (command, checked) in entries {
        let label = match command {
            CommandId::ToggleHistory => "Hide history panel",
            CommandId::ToggleFiles => "Hide files panel",
            CommandId::ToggleChangeEvolution => "Hide change evolution panel",
            CommandId::ToggleFileEvolution => "Hide file evolution panel",
            _ => command.label(),
        };
        menu = menu.menu_with_check(
            label,
            *checked,
            Box::new(commands::ExecuteCommand { command: *command }),
        );
    }
    menu
}

/// A menu keeps its original subject even if another row becomes selected.
#[derive(Clone)]
pub(super) struct FileTarget {
    pub(super) source: ComparisonIdentity,
    pub(super) path: String,
    pub(super) directory: bool,
    working_path: Option<PathBuf>,
}

pub(super) fn file_menu(
    mut menu: PopupMenu,
    target: FileTarget,
    viewer: WeakEntity<Viewer>,
) -> PopupMenu {
    for command in [
        CommandId::OpenFile,
        CommandId::RevealInFileManager,
        CommandId::CopyFileName,
        CommandId::CopyRelativePath,
        CommandId::CopyPath,
    ] {
        if target.directory && command == CommandId::OpenFile {
            continue;
        }
        if command == CommandId::CopyFileName {
            menu = menu.separator();
        }
        let label = match command {
            CommandId::OpenFile => "Open file",
            CommandId::CopyFileName => "Copy name",
            CommandId::CopyPath => "Copy path",
            CommandId::RevealInFileManager if cfg!(target_os = "macos") => "Reveal in Finder",
            CommandId::RevealInFileManager => "Reveal in file manager",
            _ => command.label(),
        };
        let disabled = matches!(
            command,
            CommandId::CopyPath | CommandId::RevealInFileManager
        ) && target.working_path.is_none();
        let subject = target.clone();
        let owner = viewer.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .action(Box::new(commands::ExecuteCommand { command }))
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    let _ = owner.update(cx, |viewer, cx| {
                        viewer.execute_file_action(command, &subject, window, cx)
                    });
                }),
        );
    }
    if target.directory {
        menu = menu.separator();
        for expanded in [true, false] {
            let subject = target.clone();
            let owner = viewer.clone();
            menu = menu.item(
                PopupMenuItem::new(if expanded {
                    "Expand all"
                } else {
                    "Collapse all"
                })
                .on_click(move |_, _, cx| {
                    let _ = owner.update(cx, |viewer, cx| {
                        if viewer
                            .comparison()
                            .is_some_and(|comparison| comparison.identity() == subject.source)
                        {
                            viewer.set_directory_expansion(Some(&subject.path), expanded, cx);
                        }
                    });
                }),
            );
        }
    }
    menu
}

impl FileTarget {
    pub(super) fn new(
        source: ComparisonIdentity,
        root: &Path,
        path: String,
        directory: bool,
        repository: bool,
    ) -> Self {
        Self {
            source,
            working_path: repository.then(|| root.join(&path)),
            path,
            directory,
        }
    }
}

impl Viewer {
    pub(super) fn file_target(&self, path: String, directory: bool) -> Option<FileTarget> {
        let comparison = self.comparison()?;
        Some(FileTarget {
            source: comparison.identity(),
            working_path: self
                .arguments
                .diff
                .is_none()
                .then(|| comparison.root.join(&path)),
            path,
            directory,
        })
    }

    pub(super) fn current_file_target(&self) -> Option<FileTarget> {
        if let Some(selected) = &self.selected_file {
            return Some(FileTarget::new(
                selected.source.identity(),
                &selected.source.root,
                selected.file.path().to_owned(),
                false,
                self.arguments.diff.is_none(),
            ));
        }
        self.file_target(self.browser.reading.clone()?, false)
    }

    pub(super) fn execute_file_action(
        &mut self,
        command: CommandId,
        target: &FileTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match command {
            CommandId::CopyRelativePath => Some(target.path.clone()),
            CommandId::CopyFileName => Some(
                Path::new(&target.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ),
            CommandId::CopyPath => target
                .working_path
                .as_ref()
                .map(|path| path.display().to_string()),
            _ => None,
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        match command {
            CommandId::OpenFile
                if !target.directory
                    && self
                        .comparison()
                        .is_some_and(|comparison| comparison.identity() == target.source) =>
            {
                self.select_path(&target.path, window, cx)
            }
            CommandId::RevealInFileManager => {
                let Some(path) = target.working_path.clone() else {
                    return;
                };
                cx.spawn_in(window, async move |this, cx| {
                    let result = cx
                        .background_spawn(async move { existing_location(&path) })
                        .await;
                    let _ = this.update(cx, |viewer, cx| match result {
                        Ok(path) => cx.reveal_path(&path),
                        Err(error) => {
                            viewer.live.message = Some(error);
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            _ => {}
        }
    }
}

// Historical/deleted files may have no working-tree counterpart. Reveal their
// nearest existing ancestor, rather than materializing historical contents.
fn existing_location(path: &Path) -> Result<PathBuf, String> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => return Ok(ancestor.to_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Could not reveal {}: {error}", path.display())),
        }
    }
    Err(format!("No existing location for {}", path.display()))
}

pub(super) fn diff_commands(
    menu: PopupMenu,
    view: ViewMode,
    wrap: bool,
    line_numbers: bool,
) -> PopupMenu {
    command_entries(
        menu,
        &[
            (CommandId::ViewInline, view == ViewMode::Inline),
            (CommandId::ViewSideBySide, view == ViewMode::SideBySide),
            (CommandId::ViewFluid, view == ViewMode::Fluid),
            (CommandId::ToggleWrap, wrap),
            (CommandId::ToggleLineNumbers, line_numbers),
            (CommandId::PreviousChange, false),
            (CommandId::NextChange, false),
            (CommandId::Overview, false),
            (CommandId::AllChanges, false),
            (CommandId::CurrentWork, false),
            (CommandId::ChooseReviewBase, false),
        ],
    )
}

pub(super) fn overview_commands(menu: PopupMenu) -> PopupMenu {
    command_entries(
        menu,
        &[
            (CommandId::CollapseOverview, false),
            (CommandId::ExpandOverview, false),
            (CommandId::OverviewContext, false),
            (CommandId::OverviewFull, false),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::existing_location;
    #[test]
    fn missing_historical_files_reveal_the_existing_parent_without_creating_files() {
        let parent = std::env::temp_dir();
        let absent = parent.join(format!("jaywalk-missing-location-{}", std::process::id()));
        assert!(!absent.exists());
        assert_eq!(
            existing_location(&absent.join("nested/renamed file.rs")).unwrap(),
            parent
        );
        assert!(!absent.exists());
    }
}
