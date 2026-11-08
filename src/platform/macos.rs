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
            item(Refresh),
            MenuItem::separator(),
            item(CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy", gpui::base::input::Copy),
            MenuItem::action("Select All", gpui::base::input::SelectAll),
        ]),
        Menu::new("View").items([
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
            MenuItem::separator(),
            item(ToggleHistory),
            item(ToggleFiles),
            MenuItem::separator(),
            item(ZoomIn),
            item(ZoomOut),
            item(ResetZoom),
            item(ResetLayout),
        ]),
        Menu::new("Go").items([
            item(WholeBranch),
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
