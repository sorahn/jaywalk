//! Shared literal, selectable diagnostics with an explicit complete-message copy action.
use gpui::component::{
    StyledExt,
    button::{Button, ButtonVariants},
};
use gpui::*;

/// Keep diagnostics literal and selectable; Copy retains the complete message,
/// including line breaks, even when only part of it fits in a narrow panel.
pub(crate) fn error_message(id: impl Into<SharedString>, message: &str, cx: &App) -> AnyElement {
    let id = id.into();
    let copied = message.to_owned();
    div()
        .id(id.clone())
        .v_flex()
        .gap(crate::spacing::edge(cx))
        .min_w_0()
        .child(
            div().child(
                Button::new(SharedString::from(format!("{id}-copy")))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("{id}-copy")
                    })
                    .ghost()
                    .label("Copy error")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                    }),
            ),
        )
        .child(gpui::base::SelectableText::new(
            SharedString::from(format!("{id}-text")),
            message.to_owned(),
        ))
        .into_any_element()
}
