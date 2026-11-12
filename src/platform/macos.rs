//! AppKit menu bar through GPUI's native menu bridge.
use super::*;
use crate::viewer::commands::ExecuteCommand;
use jaywalk::commands::CommandId;

fn item(command: CommandId) -> MenuItem {
    MenuItem::action(command.label(), ExecuteCommand { command })
}

pub(super) fn update_menus(cx: &mut App) {
    use CommandId::*;
    cx.set_menus([
        Menu::new("Jaywalk").items([
            item(About),
            MenuItem::separator(),
            item(OpenSettings),
            item(OpenKeybindings),
            item(SelectKeybindings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            item(Hide),
            item(HideOthers),
            item(ShowAll),
            MenuItem::separator(),
            item(Quit),
        ]),
        Menu::new("File").items([
            item(OpenRepository),
            item(OpenRecent),
            item(ComparePaths),
            item(ClearRecent),
            item(Refresh),
            MenuItem::separator(),
            item(CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy", gpui::base::input::Copy),
            MenuItem::action("Select All", gpui::base::input::SelectAll),
        ]),
        Menu::new("View").items([
            item(ShowWelcome),
            item(ShowCommands),
            item(FindFile),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Diff View").items([
                item(ViewInline),
                item(ViewSideBySide),
                item(ViewFluid),
            ])),
            item(ToggleWrap),
            item(ToggleLineNumbers),
            item(ToggleUnchangedLines),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("References").items([
                item(ShowReferences),
                item(ToggleReferenceLabels),
                item(ShowAllRemotes),
                item(HideAllRemotes),
                item(RevealSelectedRevision),
            ])),
            item(ToggleHistory),
            item(ToggleFiles),
            item(ToggleChangeEvolution),
            item(ToggleFileEvolution),
            MenuItem::submenu(Menu::new("Panel Placement").items([
                item(HistoryLeft),
                item(HistoryRight),
                item(FilesLeft),
                item(FilesRight),
                item(SwapSidebars),
                MenuItem::separator(),
                item(ChangeEvolutionSidebar),
                item(ChangeEvolutionBottom),
                item(FileEvolutionSidebar),
                item(FileEvolutionBottom),
            ])),
            MenuItem::separator(),
            item(ZoomIn),
            item(ZoomOut),
            item(ResetZoom),
            item(ResetLayout),
            item(ResetInterface),
        ]),
        Menu::new("Go").items([
            item(GoBack),
            item(GoForward),
            MenuItem::separator(),
            item(AllChanges),
            item(CurrentWork),
            item(OpenedComparison),
            item(Overview),
            MenuItem::separator(),
            item(NextChange),
            item(PreviousChange),
            item(NextFile),
            item(PreviousFile),
            item(NextRevision),
            item(PreviousRevision),
        ]),
        Menu::new("Window").items([item(Minimize), item(Fullscreen)]),
        Menu::new("Help").items([item(ShowCommands), item(OpenSettings)]),
    ]);
}
