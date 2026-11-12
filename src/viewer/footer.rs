//! Footer controls follow panel placement while the diff group follows the workspace edge.
use super::*;
use gpui::component::Sizable;
use gpui::prelude::FluentBuilder;
use jaywalk::panel_layout::Side;

const INSET: f32 = 8.;
const GAP: f32 = 4.;

fn right_spacer(sidebar_width: f32, controls_width: f32) -> f32 {
    (sidebar_width - controls_width - INSET - GAP).max(0.)
}

impl Viewer {
    pub(super) fn render_footer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let control_height = self.settings.ui.footer_control_height;
        // Hidden panels retain an edge button so they can be reopened.
        let right_count = self.placement().sidebars(Side::Right).len() + 1;
        let right_controls_width =
            right_count as f32 * control_height + (right_count - 1) as f32 * GAP;
        let spacer = right_spacer(self.right_sidebar_width(cx), right_controls_width);
        div()
            .h_flex()
            .items_center()
            .px(px(INSET))
            .gap(px(GAP))
            .h(px(self.settings.footer_height()))
            .flex_shrink_0()
            .border_t_1()
            .border_color(gpui::component::Theme::global(cx).border)
            .bg(gpui::component::Theme::global(cx).muted)
            .text_size(px(self.settings.ui.footer_font_size))
            .child(self.render_panel_toggles(Side::Left, cx))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_3()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_3()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().min_w_0().truncate().child(format!(
                                        "{}  |  {}",
                                        if self.arguments.diff.is_some() {
                                            "Read-only comparison"
                                        } else if !self.supports_evolution() {
                                            "Read-only Git snapshot"
                                        } else {
                                            "Read-only jj snapshot"
                                        },
                                        self.scope_label()
                                    )))
                                    .children(self.status_notice(cx)),
                            )
                            .child(self.render_diff_controls(window, cx)),
                    )
                    .child(div().w(px(spacer)).flex_shrink_0()),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .flex_shrink_0()
                    .child(self.render_timeline_toggle(cx))
                    .children(
                        (right_count > 1).then(|| self.render_panel_toggles(Side::Right, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_diff_controls(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let total = self.code().map_or(0, |CodeComparison { model, .. }| {
            model.blocks.iter().filter(|block| block.changed).count()
        });
        div()
            .h_flex()
            .items_center()
            .flex_shrink_0()
            .when(self.selected_file.is_none(), |controls| controls.hidden())
            .gap_2()
            .child(
                Button::new("word-wrap")
                    .ghost()
                    .with_size(px(self.settings.ui.footer_control_height))
                    .compact()
                    .icon(crate::appearance::active_icon(
                        IconName::TextWrap,
                        self.wrapping.enabled(self.view.effective()),
                        cx,
                    ))
                    .tooltip("Word wrap")
                    .toggled(self.wrapping.enabled(self.view.effective()))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_wrap(window, cx))),
            )
            .child(self.render_view_toggle(cx))
            .child(self.render_change_counter(total, window, cx))
            .child(
                Button::new("previous-change")
                    .ghost()
                    .with_size(px(self.settings.ui.footer_control_height))
                    .label("↑")
                    .disabled(
                        self.code().is_none()
                            || total == 0
                            || (self.view.effective() != ViewMode::Fluid
                                && self
                                    .code()
                                    .is_none_or(|code| !matches!(code.standard, Ok(Some(_))))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.navigate_change(-1, cx))),
            )
            .child(
                Button::new("next-change")
                    .ghost()
                    .with_size(px(self.settings.ui.footer_control_height))
                    .label("↓")
                    .disabled(
                        self.code().is_none()
                            || total == 0
                            || (self.view.effective() != ViewMode::Fluid
                                && self
                                    .code()
                                    .is_none_or(|code| !matches!(code.standard, Ok(Some(_))))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.navigate_change(1, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{GAP, INSET, right_spacer};
    #[test]
    fn controls_follow_the_diff_edge_until_they_meet_the_outer_buttons() {
        for sidebar in [0., 20., 260., 540.] {
            for buttons in [22., 48., 74.] {
                let gap = right_spacer(sidebar, buttons);
                assert_eq!(
                    INSET + buttons + GAP + gap,
                    sidebar.max(INSET + buttons + GAP)
                );
            }
        }
    }
}
