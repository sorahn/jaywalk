//! Application spacing roles consume one logical-pixel scale, never font rems.
use gpui::component::StyledExt as _;
use gpui::component::{HostSpacing, Theme};
use gpui::{App, Div, Pixels, Styled, div, px};

fn policy(cx: &App) -> HostSpacing {
    Theme::global(cx).host_spacing.unwrap_or(HostSpacing {
        unit: px(4.),
        edge: px(8.),
        interactive: px(4.),
    })
}
pub(crate) fn space(units: f32, cx: &App) -> Pixels {
    policy(cx).space(units)
}
pub(crate) fn edge(cx: &App) -> Pixels {
    policy(cx).edge
}
pub(crate) fn interactive(cx: &App) -> Pixels {
    policy(cx).interactive
}

pub(crate) fn toggle_group(surface: jaywalk::theme::Surface, cx: &App) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(interactive(cx))
        // The buttons own the interactive inset; adding it here doubles it.
        .p(px(0.))
        .rounded(Theme::global(cx).radius)
        .bg(crate::appearance::surface(
            surface,
            jaywalk::theme::Part::GroupBackground,
            cx,
        ))
}
