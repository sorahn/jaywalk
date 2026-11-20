//! Shared picker presentation using toolkit elements; each source owns its requests.
use gpui::component::{StyledExt, Theme};
use gpui::*;

pub(crate) fn frame(cx: &App) -> Div {
    let theme = Theme::global(cx);
    div()
        .v_flex()
        .min_w_0()
        .bg(theme.background)
        .border_1()
        .border_color(crate::appearance::floating_outline(cx))
        .rounded_lg()
}
pub(crate) fn context(lines: impl IntoIterator<Item = String>, cx: &App) -> Div {
    div()
        .v_flex()
        .min_w_0()
        .flex_shrink_0()
        .px_3()
        .py_2()
        .text_sm()
        .text_color(Theme::global(cx).muted_foreground)
        .children(lines.into_iter().map(|line| div().truncate().child(line)))
}
pub(crate) fn matched_label(text: SharedString, query: &str, cx: &App) -> StyledText {
    let style = HighlightStyle {
        color: Some(Theme::global(cx).list_highlight_foreground),
        ..Default::default()
    };
    let highlights = jaywalk::quick_picker::match_ranges(&text, query)
        .into_iter()
        .map(|range| (range, style));
    StyledText::new(text).with_highlights(highlights)
}
