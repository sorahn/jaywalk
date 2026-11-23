//! Shortcut presentation uses the active window keymap, including user remaps.
use crate::viewer::commands::ExecuteCommand;
use gpui::component::button::Button;
use gpui::prelude::FluentBuilder;
use gpui::*;
use jaywalk::commands::CommandId;

#[derive(IntoElement)]
pub(crate) struct ShortcutHint(pub(crate) CommandId);
impl RenderOnce for ShortcutHint {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        // The rendered dispatch stack can be empty while a page is built.
        // Fall back to its explicit app context for context-scoped commands.
        let binding = gpui::component::kbd::Kbd::binding_for_action(
            &ExecuteCommand { command: self.0 },
            None,
            window,
        )
        .or_else(|| {
            gpui::component::kbd::Kbd::binding_for_action(
                &ExecuteCommand { command: self.0 },
                Some("Jaywalk"),
                window,
            )
        });
        div()
            // An unbound command must not leave a flex gap after its label.
            .when(binding.is_none(), |hint| hint.hidden())
            .debug_selector(move || format!("shortcut-{}", self.0.key()))
            .children(binding)
    }
}

pub(crate) trait CommandButtonExt {
    /// Text actions show their current binding beside the label.
    fn command_hint(self, command: CommandId) -> Self;
    /// Icon actions retain their compact shape and show the binding on hover.
    fn command_tooltip(self, label: impl Into<SharedString>, command: CommandId) -> Self;
}
impl CommandButtonExt for Button {
    fn command_hint(self, command: CommandId) -> Self {
        self.child(ShortcutHint(command))
    }
    fn command_tooltip(self, label: impl Into<SharedString>, command: CommandId) -> Self {
        self.tooltip_with_action(label, &ExecuteCommand { command }, None)
    }
}
