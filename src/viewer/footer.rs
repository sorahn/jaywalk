//! Footer controls follow panel placement while the diff group follows the workspace edge.
use super::*;
use gpui::component::Sizable;
use gpui::component::menu::DropdownMenu as _;
use gpui::prelude::FluentBuilder;
use jaywalk::panel_layout::Side;

const INSET: f32 = 8.;
const GAP: f32 = 4.;

/// Allocate from the outside edge toward the viewer. Closed toggles share the
/// next visible drawer's footer, rather than shifting the viewer by their width.
/// Trailing closed toggles occupy a compact strip at the viewer's edge.
fn drawer_slots(widths: &[Option<f32>], compact: f32) -> Vec<f32> {
    let mut closed = 0.;
    widths
        .iter()
        .map(|width| match width {
            Some(width) => {
                let slot = (width - closed).max(compact);
                closed = 0.;
                slot
            }
            None => {
                closed += compact;
                compact
            }
        })
        .collect()
}

impl Viewer {
    pub(super) fn render_footer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h_flex()
            .items_center()
            .h(px(self.settings.footer_height()))
            .flex_shrink_0()
            .border_t_1()
            .border_color(gpui::component::Theme::global(cx).border)
            .bg(gpui::component::Theme::global(cx).muted)
            .text_size(px(self.settings.ui.footer_font_size))
            .whitespace_nowrap()
            .child(self.render_drawer_footer(Side::Left, cx))
            .child(
                div()
                    .debug_selector(|| "viewer-footer".into())
                    .h_flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .px(px(INSET))
                    .gap(px(GAP))
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
                    .child(self.render_diff_controls(window, cx))
                    .children(
                        self.bottom_timeline_state()
                            .is_some()
                            .then(|| self.render_timeline_toggle(cx)),
                    ),
            )
            .child(self.render_drawer_footer(Side::Right, cx))
            .into_any_element()
    }

    fn drawer_footer_slots(
        &self,
        side: Side,
        cx: &App,
    ) -> Vec<(jaywalk::panel_layout::PanelId, f32)> {
        let mut panels = self.placement().sidebars(side);
        // Right-side allocation starts at the right window edge; rendering is
        // still in ordinary left-to-right order.
        if side == Side::Right {
            panels.reverse();
        }
        let widths: Vec<_> = panels
            .iter()
            .map(|panel| self.measured_sidebar_width(*panel, cx))
            .collect();
        let widths = drawer_slots(&widths, self.settings.ui.footer_control_height + GAP);
        let mut slots: Vec<_> = panels.into_iter().zip(widths).collect();
        if side == Side::Right {
            slots.reverse();
        }
        slots
    }

    fn render_drawer_footer(&self, side: Side, cx: &mut Context<Self>) -> AnyElement {
        let slots = self.drawer_footer_slots(side, cx);
        div()
            .h_flex()
            .items_center()
            .flex_shrink_0()
            .children(slots.into_iter().map(|(panel, width)| {
                let visible = self.measured_sidebar_width(panel, cx).is_some();
                div()
                    .h_flex()
                    .items_center()
                    .gap(px(GAP))
                    .w(px(width))
                    .min_w_0()
                    .flex_shrink_0()
                    .when(side == Side::Left, |slot| slot.pl(px(INSET)))
                    .when(side == Side::Right, |slot| slot.pr(px(INSET)))
                    .when(visible && side == Side::Right, |slot| slot.pl(px(INSET)))
                    .when(visible && side == Side::Left, |slot| slot.pr(px(INSET)))
                    .when(side == Side::Left, |slot| {
                        slot.child(self.render_panel_toggle(panel, cx))
                    })
                    .when(visible, |slot| {
                        let (status, kind) = match panel {
                            jaywalk::panel_layout::PanelId::History => (
                                self.history_footer_status(),
                                evolution::EvolutionKind::Change,
                            ),
                            jaywalk::panel_layout::PanelId::Files => {
                                (self.files_footer_status(), evolution::EvolutionKind::File)
                            }
                            _ => return slot,
                        };
                        slot.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .debug_selector(move || format!("drawer-status-{panel:?}"))
                                .text_color(gpui::component::Theme::global(cx).muted_foreground)
                                .child(status),
                        )
                        .children(
                            self.supports_evolution()
                                .then(|| self.render_evolution_toggle(kind, cx)),
                        )
                    })
                    .when(side == Side::Right, |slot| {
                        slot.child(self.render_panel_toggle(panel, cx))
                    })
            }))
            .into_any_element()
    }

    fn render_diff_controls(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let drawer_width: f32 = [Side::Left, Side::Right]
            .into_iter()
            .flat_map(|side| self.drawer_footer_slots(side, cx))
            .map(|(_, width)| width)
            .sum();
        let available = f32::from(window.bounds().size.width) - drawer_width - 2. * INSET;
        let compact = available
            < self.settings.ui.footer_font_size * 30.
                + self.settings.ui.footer_control_height * 5.
                + 8.;
        let focus = self.focus.clone();
        let view = self.view.effective();
        let wrap = self.wrapping.enabled(view);
        let line_numbers = self.line_numbers;
        let collapsed = self.standard_context_collapsed();
        let total = self.code().map_or(0, |code| {
            if view != ViewMode::Fluid
                && let Ok(Some(state)) = &code.standard
            {
                state.read(cx).change_navigation_position().1
            } else {
                code.model
                    .blocks
                    .iter()
                    .filter(|block| block.changed)
                    .count()
            }
        });
        div()
            .debug_selector(|| "diff-footer-controls".into())
            .h_flex()
            .items_center()
            .flex_shrink_0()
            .when(self.selected_file.is_none(), |controls| controls.hidden())
            .gap_2()
            .when(compact, |controls| {
                controls.child(
                    Button::new("diff-controls-menu")
                        .ghost()
                        .compact()
                        .debug_selector(|| "diff-controls-menu".into())
                        .with_size(px(self.settings.ui.footer_control_height))
                        .icon(IconName::Ellipsis)
                        .tooltip("Diff display options")
                        .dropdown_menu(move |menu, _, _| {
                            context_menu::diff_commands(
                                menu.action_context(focus.clone()),
                                view,
                                wrap,
                                line_numbers,
                                collapsed,
                            )
                        }),
                )
            })
            .when(!compact, |controls| {
                controls
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .mr(px(8.))
                            .child(
                                Button::new("word-wrap")
                                    .ghost()
                                    .with_size(px(self.settings.ui.footer_control_height))
                                    .compact()
                                    .icon(crate::appearance::active_icon(
                                        IconName::TextWrap,
                                        wrap,
                                        cx,
                                    ))
                                    .tooltip("Word wrap")
                                    .toggled(wrap)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_wrap(window, cx)
                                    })),
                            )
                            .when(
                                view != ViewMode::Fluid
                                    && self
                                        .code()
                                        .is_some_and(|code| matches!(code.standard, Ok(Some(_)))),
                                |options| {
                                    options.child(
                                        Button::new("file-context")
                                            .debug_selector(|| "file-context".into())
                                            .ghost()
                                            .with_size(px(self.settings.ui.footer_control_height))
                                            .compact()
                                            .icon(crate::appearance::active_icon(
                                                IconName::FoldVertical,
                                                collapsed,
                                                cx,
                                            ))
                                            .tooltip(if collapsed {
                                                "Show full file"
                                            } else {
                                                "Fold unchanged lines"
                                            })
                                            .toggled(collapsed)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_unchanged_lines(cx)
                                            })),
                                    )
                                },
                            ),
                    )
                    .child(self.render_view_toggle(cx))
            })
            .child(self.render_change_counter(total, compact, window, cx))
            .child(
                Button::new("previous-change")
                    .debug_selector(|| "previous-change".into())
                    .ghost()
                    .with_size(px(self.settings.ui.footer_control_height))
                    .label("↑")
                    .disabled(
                        self.standard_context_collapsed()
                            || self.code().is_none()
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
                    .debug_selector(|| "next-change".into())
                    .ghost()
                    .with_size(px(self.settings.ui.footer_control_height))
                    .label("↓")
                    .disabled(
                        self.standard_context_collapsed()
                            || self.code().is_none()
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
    use super::drawer_slots;
    #[test]
    fn drawer_footers_follow_visible_widths_and_pack_closed_controls() {
        let compact = 26.;
        assert_eq!(
            drawer_slots(&[Some(280.), Some(260.)], compact),
            [280., 260.]
        );
        assert_eq!(drawer_slots(&[None, Some(260.)], compact), [26., 234.]);
        assert_eq!(drawer_slots(&[Some(280.), None], compact), [280., 26.]);
        assert_eq!(drawer_slots(&[None, None], compact), [26., 26.]);
        // Width changes are measured, not tied to sidebar defaults.
        assert_eq!(
            drawer_slots(&[Some(417.), Some(193.)], compact),
            [417., 193.]
        );
        assert_eq!(drawer_slots(&[Some(12.), None], compact), [26., 26.]);
        assert!(drawer_slots(&[], compact).is_empty());
    }
}
