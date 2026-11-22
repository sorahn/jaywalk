//! Retained native settings content. Controls edit the shared TOML source.
mod controls;
use crate::command_ui::CommandButtonExt;
use controls::FontPicker;
use gpui::component::{
    Disableable, Selectable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, NumberStep, StepAction},
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    scroll::Scrollbar,
    switch::Switch,
};
use gpui::prelude::FluentBuilder;
use gpui::*;
use jaywalk::commands::CommandId;
use jaywalk::{
    preferences::{
        self, Expect, Patch, Snapshot,
        catalogue::{self, Category, Kind},
    },
    settings_reload::SettingsSource,
};
use std::{collections::HashMap, sync::Arc};
actions!(preferences_ui, [Cancel]);
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Cancel, Some("JaywalkSettings"))]);
}
fn input_text(value: Option<&toml::Value>) -> String {
    value
        .map(|value| preferences::display(Some(value)))
        .unwrap_or_default()
}
pub(crate) enum Event {
    Leave,
    EditToml,
    Accounts,
    Keybindings,
}
impl EventEmitter<Event> for SettingsPage {}
struct Draft {
    input: Entity<InputState>,
    baseline: Option<toml::Value>,
    color_dark: bool,
    dirty: bool,
    error: Option<String>,
}
pub(crate) struct SettingsPage {
    source: SettingsSource,
    query: Entity<InputState>,
    surface_focus: FocusHandle,
    snapshot: Option<Arc<Snapshot>>,
    category: Option<Category>,
    changed: bool,
    query_active: bool,
    before_search: Option<Category>,
    defaults: bool,
    rows: Vec<usize>,
    scroll: UniformListScrollHandle,
    drafts: HashMap<usize, Draft>,
    font_names: Vec<String>,
    font_pickers: HashMap<usize, Entity<FontPicker>>,
    busy: Option<usize>,
    message: Option<String>,
    error: Option<String>,
    subscriptions: Vec<Subscription>,
}
impl SettingsPage {
    pub(crate) fn new(
        source: SettingsSource,
        settings: &jaywalk::settings::Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search keys and descriptions…"));
        let subscription = cx.subscribe_in(&query, window, |page, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                page.filter(cx);
            }
        });
        let mut font_names = cx.text_system().all_font_names();
        font_names.sort_unstable_by_key(|name| name.to_lowercase());
        font_names.dedup();
        let mut page = Self {
            source: source.clone(),
            query,
            surface_focus: cx.focus_handle(),
            snapshot: None,
            category: Some(Category::General),
            changed: false,
            query_active: false,
            before_search: None,
            defaults: false,
            rows: vec![],
            scroll: UniformListScrollHandle::new(),
            drafts: HashMap::new(),
            font_names,
            font_pickers: HashMap::new(),
            busy: None,
            message: None,
            error: None,
            subscriptions: vec![subscription],
        };
        match crate::settings_service::source(source.clone(), settings, cx) {
            Ok(state) => {
                page.error = state.read(cx).error.clone();
                if let Some(snapshot) = state.read(cx).snapshot.clone() {
                    page.accept(snapshot, window, cx);
                }
                page.subscriptions.push(cx.observe_in(
                    &state,
                    window,
                    |page, state, window, cx| {
                        let state = state.read(cx);
                        let snapshot = state.snapshot.clone();
                        page.error = state.error.clone();
                        if let Some(snapshot) = snapshot {
                            page.accept(snapshot, window, cx);
                        }
                        cx.notify();
                    },
                ));
            }
            Err(error) => page.error = Some(error),
        }
        cx.spawn_in(window, async move |page, cx| {
            let result = cx
                .background_spawn(async move {
                    source.initialize()?;
                    source.read_snapshot()
                })
                .await;
            let _ = page.update_in(cx, |page, window, cx| match result {
                Ok(snapshot) => match crate::settings_service::publish_snapshot(
                    page.source.clone(),
                    snapshot,
                    cx,
                ) {
                    Ok(state) => {
                        let snapshot = state.read(cx).snapshot.clone();
                        if let Some(snapshot) = snapshot {
                            page.accept(snapshot, window, cx);
                        }
                    }
                    Err(error) => {
                        page.error = Some(error);
                        cx.notify();
                    }
                },
                Err(error) => {
                    page.error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
        page
    }
    pub(crate) fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.query.focus_handle(cx), cx);
    }
    pub(crate) fn show_defaults(&mut self, defaults: bool, cx: &mut Context<Self>) {
        if self.defaults != defaults {
            self.defaults = defaults;
            self.changed = false;
            self.filter(cx);
        }
    }
    fn select_category(&mut self, category: Option<Category>, cx: &mut Context<Self>) {
        self.category = category;
        self.changed = false;
        // Explicit navigation during a search becomes the browsing destination
        // when that search is cleared, rather than reviving an older category.
        if self.query_active {
            self.before_search = category;
        }
        self.filter(cx);
    }
    fn select_changed(&mut self, cx: &mut Context<Self>) {
        self.category = None;
        self.changed = true;
        if self.query_active {
            self.before_search = None;
        }
        self.filter(cx);
    }
    fn filter(&mut self, cx: &mut Context<Self>) {
        let searching = !self.query.read(cx).value().is_empty();
        if searching && !self.query_active {
            self.before_search = self.category;
            self.category = None;
        }
        if !searching && self.query_active {
            self.category = self.before_search;
        }
        self.query_active = searching;
        self.rows = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot.matching(
                    self.query.read(cx).value().as_ref(),
                    self.category,
                    self.changed && !self.defaults,
                )
            })
            .unwrap_or_default();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }
    fn value(&self, index: usize) -> Option<&toml::Value> {
        let key = &catalogue::SETTINGS[index].key;
        if self.defaults {
            Snapshot::default_value(key)
        } else {
            self.snapshot.as_ref()?.value(key)
        }
    }
    fn accept(&mut self, snapshot: Arc<Snapshot>, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|current| current.revision > snapshot.revision)
        {
            return;
        }
        let dark = gpui::component::Theme::global(cx).is_dark();
        for (index, draft) in &mut self.drafts {
            let key = &catalogue::SETTINGS[*index].key;
            if draft.dirty {
                if snapshot.explicit(key) != draft.baseline.as_ref() {
                    draft.error =
                        Some("Changed elsewhere; press Escape to reload before saving.".into());
                }
            } else {
                draft.baseline = snapshot.explicit(key).cloned();
                draft.color_dark = dark;
                let text = controls::color_text(*index, snapshot.value(key), dark);
                if draft.input.read(cx).value().as_ref() != text {
                    draft
                        .input
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                }
            }
        }
        self.snapshot = Some(snapshot);
        self.rows = self.snapshot.as_ref().expect("just assigned").matching(
            self.query.read(cx).value().as_ref(),
            self.category,
            self.changed && !self.defaults,
        );
        cx.notify();
    }
    fn input(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let dark = gpui::component::Theme::global(cx).is_dark();
        if let Some(draft) = self.drafts.get(&index) {
            let input = draft.input.clone();
            if !draft.dirty && draft.color_dark != dark {
                let text = controls::color_text(index, self.value(index), dark);
                self.drafts
                    .get_mut(&index)
                    .expect("draft exists")
                    .color_dark = dark;
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
            return input;
        }
        let value = controls::color_text(index, self.value(index), dark);
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("Automatic / inherited");
            input.set_value(value, window, cx);
            input.set_step(None::<NumberStep>, window, cx);
            input
        });
        self.drafts.insert(
            index,
            Draft {
                input: input.clone(),
                color_dark: dark,
                baseline: self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.explicit(&catalogue::SETTINGS[index].key))
                    .cloned(),
                dirty: false,
                error: None,
            },
        );
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |page, input, event, window, cx| match event {
                InputEvent::Change => {
                    let value = input.read(cx).value();
                    let dark = page.drafts.get(&index).expect("draft exists").color_dark;
                    let baseline = controls::color_text(index, page.value(index), dark);
                    if let Some(draft) = page.drafts.get_mut(&index) {
                        draft.dirty = value.as_ref() != baseline;
                        if !draft.dirty
                            || page.snapshot.as_ref().is_some_and(|snapshot| {
                                snapshot.explicit(&catalogue::SETTINGS[index].key)
                                    == draft.baseline.as_ref()
                            })
                        {
                            draft.error = None;
                        }
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    page.commit_draft(index, window, cx);
                }
                _ => {}
            },
        ));
        if catalogue::SETTINGS[index].kind == Kind::Number {
            self.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |page, _, event: &NumberInputEvent, window, cx| {
                    let NumberInputEvent::Step(action) = event;
                    page.step_number(index, *action == StepAction::Increment, window, cx);
                },
            ));
        }
        input
    }
    fn commit_draft(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.drafts.get(&index) else {
            return;
        };
        if !draft.dirty || draft.error.is_some() || self.busy.is_some() || self.defaults {
            return;
        }
        let value = controls::color_edit(
            index,
            self.value(index),
            draft.input.read(cx).value().as_ref(),
            draft.color_dark,
        );
        match value {
            Ok(value) => self.save(index, Some(value), window, cx),
            Err(error) => {
                self.drafts.get_mut(&index).expect("draft exists").error = Some(error);
                cx.notify();
            }
        }
    }
    fn save(
        &mut self,
        index: usize,
        value: Option<toml::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() || self.error.is_some() || self.defaults {
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let key = catalogue::SETTINGS[index].key.clone();
        let expected = self
            .drafts
            .get(&index)
            .filter(|draft| draft.dirty)
            .map(|draft| draft.baseline.clone())
            .unwrap_or_else(|| snapshot.explicit(&key).cloned());
        let patch = Patch {
            key,
            value,
            expected: Expect::Explicit(expected),
        };
        let source = self.source.clone();
        self.busy = Some(index);
        self.message = Some("Saving…".into());
        let (done, finished) = async_channel::bounded(1);
        let quit = App::on_app_quit(cx, move |_| {
            let finished = finished.clone();
            async move {
                let _ = finished.recv().await;
            }
        });
        let work = cx.background_spawn(async move {
            let result = preferences::patch(&source, patch);
            let _ = done.try_send(());
            result
        });
        cx.spawn_in(window, async move |page, cx| {
            let result = work.await;
            drop(quit);
            let _ = page.update_in(cx, |page, window, cx| {
                page.busy = None;
                match result {
                    Ok(snapshot) => {
                        if let Some(draft) = page.drafts.get_mut(&index) {
                            draft.dirty = false;
                            draft.error = None;
                        }
                        match crate::settings_service::publish_snapshot(
                            page.source.clone(),
                            snapshot,
                            cx,
                        ) {
                            Ok(state) => {
                                let snapshot = state.read(cx).snapshot.clone();
                                if let Some(snapshot) = snapshot {
                                    page.accept(snapshot, window, cx);
                                }
                                page.message = Some("Saved".into());
                            }
                            Err(error) => page.error = Some(error),
                        }
                    }
                    Err(error) => {
                        page.message = Some(error.clone());
                        if let Some(draft) = page.drafts.get_mut(&index) {
                            draft.error = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn focus_background(
        &self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let outside_input = std::iter::once(&self.query)
            .chain(self.drafts.values().map(|draft| &draft.input))
            .any(|input| {
                input.focus_handle(cx).is_focused(window)
                    && !input.read(cx).input_bounds().contains(&position)
            });
        let closed_select = self
            .font_pickers
            .values()
            .any(|picker| !picker.read(cx).is_open() && picker.focus_handle(cx).is_focused(window));
        if outside_input || closed_select {
            window.focus(&self.surface_focus, cx);
        }
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self
            .drafts
            .iter()
            .find(|(_, draft)| draft.dirty && draft.input.focus_handle(cx).is_focused(window))
            .map(|(index, _)| *index);
        if let Some(index) = dirty {
            let dark = gpui::component::Theme::global(cx).is_dark();
            let text = controls::color_text(index, self.value(index), dark);
            let baseline = self
                .snapshot
                .as_ref()
                .and_then(|s| s.explicit(&catalogue::SETTINGS[index].key))
                .cloned();
            let draft = self.drafts.get_mut(&index).expect("found draft");
            draft.dirty = false;
            draft.error = None;
            draft.color_dark = dark;
            draft.baseline = baseline;
            draft
                .input
                .update(cx, |input, cx| input.set_value(text, window, cx));
            cx.notify();
        } else if !self.query.read(cx).value().is_empty() {
            self.query
                .update(cx, |query, cx| query.set_value("", window, cx));
            self.filter(cx);
        } else {
            cx.emit(Event::Leave);
        }
    }
    fn render_row(
        &mut self,
        index: usize,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let setting = &catalogue::SETTINGS[index];
        let key = setting.key.clone();
        let explicit = !self.defaults
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.explicit(&key).is_some());
        let modified = !self.defaults
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.modified(setting));
        let value = self.value(index).cloned();
        let disabled = self.defaults || self.error.is_some();
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        let border = theme.border;
        let danger = theme.danger;
        let accent = theme.primary;
        let dark = theme.is_dark();
        let dependent = match key.as_str() {
            "updates.automatic_downloads" => Some("updates.automatic_checks"),
            "updates.automatic_installation" => Some("updates.automatic_downloads"),
            _ => None,
        };
        let prerequisite = dependent.is_none_or(|key| {
            self.snapshot
                .as_ref()
                .and_then(|s| s.value(key))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
        });
        let kind = if setting.advanced
            || (setting.kind == Kind::Number && value.as_ref().is_some_and(toml::Value::is_table))
        {
            Kind::Complex
        } else {
            setting.kind
        };
        let control = match kind {
            Kind::Boolean => div()
                .p(crate::spacing::interactive(cx))
                .debug_selector({
                    let key = key.clone();
                    move || format!("setting-{key}")
                })
                .child(
                    Switch::new(("setting-check", index))
                        .small()
                        .accessibility_label(setting.title.clone())
                        .checked(
                            value
                                .as_ref()
                                .and_then(toml::Value::as_bool)
                                .unwrap_or(false),
                        )
                        .disabled(disabled || !prerequisite)
                        .on_click(cx.listener(move |page, checked: &bool, window, cx| {
                            page.save(index, Some((*checked).into()), window, cx)
                        })),
                )
                .into_any_element(),
            Kind::Choice => {
                let owner = cx.entity().downgrade();
                let current = value
                    .as_ref()
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                Button::new(("setting-choice", index))
                    .debug_selector({
                        let key = key.clone();
                        move || format!("setting-{key}")
                    })
                    .label(current.replace('_', " "))
                    .px(crate::spacing::edge(cx))
                    .dropdown_caret(true)
                    .small()
                    .disabled(disabled)
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for choice in &catalogue::SETTINGS[index].choices {
                            let owner = owner.clone();
                            let choice = choice.clone();
                            menu = menu.item(
                                PopupMenuItem::new(choice.replace('_', " "))
                                    .checked(choice == current)
                                    .on_click(move |_, window, cx| {
                                        let _ = owner.update(cx, |page, cx| {
                                            page.save(
                                                index,
                                                Some(choice.clone().into()),
                                                window,
                                                cx,
                                            )
                                        });
                                    }),
                            );
                        }
                        menu
                    })
                    .into_any_element()
            }
            Kind::Font => self.font_control(index, disabled, window, cx),
            Kind::Complex => Button::new(("setting-edit", index))
                .small()
                .ghost()
                .border_1()
                .border_color(border)
                .label(if key == "accounts" {
                    "Manage accounts…"
                } else if key == "keyboard.profile" {
                    "Choose profile…"
                } else {
                    "Edit TOML…"
                })
                .command_hint(if key == "accounts" {
                    CommandId::Accounts
                } else if key == "keyboard.profile" {
                    CommandId::SelectKeybindings
                } else {
                    CommandId::OpenSettings
                })
                .disabled(self.defaults)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(if key == "accounts" {
                        Event::Accounts
                    } else if key == "keyboard.profile" {
                        Event::Keybindings
                    } else {
                        Event::EditToml
                    })
                }))
                .into_any_element(),
            _ if self.defaults => div()
                .w(px(210.))
                .text_sm()
                .text_right()
                .truncate()
                .child(controls::color_text(index, value.as_ref(), dark))
                .into_any_element(),
            _ => {
                let input = self.input(index, window, cx);
                let text = input.read(cx).value().to_string();
                let color_input = kind == Kind::Color || key == "ui.accent_color";
                let swatch = if key == "ui.accent_color" && text.trim() == "system" {
                    Some(accent)
                } else if color_input {
                    setting.parse_input(&text).ok().and_then(|value| {
                        value
                            .try_into::<jaywalk::settings::ColorSetting>()
                            .ok()
                            .map(|color| Hsla::from(rgb(color.resolve(dark).value())))
                    })
                } else {
                    None
                };
                let width = if kind == Kind::Number {
                    self.number_width(index, &text, window, cx)
                } else if color_input {
                    controls::dropdown_width("#FFFFFF", window, cx)
                } else {
                    px(if narrow { 240. } else { 210. })
                };
                let field = div()
                    .h_flex()
                    .gap(crate::spacing::interactive(cx))
                    .debug_selector({
                        let key = key.clone();
                        move || format!("setting-{key}")
                    })
                    .when_some(swatch, |row, color| {
                        row.child(
                            div()
                                .debug_selector(move || {
                                    format!("setting-swatch-{}", catalogue::SETTINGS[index].key)
                                })
                                .size(crate::spacing::space(5., cx))
                                .flex_shrink_0()
                                .rounded(crate::spacing::interactive(cx))
                                .border_1()
                                .border_color(border)
                                .bg(color),
                        )
                    })
                    .child(div().w(width).child(if kind == Kind::Number {
                        NumberInput::new(&input)
                            .small()
                            .placeholder("Auto")
                            .readonly(self.busy.is_some())
                            .text_center()
                            .accessibility_label(setting.title.clone())
                            .disabled(disabled)
                            .into_any_element()
                    } else {
                        Input::new(&input)
                            .small()
                            .readonly(self.busy.is_some())
                            .text_right()
                            .disabled(disabled)
                            .into_any_element()
                    }));
                if controls::paired_color(index) {
                    let separate = value.as_ref().is_some_and(toml::Value::is_table);
                    let field_dark = self
                        .drafts
                        .get(&index)
                        .expect("input created draft")
                        .color_dark;
                    div()
                        .h_flex()
                        .gap(crate::spacing::interactive(cx))
                        .child(
                            Button::new(("setting-separate-color", index))
                                .small()
                                .compact()
                                .ghost()
                                .icon(crate::appearance::active_icon(
                                    gpui::component::Icon::default().data(include_bytes!("../assets/icons/sun-moon.svg")),
                                    separate, cx,
                                ))
                                .toggled(separate)
                                .accessibility_label("Separate light/dark colors")
                                .tooltip(if separate {
                                    "Separate light/dark colors (on) — click to use this color for both"
                                } else {
                                    "Separate light/dark colors (off) — click to set colors individually"
                                })
                                .debug_selector(move || {
                                    format!("setting-separate-{}", catalogue::SETTINGS[index].key)
                                })
                                .disabled(disabled || self.busy.is_some())
                                .on_click(cx.listener(move |page, _, window, cx| {
                                    if let Some(value) = page.value(index) {
                                        let value = if value.as_str() == Some("system") {
                                            let ui = &page
                                                .snapshot
                                                .as_ref()
                                                .expect("loaded settings")
                                                .settings
                                                .ui;
                                            let color = ui.accent_color.resolve(
                                                field_dark,
                                                crate::platform::accent::read(field_dark),
                                                ui.accent_color_fallback,
                                            );
                                            toml::Value::String(format!("#{:06x}", color.value()))
                                        } else {
                                            value.clone()
                                        };
                                        let next =
                                            controls::separate_color(&value, !separate, field_dark);
                                        page.save(index, Some(next), window, cx);
                                    }
                                })),
                        )
                        .child(field)
                        .into_any_element()
                } else {
                    field.into_any_element()
                }
            }
        };
        let draft_error = self
            .drafts
            .get(&index)
            .and_then(|draft| draft.error.clone());
        let default = if setting.key == "diff.colors" {
            "Built-in diff palette".into()
        } else {
            preferences::display(Snapshot::default_value(&setting.key))
        };
        let provenance = if explicit {
            "Set in this file"
        } else if self.defaults {
            "Built-in"
        } else {
            "Inherited"
        };
        let details = format!(
            "{} · {}\nDefault: {default} · {provenance}",
            setting.category.label(),
            setting.key
        );
        let notice = draft_error.or_else(|| {
            (!prerequisite).then(|| format!("Requires {}", dependent.unwrap_or_default()))
        });
        let owner = cx.entity().downgrade();
        let copy_key = setting.key.clone();
        let reset_label = format!("Reset to default: {default}");
        let title = div()
            .id(("setting-title", index))
            .h_flex()
            .items_center()
            .gap(crate::spacing::interactive(cx))
            .child(div().font_medium().child(setting.title.clone()))
            .when(
                modified
                    && !matches!(
                        setting.key.as_str(),
                        "accounts" | "diff.colors" | "theme.overrides"
                    ),
                |row| {
                    row.child(
                        Button::new(("setting-reset", index))
                            .debug_selector({
                                let key = setting.key.clone();
                                move || format!("reset-{key}")
                            })
                            .ghost()
                            .xsmall()
                            .compact()
                            .icon(gpui::component::IconName::Undo2)
                            .tooltip(reset_label.clone())
                            .disabled(disabled)
                            .on_click(cx.listener(move |page, _, window, cx| {
                                page.save(index, None, window, cx)
                            })),
                    )
                },
            )
            .context_menu(move |menu, _, _| {
                let copy_key = copy_key.clone();
                let owner = owner.clone();
                menu.item(
                    PopupMenuItem::new("Copy setting key").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_key.clone()))
                    }),
                )
                .item(
                    PopupMenuItem::new(reset_label.clone())
                        .disabled(
                            !explicit
                                || disabled
                                || matches!(
                                    setting.key.as_str(),
                                    "accounts" | "diff.colors" | "theme.overrides"
                                ),
                        )
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |page, cx| page.save(index, None, window, cx));
                        }),
                )
            });
        let text = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .gap(crate::spacing::interactive(cx))
            .child(title)
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .id(("setting-description", index))
                    .tooltip({
                        let description = format!("{}\n\n{}", setting.description, details);
                        move |window, cx| {
                            gpui::component::tooltip::Tooltip::new(description.clone())
                                .build(window, cx)
                        }
                    })
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_ellipsis()
                    .line_clamp(if notice.is_some() { 1 } else { 2 })
                    .line_height(relative(1.35))
                    .child(
                        setting
                            .summary
                            .as_ref()
                            .unwrap_or(&setting.description)
                            .clone(),
                    ),
            )
            .when_some(notice, |row, notice| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(if !prerequisite { muted } else { danger })
                        .truncate()
                        .child(notice),
                )
            });
        let controls = div()
            // Serialize edits without flashing every control's disabled colors
            // for a disk write that normally finishes within a frame.
            .when(self.busy.is_some(), |controls| {
                controls
                    .capture_any_mouse_down(|_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .capture_key_down(|_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
            })
            .h_flex()
            .items_center()
            .justify_end()
            .w(px(if narrow { 240. } else { 210. }))
            .flex_shrink_0()
            .when(narrow, |row| row.self_end())
            .child(control);

        div()
            .id(("setting-row", index))
            .debug_selector({
                let key = setting.key.clone();
                move || format!("settings-row-{key}")
            })
            .v_flex()
            .h(px(self
                .snapshot
                .as_ref()
                .map_or(16., |snapshot| snapshot.settings.ui.font_size)
                * if narrow { 8. } else { 4.5 })
                + crate::spacing::space(4., cx))
            .w_full()
            .max_w(px(1120.))
            .mx_auto()
            .relative()
            .px(crate::spacing::space(4., cx))
            .py(crate::spacing::space(2., cx))
            .child(
                div()
                    .size_full()
                    .overflow_hidden()
                    .when(narrow, |row| row.v_flex().gap(crate::spacing::edge(cx)))
                    .when(!narrow, |row| {
                        row.h_flex()
                            .items_center()
                            .gap(crate::spacing::space(4., cx))
                    })
                    .child(text)
                    .child(controls),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(crate::spacing::space(4., cx))
                    .right(crate::spacing::space(4., cx))
                    .h(px(1.))
                    .bg(border),
            )
            .into_any_element()
    }
}
impl Focusable for SettingsPage {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}
impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = f32::from(window.viewport_size().width) < 900.;
        let compact = f32::from(window.viewport_size().width) < 650.;
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        let border = theme.border;
        let query = self.query.read(cx).value();
        let searching = !query.is_empty();
        let modified = self.snapshot.as_ref().map_or(0, |snapshot| {
            catalogue::SETTINGS
                .iter()
                .filter(|setting| !setting.advanced && snapshot.modified(setting))
                .count()
        });
        let mut nav = div()
            .v_flex()
            .w(px(if compact { 170. } else { 235. }))
            .flex_shrink_0()
            .p(crate::spacing::space(3., cx))
            .gap(crate::spacing::space(1., cx))
            .border_r_1()
            .border_color(border)
            .bg(crate::appearance::surface(
                jaywalk::theme::Surface::History,
                jaywalk::theme::Part::Background,
                cx,
            ))
            .child(
                div().debug_selector(|| "settings-search".into()).child(
                    Input::new(&self.query)
                        .small()
                        .px(crate::spacing::space(2., cx))
                        .border_color(border),
                ),
            );
        nav = nav
            .child(
                Button::new("settings-all")
                    .debug_selector(|| "settings-all".into())
                    .accessibility_label("All settings")
                    .child(div().w_full().text_left().child("All settings"))
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .selected(self.category.is_none() && !self.changed)
                    .justify_start()
                    .on_click(cx.listener(|page, _, _, cx| {
                        page.select_category(None, cx);
                    })),
            )
            .child(
                Button::new("settings-modified")
                    .debug_selector(|| "settings-modified".into())
                    .accessibility_label("Changed from defaults")
                    .child(
                        div()
                            .w_full()
                            .text_left()
                            .child(format!("Changed from defaults  {modified}")),
                    )
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .selected(self.changed)
                    .justify_start()
                    .disabled(self.defaults)
                    .on_click(cx.listener(|page, _, _, cx| {
                        page.select_changed(cx);
                    })),
            );
        for (index, category) in Category::ALL.into_iter().enumerate() {
            nav = nav.child(
                Button::new(("settings-category", index))
                    .debug_selector(move || format!("settings-category-{index}"))
                    .accessibility_label(category.label())
                    .child(div().w_full().text_left().child(category.label()))
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .selected(self.category == Some(category))
                    .justify_start()
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.select_category(Some(category), cx);
                    })),
            );
        }
        let nav = if compact {
            let owner = cx.entity().downgrade();
            div()
                .v_flex()
                .w_full()
                .flex_shrink_0()
                .p(crate::spacing::space(3., cx))
                .gap(crate::spacing::space(1., cx))
                .border_b_1()
                .border_color(border)
                .child(
                    div().debug_selector(|| "settings-search".into()).child(
                        Input::new(&self.query)
                            .small()
                            .px(crate::spacing::space(2., cx))
                            .border_color(border),
                    ),
                )
                .child(
                    div()
                        .h_flex()
                        .gap(crate::spacing::space(2., cx))
                        .child(
                            Button::new("settings-category-menu")
                                .debug_selector(|| "settings-category-menu".into())
                                .px(crate::spacing::edge(cx))
                                .dropdown_caret(true)
                                .label(if self.changed {
                                    "Changed from defaults"
                                } else {
                                    self.category.map_or("All settings", Category::label)
                                })
                                .small()
                                .dropdown_menu(move |menu, _, _| {
                                    let mut menu = menu;
                                    let all = owner.clone();
                                    menu = menu.item(PopupMenuItem::new("All settings").on_click(
                                        move |_, _, cx| {
                                            let _ = all.update(cx, |page, cx| {
                                                page.select_category(None, cx);
                                            });
                                        },
                                    ));
                                    for category in Category::ALL {
                                        let owner = owner.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(category.label()).on_click(
                                                move |_, _, cx| {
                                                    let _ = owner.update(cx, |page, cx| {
                                                        page.select_category(Some(category), cx);
                                                    });
                                                },
                                            ),
                                        );
                                    }
                                    menu
                                }),
                        )
                        .child(
                            Button::new("settings-modified-small")
                                .debug_selector(|| "settings-modified-small".into())
                                .small()
                                .ghost()
                                .label(format!("Changed {modified}"))
                                .selected(self.changed)
                                .disabled(self.defaults)
                                .on_click(cx.listener(|page, _, _, cx| {
                                    page.select_changed(cx);
                                })),
                        ),
                )
                .into_any_element()
        } else {
            nav.into_any_element()
        };
        let source = if self.defaults {
            "Built-in defaults · read only".into()
        } else {
            self.source.path().display().to_string()
        };
        let title = if searching {
            format!(
                "{} matching {}",
                self.rows.len(),
                if self.changed {
                    "changed settings"
                } else {
                    "settings"
                }
            )
        } else if self.changed {
            "Changed from defaults".into()
        } else {
            self.category.map_or("All settings", Category::label).into()
        };
        let header_surface = jaywalk::theme::Surface::Header;
        let header_muted =
            crate::appearance::surface(header_surface, jaywalk::theme::Part::MutedForeground, cx);
        let header = div()
            .debug_selector(|| "settings-header".into())
            .bg(crate::appearance::surface(
                header_surface,
                jaywalk::theme::Part::Background,
                cx,
            ))
            .text_color(crate::appearance::surface(
                header_surface,
                jaywalk::theme::Part::Foreground,
                cx,
            ))
            .v_flex()
            .gap(crate::spacing::interactive(cx))
            .p(crate::spacing::space(3., cx))
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .when(narrow, |row| row.v_flex())
                    .when(!narrow, |row| row.h_flex().items_center().justify_between())
                    .gap(crate::spacing::space(2., cx))
                    .child(div().text_lg().font_medium().child(title))
                    .child(
                        div()
                            .h_flex()
                            .gap(crate::spacing::space(2., cx))
                            .child(
                                Button::new("settings-defaults")
                                    .small()
                                    .ghost()
                                    .border_1()
                                    .border_color(border)
                                    .label(if self.defaults {
                                        "User settings"
                                    } else {
                                        "Defaults"
                                    })
                                    .command_hint(if self.defaults {
                                        CommandId::ShowSettings
                                    } else {
                                        CommandId::ShowDefaultSettings
                                    })
                                    .on_click(cx.listener(|page, _, _, cx| {
                                        page.show_defaults(!page.defaults, cx)
                                    })),
                            )
                            .child(
                                Button::new("settings-edit-toml")
                                    .small()
                                    .border_1()
                                    .border_color(border)
                                    .label("Edit TOML…")
                                    .command_hint(CommandId::OpenSettings)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::EditToml))),
                            ),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::edge(cx))
                    .text_xs()
                    .text_color(header_muted)
                    .child(div().flex_1().min_w_0().truncate().child(source))
                    .when_some(
                        self.error.clone().or_else(|| self.message.clone()),
                        |row, message| {
                            row.child(
                                div()
                                    .id("settings-save-status")
                                    .max_w(relative(0.5))
                                    .truncate()
                                    .child(message.clone())
                                    .tooltip(move |window, cx| {
                                        gpui::component::tooltip::Tooltip::new(message.clone())
                                            .build(window, cx)
                                    }),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(header_muted)
                    .child("Changes apply live unless a setting says otherwise."),
            );
        let owner = cx.entity().downgrade();
        let count = self.rows.len();
        let list = uniform_list("settings-rows", count, move |range, window, cx| {
            owner
                .update(cx, |page, cx| {
                    range
                        .filter_map(|index| page.rows.get(index).copied())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .map(|index| page.render_row(index, narrow, window, cx))
                        .collect()
                })
                .unwrap_or_default()
        })
        .size_full()
        .track_scroll(&self.scroll);
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(list)
            .child(Scrollbar::vertical(&self.scroll))
            .when(count == 0, |row| {
                row.child(
                    div()
                        .absolute()
                        .inset_0()
                        .p(crate::spacing::space(6., cx))
                        .text_color(muted)
                        .child(if self.snapshot.is_none() {
                            "Loading settings…"
                        } else {
                            "No settings match these filters."
                        }),
                )
            });
        div()
            .id("settings-page")
            .debug_selector(|| "settings-page".into())
            .key_context("JaywalkSettings")
            .track_focus(&self.surface_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, event: &MouseDownEvent, window, cx| {
                    page.focus_background(event.position, window, cx);
                }),
            )
            .size_full()
            .line_height(relative(1.35))
            .when(compact, |row| row.v_flex())
            .when(!compact, |row| row.h_flex().items_stretch())
            .on_action(cx.listener(|page, _: &Cancel, window, cx| page.cancel(window, cx)))
            .child(nav)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(header)
                    .child(body),
            )
    }
}

#[cfg(test)]
impl SettingsPage {
    pub(crate) fn test_status(&self, cx: &App) -> String {
        format!(
            "query={:?} category={:?} changed={} loaded={} error={:?} message={:?} rows={:?}",
            self.query.read(cx).value(),
            self.category,
            self.changed,
            self.snapshot.is_some(),
            self.error,
            self.message,
            self.rows
                .iter()
                .map(|i| &catalogue::SETTINGS[*i].key)
                .collect::<Vec<_>>()
        )
    }
}
