//! Shared menu composition. Keep commands on the same action path as shortcuts.
use super::*;
use gpui::component::menu::PopupMenu;
use jaywalk::commands::CommandId;

pub(super) fn command_entries(mut menu: PopupMenu, entries: &[(CommandId, bool)]) -> PopupMenu {
    for (command, checked) in entries {
        menu = menu.menu_with_check(
            command.label(),
            *checked,
            Box::new(commands::ExecuteCommand { command: *command }),
        );
    }
    menu
}
