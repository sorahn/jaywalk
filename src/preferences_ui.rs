//! Retained native settings content. Controls edit the shared TOML source.
mod colors;
mod controls;
mod groups;
mod keyboard;
mod session;
mod structured;
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
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
actions!(preferences_ui, [Cancel, SaveDraft]);
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some("JaywalkSettings")),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-s"
            } else {
                "ctrl-s"
            },
            SaveDraft,
            Some("JaywalkSettings"),
        ),
    ]);
}
// Reserve the standard small button's line box, padding and border even when
// a destination has no header actions. Use a minimum so wrapping can still grow.
fn header_row(window: &Window, cx: &App) -> Div {
    div().min_h(window.rem_size() * 0.875 * 1.25 + crate::spacing::interactive(cx) * 2. + px(2.))
}

fn input_text(value: Option<&toml::Value>) -> String {
    value
        .map(|value| preferences::display(Some(value)))
        .unwrap_or_default()
}
pub(crate) enum Event {
    Leave,
    Close { quit: bool },
    EditToml,
    Accounts,
    Keybindings,
}
impl EventEmitter<Event> for SettingsPage {}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination {
    Settings,
    Accounts,
    Keyboard,
}

struct Draft {
    input: Entity<InputState>,
    baseline: Option<toml::Value>,
    color_dark: bool,
    dirty: bool,
    error: Option<String>,
}
pub(crate) struct SettingsPage {
    source: SettingsSource,
    destination: Destination,
    accounts: Option<Entity<crate::account_ui::AccountPanel>>,
    keyboard: keyboard::Keyboard,
    query: Entity<InputState>,
    surface_focus: FocusHandle,
    snapshot: Option<Arc<Snapshot>>,
    persisted: Option<Arc<Snapshot>>,
    pending: BTreeMap<usize, Patch>,
    raw_preview: BTreeMap<usize, Patch>,
    close_after_save: Option<bool>,
    close_prompt: bool,
    category: Option<Category>,
    changed: bool,
    query_active: bool,
    before_search: Option<Category>,
    before_search_row: Option<groups::Row>,
    defaults: bool,
    rows: Vec<usize>,
    scroll: gpui::ListState,
    display_rows: Vec<groups::Row>,
    expanded: std::collections::BTreeSet<jaywalk::preferences::groups::Group>,
    row_focus: Vec<FocusHandle>,
    list_width: Pixels,
    list_hints_dirty: bool,
    drafts: HashMap<usize, Draft>,
    font_names: Vec<String>,
    font_pickers: HashMap<usize, Entity<FontPicker>>,
    color_pickers: HashMap<usize, colors::Picker>,
    structured: structured::Editors,
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
        let owner = cx.entity_id();
        cx.on_release(move |page, cx| {
            let _ = crate::settings_service::clear_preview(&page.source, owner, cx);
        })
        .detach();
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search keys and descriptions…"));
        let subscription = cx.subscribe_in(&query, window, |page, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                page.destination = Destination::Settings;
                page.filter(cx);
            }
        });
        let mut font_names = cx.text_system().all_font_names();
        font_names.sort_unstable_by_key(|name| name.to_lowercase());
        font_names.dedup();
        let mut page = Self {
            source: source.clone(),
            destination: Destination::Settings,
            accounts: None,
            keyboard: keyboard::Keyboard::new(window, cx),
            query,
            surface_focus: cx.focus_handle(),
            snapshot: None,
            persisted: None,
            pending: BTreeMap::new(),
            raw_preview: BTreeMap::new(),
            close_after_save: None,
            close_prompt: false,
            category: Some(Category::General),
            changed: false,
            query_active: false,
            before_search: None,
            before_search_row: None,
            defaults: false,
            rows: vec![],
            scroll: gpui::ListState::new(0, ListAlignment::Top, px(100.)),
            display_rows: Vec::new(),
            expanded: Default::default(),
            row_focus: catalogue::SETTINGS
                .iter()
                .map(|_| cx.focus_handle())
                .collect(),
            list_width: px(0.),
            list_hints_dirty: true,
            drafts: HashMap::new(),
            font_names,
            font_pickers: HashMap::new(),
            color_pickers: HashMap::new(),
            structured: Default::default(),
            busy: None,
            message: None,
            error: None,
            subscriptions: vec![subscription],
        };
        page.load_groups(cx);
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
        if self.destination == Destination::Accounts
            && let Some(accounts) = &self.accounts
        {
            accounts.update(cx, |accounts, cx| accounts.focus(window, cx));
        } else if self.destination == Destination::Keyboard {
            window.focus(&self.keyboard.query.focus_handle(cx), cx);
        } else {
            window.focus(&self.query.focus_handle(cx), cx);
        }
    }
    pub(crate) fn show_accounts(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.destination = Destination::Accounts;
        if let Some(accounts) = &self.accounts {
            if command != CommandId::Accounts {
                accounts.update(cx, |accounts, cx| accounts.show(command, window, cx));
            }
        } else {
            if !cx.has_global::<jaywalk::accounts::application::Accounts>() {
                jaywalk::accounts::application::init(cx);
            }
            let panel =
                cx.new(|cx| crate::account_ui::AccountPanel::new(command, window, cx).embedded());
            self.subscriptions.push(cx.subscribe_in(
                &panel,
                window,
                |page, _, _: &crate::account_ui::LeavePage, window, cx| {
                    page.destination = Destination::Settings;
                    page.focus(window, cx);
                    cx.notify();
                },
            ));
            self.accounts = Some(panel);
        }
        self.focus(window, cx);
        cx.notify();
    }
    pub(crate) fn show_defaults(&mut self, defaults: bool, cx: &mut Context<Self>) {
        self.destination = Destination::Settings;
        cx.notify();
        if self.defaults != defaults {
            self.defaults = defaults;
            self.changed = false;
            self.filter(cx);
        }
    }
    fn select_category(&mut self, category: Option<Category>, cx: &mut Context<Self>) {
        self.destination = Destination::Settings;
        self.category = category;
        self.changed = false;
        // Explicit navigation during a search becomes the browsing destination
        // when that search is cleared, rather than reviving an older category.
        if self.query_active {
            self.before_search = category;
            self.before_search_row = None;
        }
        self.filter(cx);
    }
    fn select_changed(&mut self, cx: &mut Context<Self>) {
        self.destination = Destination::Settings;
        self.category = None;
        self.changed = true;
        if self.query_active {
            self.before_search = None;
            self.before_search_row = None;
        }
        self.filter(cx);
    }
    fn browsing_row_index(&self) -> usize {
        self.scroll.logical_scroll_top().item_ix
    }
    fn filter(&mut self, cx: &mut Context<Self>) {
        let searching = !self.query.read(cx).value().is_empty();
        if searching && !self.query_active {
            self.before_search_row = self.display_rows.get(self.browsing_row_index()).copied();
            self.before_search = self.category;
            self.category = None;
        }
        let restoring = !searching && self.query_active;
        if restoring {
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
        self.rebuild_rows(cx);
        let row = if restoring {
            self.before_search_row
                .and_then(|key| self.display_rows.iter().position(|row| *row == key))
                .unwrap_or(0)
        } else {
            0
        };
        self.scroll.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
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
        self.structured.prompt_serial += 1;
        if self
            .persisted
            .as_ref()
            .is_some_and(|current| current.revision > snapshot.revision)
        {
            return;
        }
        self.persisted = Some(snapshot.clone());
        let snapshot = if self.pending.is_empty() {
            snapshot
        } else {
            match snapshot.preview(&session::ordered_patches(&self.pending)) {
                Ok(preview) => Arc::new(preview),
                Err(error) => {
                    self.message = Some(error);
                    cx.notify();
                    return;
                }
            }
        };
        let dark = gpui::component::Theme::global(cx).is_dark();
        for (index, draft) in &mut self.drafts {
            let key = &catalogue::SETTINGS[*index].key;
            if draft.dirty {
                if self
                    .persisted
                    .as_ref()
                    .and_then(|saved| saved.explicit(key))
                    != draft.baseline.as_ref()
                {
                    draft.error =
                        Some("Changed elsewhere; press Escape to reload before saving.".into());
                }
            } else {
                draft.baseline = self
                    .persisted
                    .as_ref()
                    .and_then(|saved| saved.explicit(key))
                    .cloned();
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
        self.reload_keyboard_if_clean(window, cx);
        self.rows = self.snapshot.as_ref().expect("just assigned").matching(
            self.query.read(cx).value().as_ref(),
            self.category,
            self.changed && !self.defaults,
        );
        self.rebuild_rows(cx);
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
                    .persisted
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
                            || page.persisted.as_ref().is_some_and(|snapshot| {
                                snapshot.explicit(&catalogue::SETTINGS[index].key)
                                    == draft.baseline.as_ref()
                            })
                        {
                            draft.error = None;
                        }
                    }
                    page.preview_input(index, cx);
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
            Ok(value) => self.stage(index, Some(value), window, cx),
            Err(error) => {
                self.drafts.get_mut(&index).expect("draft exists").error = Some(error);
                cx.notify();
            }
        }
    }
    fn focus_background(
        &self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let outside_input = std::iter::once(&self.query)
            .chain(self.drafts.values().map(|draft| &draft.input))
            .chain(self.structured.inputs())
            .chain(std::iter::once(&self.keyboard.query))
            .chain(self.keyboard.inputs.values().flatten())
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
            self.raw_preview.remove(&index);
            self.refresh_preview(cx);
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
        let disabled = self.defaults
            || self.error.is_some()
            || (key.starts_with("theme.shell_auto.seed.")
                && self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.value("theme.shell_auto.source"))
                    .and_then(toml::Value::as_str)
                    == Some("code_theme"));
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
            || setting.toml_only
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
                            page.stage(index, Some((*checked).into()), window, cx)
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
                    .label(setting.choice_label(&current))
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
                                PopupMenuItem::new(
                                    catalogue::SETTINGS[index].choice_label(&choice),
                                )
                                .checked(choice == current)
                                .on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |page, cx| {
                                        page.stage(index, Some(choice.clone().into()), window, cx)
                                    });
                                }),
                            );
                        }
                        menu
                    })
                    .into_any_element()
            }
            Kind::Font => self.font_control(index, disabled, window, cx),
            Kind::Structured => self.structured_control(index, disabled, window, cx),
            Kind::Complex => Button::new(("setting-edit", index))
                .debug_selector(move || {
                    format!("setting-edit-toml-{}", catalogue::SETTINGS[index].key)
                })
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
                    .when(color_input, |row| {
                        row.child(self.color_picker(
                            index,
                            swatch,
                            disabled || self.busy.is_some(),
                            window,
                            cx,
                        ))
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
                                        page.stage(index, Some(next), window, cx);
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
        let notice_is_error = draft_error.is_some();
        let notice = draft_error
            .or_else(|| {
                (!prerequisite).then(|| format!("Requires {}", dependent.unwrap_or_default()))
            })
            .or_else(|| {
                (!crate::settings_service::previewable(&setting.key) && kind != Kind::Complex)
                    .then(|| "Takes effect after Save.".to_owned())
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
                                page.stage(index, None, window, cx)
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
                            let _ =
                                owner.update(cx, |page, cx| page.stage(index, None, window, cx));
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
                        .text_color(if notice_is_error { danger } else { muted })
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
            .w_full()
            .max_w(px(1120.))
            .mx_auto()
            .relative()
            .px(crate::spacing::space(2., cx))
            .py(crate::spacing::space(2., cx))
            .child(
                div()
                    .w_full()
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
            .when(kind == Kind::Structured, |row| {
                row.child(self.structured_details(index, disabled, window, cx))
            })
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(crate::spacing::space(2., cx))
                    .right(crate::spacing::space(2., cx))
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
        let accounts_active = self.destination == Destination::Accounts;
        let keyboard_active = self.destination == Destination::Keyboard;
        let settings_active = self.destination == Destination::Settings;
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
                .filter(|setting| {
                    !setting.advanced
                        && setting.key != "keyboard.profile"
                        && snapshot.modified(setting)
                })
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
            .child(div().h(px(1.)).my(crate::spacing::edge(cx)).bg(border))
            .child(
                Button::new("settings-all")
                    .debug_selector(|| "settings-all".into())
                    .accessibility_label("All settings")
                    .child(div().w_full().text_left().child("All settings"))
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .selected(settings_active && self.category.is_none() && !self.changed)
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
                            .min_w_0()
                            .h_flex()
                            .items_center()
                            .gap(crate::spacing::edge(cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child("Changed from defaults"),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .debug_selector(|| "settings-modified-count".into())
                                    .child(modified.to_string()),
                            ),
                    )
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .selected(settings_active && self.changed)
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
                    .selected(settings_active && self.category == Some(category))
                    .justify_start()
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.select_category(Some(category), cx);
                    })),
            );
        }
        nav = nav
            .child(div().h(px(1.)).my(crate::spacing::edge(cx)).bg(border))
            .child(
                Button::new("settings-keyboard")
                    .debug_selector(|| "settings-keyboard".into())
                    .child(div().w_full().text_left().child("Keyboard Shortcuts"))
                    .command_hint(CommandId::SelectKeybindings)
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .justify_start()
                    .selected(keyboard_active)
                    .on_click(cx.listener(|page, _, window, cx| page.show_keyboard(window, cx))),
            )
            .child(
                Button::new("settings-accounts")
                    .debug_selector(|| "settings-accounts".into())
                    .accessibility_label("Accounts")
                    .child(div().w_full().text_left().child("Accounts"))
                    .command_hint(CommandId::Accounts)
                    .ghost()
                    .px(crate::spacing::space(2., cx))
                    .justify_start()
                    .selected(accounts_active)
                    .on_click(cx.listener(|page, _, window, cx| {
                        page.show_accounts(CommandId::Accounts, window, cx)
                    })),
            );
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
                                .label(if accounts_active {
                                    "Accounts"
                                } else if self.changed {
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
                                    let owner = owner.clone();
                                    let keyboard_owner = owner.clone();
                                    menu.separator()
                                        .item(PopupMenuItem::new("Keyboard Shortcuts").on_click(
                                            move |_, window, cx| {
                                                let _ = keyboard_owner.update(cx, |page, cx| {
                                                    page.show_keyboard(window, cx)
                                                });
                                            },
                                        ))
                                        .item(PopupMenuItem::new("Accounts").on_click(
                                            move |_, window, cx| {
                                                let _ = owner.update(cx, |page, cx| {
                                                    page.show_accounts(
                                                        CommandId::Accounts,
                                                        window,
                                                        cx,
                                                    )
                                                });
                                            },
                                        ))
                                }),
                        )
                        .child(
                            Button::new("settings-modified-small")
                                .debug_selector(|| "settings-modified-small".into())
                                .small()
                                .ghost()
                                .label(format!("Changed {modified}"))
                                .selected(settings_active && self.changed)
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
        let source = if accounts_active {
            "Connections shared across all windows".into()
        } else if self.defaults {
            "Built-in defaults · read only".into()
        } else {
            self.source.path().display().to_string()
        };
        let title = if keyboard_active {
            "Keyboard Shortcuts".into()
        } else if accounts_active {
            "Accounts".into()
        } else if searching {
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
            .p(crate::spacing::space(2., cx))
            .border_b_1()
            .border_color(border)
            .child(
                header_row(window, cx)
                    .when(narrow, |row| row.v_flex())
                    .when(!narrow, |row| row.h_flex().items_center().justify_between())
                    .gap(crate::spacing::space(2., cx))
                    .child(
                        div()
                            .debug_selector(|| "settings-header-title".into())
                            .text_lg()
                            .font_medium()
                            .child(title),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap(crate::spacing::space(2., cx))
                            .when(!settings_active, |row| row.hidden())
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
                                    .debug_selector(|| "settings-edit-toml".into())
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
                        (!accounts_active)
                            .then(|| self.error.clone().or_else(|| self.message.clone()))
                            .flatten(),
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
                    .child(if accounts_active {
                        "Account actions take effect immediately."
                    } else {
                        "Changes preview live. Save writes them; Discard restores saved settings."
                    }),
            );
        let header = if keyboard_active {
            self.render_keyboard_header(window, cx)
        } else {
            header.into_any_element()
        };
        let owner = cx.entity().downgrade();
        let count = self.display_rows.len();
        let width = self.scroll.viewport_bounds().size.width;
        if self.list_hints_dirty || width != self.list_width {
            self.list_width = width;
            self.list_hints_dirty = false;
            self.scroll.clone().with_uniform_item_height(px(90.));
            let owner = owner.clone();
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |page, cx| {
                    page.scroll.clone().with_uniform_item_height(px(90.));
                    cx.notify();
                });
            });
        }
        let list = list(self.scroll.clone(), move |index, window, cx| {
            owner
                .update(cx, |page, cx| {
                    page.render_settings_entry(index, narrow, window, cx)
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        let body = div()
            .relative()
            .debug_selector(|| "settings-content".into())
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
        let body = if accounts_active {
            self.accounts
                .as_ref()
                .expect("Accounts is constructed before display")
                .clone()
                .into_any_element()
        } else if keyboard_active {
            self.render_keyboard(window, cx)
        } else {
            body.into_any_element()
        };
        let footer = div()
            .h_flex()
            .justify_between()
            .items_center()
            .p(crate::spacing::space(3., cx))
            .gap(crate::spacing::edge(cx))
            .bg(crate::appearance::surface(
                header_surface,
                jaywalk::theme::Part::Background,
                cx,
            ))
            .border_t_1()
            .border_color(border)
            .child(
                Button::new("settings-discard")
                    .small()
                    .label("Discard")
                    .debug_selector(|| "settings-discard".into())
                    .disabled(!self.has_changes() || self.busy.is_some())
                    .on_click(cx.listener(|page, _, window, cx| page.discard(window, cx))),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::edge(cx))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(if self.busy.is_some() {
                                "Saving…"
                            } else if self.has_changes() {
                                "Unsaved changes"
                            } else {
                                ""
                            }),
                    )
                    .child(
                        Button::new("settings-save")
                            .small()
                            .primary()
                            .label("Save")
                            .child(session::SaveHint)
                            .debug_selector(|| "settings-save".into())
                            .disabled(
                                !self.has_changes() || self.busy.is_some() || self.error.is_some(),
                            )
                            .on_click(cx.listener(|page, _, window, cx| page.save_all(window, cx))),
                    ),
            );
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
            .on_action(cx.listener(|page, _: &SaveDraft, window, cx| page.save_all(window, cx)))
            .child(nav)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(header)
                    .child(body)
                    .when(!accounts_active, |column| column.child(footer)),
            )
    }
}

#[cfg(test)]
impl SettingsPage {
    pub(crate) fn test_first_setting_key(&self) -> Option<&str> {
        self.display_rows
            .get(self.browsing_row_index())
            .map(|row| match row {
                groups::Row::Setting(index) => catalogue::SETTINGS[*index].key.as_str(),
                groups::Row::Group(group) => group.key(),
                groups::Row::Section(_, section) => section,
            })
    }
    pub(crate) fn test_row_labels(&self) -> Vec<String> {
        self.display_rows
            .iter()
            .map(|row| match row {
                groups::Row::Setting(index) => catalogue::SETTINGS[*index].key.clone(),
                groups::Row::Group(group) => format!("group:{}", group.key()),
                groups::Row::Section(_, section) => format!("section:{section}"),
            })
            .collect()
    }
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
