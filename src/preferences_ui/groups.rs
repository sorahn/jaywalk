//! Collapsed advanced sections are presentation state, independent of drafts.
use super::*;
use jaywalk::preferences::groups::Group;
use std::collections::BTreeSet;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    Setting(usize),
    Group(Group),
    Section(Category, &'static str),
}
impl SettingsPage {
    pub(super) fn load_groups(&mut self, cx: &mut Context<Self>) {
        let path = self.source.path().to_owned();
        if let Some(expanded) = crate::interface_state::settings_groups(&path, cx) {
            self.expanded = expanded;
            return;
        }
        cx.spawn(async move |page, cx| {
            let read_path = path.clone();
            let result = cx
                .background_spawn(async move { jaywalk::preferences::groups::read(&read_path) })
                .await;
            let _ = page.update(cx, |page, cx| {
                match result {
                    Ok(expanded) => {
                        page.expanded =
                            crate::interface_state::loaded_settings_groups(path, expanded, cx)
                    }
                    Err(error) => page.message = Some(error),
                }
                page.rebuild_rows(cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn rebuild_rows(&mut self, cx: &App) {
        let reveal = self.changed || !self.query.read(cx).value().is_empty();
        self.rebuild_group_rows(reveal);
    }
    pub(super) fn rebuild_group_rows(&mut self, reveal: bool) {
        let mut display = Vec::new();
        if reveal {
            display.extend(self.rows.iter().copied().map(Row::Setting));
        } else {
            for category in Category::ALL {
                let settings = self
                    .rows
                    .iter()
                    .copied()
                    .filter(|index| catalogue::SETTINGS[*index].category == category)
                    .collect::<Vec<_>>();
                let sections = settings
                    .iter()
                    .map(|index| {
                        let setting = &catalogue::SETTINGS[*index];
                        (setting.section_order, setting.section.as_str())
                    })
                    .collect::<BTreeSet<_>>();
                let show_heading = self.category.is_none() || sections.len() > 1;
                for (_, section) in sections {
                    if show_heading {
                        display.push(Row::Section(category, section));
                    }
                    let members = settings
                        .iter()
                        .copied()
                        .filter(|index| catalogue::SETTINGS[*index].section == section)
                        .collect::<Vec<_>>();
                    display.extend(
                        members
                            .iter()
                            .copied()
                            .filter(|index| catalogue::SETTINGS[*index].group.is_none())
                            .map(Row::Setting),
                    );
                    let groups = members
                        .iter()
                        .filter_map(|index| catalogue::SETTINGS[*index].group)
                        .collect::<BTreeSet<_>>();
                    for group in groups {
                        display.push(Row::Group(group));
                        if self.expanded.contains(&group) {
                            display.extend(
                                members
                                    .iter()
                                    .copied()
                                    .filter(|index| {
                                        catalogue::SETTINGS[*index].group == Some(group)
                                    })
                                    .map(Row::Setting),
                            );
                        }
                    }
                }
            }
        }
        if self.display_rows != display {
            let old = self.scroll.logical_scroll_top();
            let anchor = self.display_rows.get(old.item_ix).copied();
            self.scroll.reset(0);
            self.list_hints_dirty = true;
            self.scroll.splice_focusable(
                0..0,
                display.iter().map(|row| match row {
                    Row::Setting(index) => Some(self.row_focus[*index].clone()),
                    Row::Group(_) | Row::Section(..) => None,
                }),
            );
            if let Some(index) =
                anchor.and_then(|row| display.iter().position(|candidate| *candidate == row))
            {
                self.scroll.scroll_to(ListOffset {
                    item_ix: index,
                    offset_in_item: old.offset_in_item,
                });
            }
            self.display_rows = display;
        } else {
            self.scroll.remeasure_items(0..display.len());
        }
    }
    pub(super) fn render_group_row(&mut self, group: Group, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.expanded.contains(&group);
        let changed = self.snapshot.as_ref().map_or(0, |snapshot| {
            self.rows
                .iter()
                .filter(|index| {
                    catalogue::SETTINGS[**index].group == Some(group)
                        && snapshot.modified(&catalogue::SETTINGS[**index])
                })
                .count()
        });
        let label = if self.category.is_none() {
            format!("{} · {}", group.category().label(), group.label())
        } else {
            group.label().to_owned()
        };
        div()
            .w_full()
            .max_w(px(1120.))
            .mx_auto()
            .py(crate::spacing::interactive(cx))
            .child(
                Button::new(SharedString::from(format!(
                    "settings-group-{}",
                    group.key()
                )))
                .debug_selector(move || format!("settings-group-{}", group.key()))
                .ghost()
                .w_full()
                .justify_start()
                .accessibility_label(format!(
                    "{label}, {}",
                    if expanded { "expanded" } else { "collapsed" }
                ))
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .gap(crate::spacing::edge(cx))
                        .child(
                            gpui::component::Icon::new(if expanded {
                                gpui::component::IconName::ChevronDown
                            } else {
                                gpui::component::IconName::ChevronRight
                            })
                            .small(),
                        )
                        .child(div().min_w_0().truncate().child(label))
                        .when(changed > 0, |row| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .debug_selector(move || {
                                        format!("settings-group-{}-changed", group.key())
                                    })
                                    .text_xs()
                                    .text_color(gpui::component::Theme::global(cx).muted_foreground)
                                    .child(format!("{changed} changed")),
                            )
                        }),
                )
                .on_click(cx.listener(move |page, _, window, cx| {
                    window.focus(&page.surface_focus, cx);
                    if !page.expanded.remove(&group) {
                        page.expanded.insert(group);
                    }
                    if let Err(error) = crate::interface_state::save_settings_groups(
                        page.source.path().to_owned(),
                        page.expanded.clone(),
                        cx,
                    ) {
                        page.message = Some(error);
                    }
                    page.rebuild_rows(cx);
                    cx.notify();
                })),
            )
            .into_any_element()
    }
    pub(super) fn render_settings_entry(
        &mut self,
        index: usize,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.display_rows[index] {
            Row::Setting(index) => div()
                .w_full()
                .track_focus(&self.row_focus[index])
                .child(self.render_row(index, narrow, window, cx))
                .into_any_element(),
            Row::Group(group) => self.render_group_row(group, cx),
            Row::Section(category, section) => div()
                .w_full()
                .debug_selector(move || {
                    format!(
                        "settings-section-{}",
                        section.to_lowercase().replace(' ', "-")
                    )
                })
                .max_w(px(1120.))
                .mx_auto()
                .px(crate::spacing::space(2., cx))
                .py(crate::spacing::space(2., cx))
                .text_sm()
                .font_medium()
                .text_color(gpui::component::Theme::global(cx).muted_foreground)
                .child(if self.category.is_none() {
                    format!("{} · {section}", category.label())
                } else {
                    section.to_owned()
                })
                .into_any_element(),
        }
    }
}
