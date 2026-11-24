//! Application command remapping. Toolkit action/context rules remain in TOML.
mod keys;
use super::*;
use gpui::component::ElementExt as _;
use gpui::component::kbd::Kbd;
use gpui::component::tooltip::{ManagedTooltipExt as _, Tooltip};
use jaywalk::keybinding_profiles as profiles;
use std::collections::BTreeSet;

#[derive(PartialEq)]
struct KeyboardRow {
    command: CommandId,
    slots: usize,
    error: Option<String>,
}

#[derive(PartialEq)]
struct RowGeometry {
    font: SharedString,
    size: Pixels,
    edge: Pixels,
    interactive: Pixels,
}

pub(super) struct Keyboard {
    pub(super) drafts: BTreeMap<String, profiles::Draft>,
    pub(super) active: Option<String>,
    names: Vec<String>,
    pub(super) query: Entity<InputState>,
    pub(super) inputs: BTreeMap<(String, CommandId), Vec<Entity<InputState>>>,
    dirty: BTreeSet<(String, CommandId)>,
    errors: BTreeMap<(String, CommandId), String>,
    pub(super) loading: bool,
    serial: u64,
    scroll: gpui::ListState,
    rows: Vec<KeyboardRow>,
    row_profile: Option<String>,
    geometry: Option<RowGeometry>,
    hint_width: Pixels,
    row_focus: BTreeMap<CommandId, FocusHandle>,
    commands: Vec<CommandId>,
    defaults: BTreeMap<CommandId, Vec<String>>,
}
impl Keyboard {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SettingsPage>) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search commands and shortcuts…"));
        cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        Self {
            drafts: BTreeMap::new(),
            active: None,
            names: vec![],
            query,
            inputs: BTreeMap::new(),
            dirty: BTreeSet::new(),
            errors: BTreeMap::new(),
            loading: false,
            serial: 0,
            scroll: gpui::ListState::new(0, ListAlignment::Top, px(100.)),
            rows: vec![],
            row_profile: None,
            geometry: None,
            hint_width: px(0.),
            row_focus: CommandId::ALL
                .iter()
                .map(|command| (*command, cx.focus_handle()))
                .collect(),
            commands: {
                let mut commands = CommandId::ALL.to_vec();
                commands.sort_by_key(|command| command.label());
                commands
            },
            defaults: profiles::default_bindings(),
        }
    }
    pub(super) fn changed(&self) -> bool {
        self.drafts.values().any(profiles::Draft::changed) || !self.dirty.is_empty()
    }
    pub(super) fn edits(&self) -> Vec<profiles::Draft> {
        self.drafts
            .values()
            .filter(|draft| {
                draft.changed()
                    || self.active.as_ref().is_some_and(|active| {
                        self.drafts
                            .get(active)
                            .is_some_and(|selected| selected.path == draft.path)
                    })
            })
            .cloned()
            .collect()
    }
    pub(super) fn reset(&mut self) {
        self.serial += 1;
        self.loading = false;
        self.drafts.clear();
        self.active = None;
        self.inputs.clear();
        self.rows.clear();
        self.row_profile = None;
        self.scroll.reset(0);
        self.dirty.clear();
        self.errors.clear();
    }
}
impl SettingsPage {
    #[cfg(test)]
    pub(crate) fn keyboard_input_count(&self) -> usize {
        self.keyboard.inputs.len()
    }

    pub(crate) fn show_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.destination = Destination::Keyboard;
        self.defaults = false;
        if self.keyboard.active.is_none()
            && !self.keyboard.loading
            && let Some(saved) = &self.snapshot
        {
            self.load_keyboard_profile(saved.settings.keyboard.profile.clone(), window, cx);
        }
        window.focus(&self.keyboard.query.focus_handle(cx), cx);
        cx.notify();
    }
    pub(crate) fn load_keyboard_profile(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        if self.keyboard.drafts.contains_key(&name) {
            self.activate_profile(name, window, cx);
            return;
        }
        self.fetch_profile(name, window, cx);
    }
    fn fetch_profile(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.source.path().to_owned();
        let additional = self
            .snapshot
            .as_ref()
            .map(|s| s.settings.keyboard.additional_directories.clone())
            .unwrap_or_default();
        self.keyboard.loading = true;
        self.keyboard.serial += 1;
        let serial = self.keyboard.serial;
        cx.spawn_in(window, async move |page, cx| {
            let result = cx
                .background_spawn(async move {
                    let draft = profiles::Draft::load(&config, &name)?;
                    let names = profiles::list(&config, &name, &additional)?;
                    Ok::<_, String>((name, draft, names))
                })
                .await;
            let _ = page.update_in(cx, |page, window, cx| {
                if page.keyboard.serial != serial {
                    return;
                }
                page.keyboard.loading = false;
                match result {
                    Ok((name, draft, names)) => {
                        page.keyboard.names = names;
                        page.keyboard
                            .inputs
                            .retain(|(profile, _), _| profile != &name);
                        page.keyboard.drafts.insert(name.clone(), draft);
                        page.activate_profile(name, window, cx);
                    }
                    Err(error) => page.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn activate_profile(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.keyboard.active.replace(name.clone());
        let index = catalogue::SETTINGS
            .iter()
            .position(|s| s.key == "keyboard.profile")
            .expect("keyboard profile is documented");
        self.stage(index, Some(name.clone().into()), window, cx);
        // stage reports validation failures without replacing the working keymap.
        let accepted = self.value(index).and_then(toml::Value::as_str) == Some(name.as_str());
        if !accepted {
            self.keyboard.active = old;
        }
        cx.notify();
    }
    fn shortcut_inputs(
        &mut self,
        name: &str,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<InputState>> {
        let key = (name.to_owned(), command);
        if let Some(inputs) = self.keyboard.inputs.get(&key) {
            return inputs.clone();
        }
        let bindings = self.keyboard.drafts[name]
            .profile
            .keybindings
            .get(&command)
            .cloned()
            .unwrap_or_default();
        let mut inputs = Vec::new();
        for binding in bindings {
            inputs.push(self.new_shortcut_input(key.clone(), binding, window, cx));
        }
        while inputs.len() < 2 {
            inputs.push(self.new_shortcut_input(key.clone(), String::new(), window, cx));
        }
        self.keyboard.inputs.insert(key, inputs.clone());
        inputs
    }
    fn new_shortcut_input(
        &mut self,
        key: (String, CommandId),
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("Unbound");
            input.set_value(value, window, cx);
            input
        });
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |page, _, event, window, cx| match event {
                InputEvent::Change => {
                    page.keyboard.dirty.insert(key.clone());
                    page.keyboard.errors.remove(&key);
                    page.message = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    page.commit_shortcut(&key, window, cx);
                    cx.notify();
                }
                InputEvent::Focus => cx.notify(),
            },
        ));
        input
    }
    pub(super) fn commit_shortcut(
        &mut self,
        key: &(String, CommandId),
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() || !self.keyboard.dirty.contains(key) {
            return;
        }
        let bindings = self
            .keyboard
            .inputs
            .get(key)
            .into_iter()
            .flatten()
            .map(|input| input.read(cx).value().trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        self.remap_shortcut(key, Some(&bindings), cx);
    }
    fn remap_shortcut(
        &mut self,
        key: &(String, CommandId),
        bindings: Option<&[String]>,
        cx: &mut Context<Self>,
    ) {
        if bindings.is_some_and(|bindings| {
            self.keyboard
                .drafts
                .get(&key.0)
                .and_then(|draft| draft.profile.keybindings.get(&key.1))
                .is_some_and(|current| current == bindings)
        }) {
            self.keyboard.dirty.remove(key);
            self.keyboard.errors.remove(key);
            self.message = None;
            cx.notify();
            return;
        }
        let result = (|| {
            let draft = self
                .keyboard
                .drafts
                .get(&key.0)
                .ok_or("Profile is still loading")?
                .remap(key.1, bindings)?;
            let settings = self
                .snapshot
                .as_ref()
                .ok_or("Settings are still loading")?
                .settings
                .clone()
                .with_keybindings(draft.profile.clone())?;
            crate::viewer::commands::validate_bindings(&settings, cx)?;
            Ok::<_, String>(draft)
        })();
        match result {
            Ok(draft) => {
                let old = self
                    .keyboard
                    .drafts
                    .insert(key.0.clone(), draft)
                    .expect("profile was loaded");
                if let Err(error) = self.try_preview(cx) {
                    self.keyboard.drafts.insert(key.0.clone(), old);
                    self.keyboard.errors.insert(key.clone(), error);
                    return;
                }
                self.keyboard.dirty.remove(key);
                self.keyboard.errors.remove(key);
                self.message = None;
                if bindings.is_none() {
                    self.keyboard.inputs.remove(key);
                }
            }
            Err(error) => {
                self.keyboard.errors.insert(key.clone(), error);
            }
        }
        cx.notify();
    }
    pub(super) fn flush_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        for key in self.keyboard.dirty.clone() {
            self.commit_shortcut(&key, window, cx);
        }
        self.keyboard.dirty.is_empty() && self.keyboard.errors.is_empty() && !self.keyboard.loading
    }
    pub(super) fn reload_keyboard_if_clean(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.keyboard.loading || self.keyboard.changed() {
            return;
        }
        let Some(saved) = &self.persisted else {
            return;
        };
        // A staged profile selection belongs to this window, not an external reload.
        if self
            .pending
            .values()
            .any(|patch| patch.key == "keyboard.profile")
        {
            return;
        }
        if let Some(active) = &self.keyboard.active
            && (active != &saved.settings.keyboard.profile
                || self.keyboard.drafts.get(active).is_some_and(|draft| {
                    draft.profile.keybindings != saved.settings.keybindings
                        || draft.profile.keymap != saved.settings.keymap
                }))
        {
            self.fetch_profile(saved.settings.keyboard.profile.clone(), window, cx);
        }
    }
    fn edit_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self
            .keyboard
            .active
            .as_ref()
            .and_then(|name| self.keyboard.drafts.get(name))
        else {
            return;
        };
        let path = draft.path.clone();
        let editor = self
            .persisted
            .as_ref()
            .and_then(|s| s.settings.settings.editor.clone());
        cx.spawn_in(window, async move |page, cx| {
            let result = cx
                .background_spawn(
                    async move { crate::platform::open_text_file(&path, editor.as_ref()) },
                )
                .await;
            if let Err(error) = result {
                let _ = page.update(cx, |page, cx| {
                    page.message = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub(super) fn render_keyboard_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let surface = jaywalk::theme::Surface::Header;
        let background = crate::appearance::surface(surface, jaywalk::theme::Part::Background, cx);
        let foreground = crate::appearance::surface(surface, jaywalk::theme::Part::Foreground, cx);
        let muted = crate::appearance::surface(surface, jaywalk::theme::Part::MutedForeground, cx);
        let border = gpui::component::Theme::global(cx).border;
        let compact = f32::from(window.viewport_size().width) < 900.;
        let active = self.keyboard.active.clone();
        let owner = cx.entity().downgrade();
        let names = self.keyboard.names.clone();
        let profile_button = Button::new("keyboard-profile")
            .small()
            .dropdown_caret(true)
            .debug_selector(|| "keyboard-profile".into())
            .label(active.clone().unwrap_or_else(|| "Loading profile…".into()))
            .disabled(self.busy.is_some() || self.keyboard.loading)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for name in &names {
                    let name = name.clone();
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |page, cx| {
                                page.load_keyboard_profile(name.clone(), window, cx)
                            });
                        },
                    ));
                }
                menu
            });
        let edit_button = Button::new("keyboard-edit-toml")
            .debug_selector(|| "keyboard-edit-toml".into())
            .small()
            .border_1()
            .border_color(border)
            .label("Edit TOML…")
            .command_hint(CommandId::OpenKeybindings)
            .disabled(active.is_none())
            .on_click(cx.listener(|page, _, window, cx| page.edit_profile(window, cx)));
        let path = self.source.path().display().to_string();
        let pane_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
        let help_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
        let pane_measure = pane_bounds.clone();
        let help_measure = help_bounds.clone();
        const HELP: &str = concat!(
            "Use GPUI shortcuts: secondary = Cmd on macOS, Ctrl elsewhere. ",
            "Spaces separate chord steps. Empty fields unbind. ",
            "Toolkit action/context rules remain in TOML.\n\n",
            "Valid changes preview live. Save writes them; Discard restores saved settings.",
        );
        let help = div()
            .relative()
            .id("keyboard-help-trigger")
            .debug_selector(|| "keyboard-help-trigger".into())
            .on_prepaint(move |bounds, _, _| help_measure.set(bounds))
            .child(
                Button::new("keyboard-help")
                    .small()
                    .ghost()
                    .label("?")
                    .accessibility_label("Shortcut syntax and saving"),
            )
            .managed_tooltip_at(
                gpui::component::Placement::Bottom,
                move |window: &mut Window, cx: &mut App| {
                    let pane = pane_bounds.get();
                    let trigger = help_bounds.get();
                    let inset = crate::spacing::space(2., cx);
                    // Bottom tooltips are centered on their trigger. Bound the
                    // card by the measured content edge, including small fonts.
                    let width = px(340.)
                        .min(((trigger.center().x - pane.left() - inset) * 2.).max(px(1.)))
                        .min((pane.size.width - inset * 2.).max(px(1.)));
                    Tooltip::element(|_, _| {
                        div()
                            .debug_selector(|| "keyboard-help-content".into())
                            .whitespace_normal()
                            .child(HELP)
                    })
                    .w(width)
                    .whitespace_normal()
                    .build(window, cx)
                },
            );
        div()
            .debug_selector(|| "settings-header".into())
            .relative()
            .on_prepaint(move |bounds, _, _| pane_measure.set(bounds))
            .v_flex()
            .gap(crate::spacing::edge(cx))
            .p(crate::spacing::space(2., cx))
            .bg(background)
            .text_color(foreground)
            .border_b_1()
            .border_color(border)
            .child(
                super::header_row(window, cx)
                    .gap(crate::spacing::edge(cx))
                    .when(compact, |row| row.v_flex())
                    .when(!compact, |row| {
                        row.h_flex().items_center().justify_between()
                    })
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .min_w_0()
                            .gap(crate::spacing::interactive(cx))
                            .when(!compact, |row| row.flex_1())
                            .child(
                                div()
                                    .id("keyboard-heading")
                                    .debug_selector(|| "keyboard-heading".into())
                                    .min_w_0()
                                    .truncate()
                                    .text_lg()
                                    .font_medium()
                                    .child("Keyboard Shortcuts")
                                    .tooltip(move |window, cx| {
                                        gpui::component::tooltip::Tooltip::new(path.clone())
                                            .build(window, cx)
                                    }),
                            )
                            .child(help),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_end()
                            .when(compact, |row| row.w_full())
                            .gap(crate::spacing::edge(cx))
                            .child(profile_button)
                            .child(edit_button),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::edge(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .debug_selector(|| "keyboard-search".into())
                            .child(
                                Input::new(&self.keyboard.query)
                                    .small()
                                    .border_color(border),
                            ),
                    )
                    .when_some(
                        self.error.clone().or_else(|| self.message.clone()),
                        |row, message| {
                            row.child(
                                div()
                                    .id("settings-save-status")
                                    .max_w(px(160.))
                                    .text_xs()
                                    .text_color(muted)
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
            .into_any_element()
    }
    fn remove_shortcut(
        &mut self,
        key: &(String, CommandId),
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() || self.keyboard.loading {
            return;
        }
        let Some(inputs) = self.keyboard.inputs.get(key) else {
            return;
        };
        if index >= inputs.len() {
            return;
        }
        let mut remaining = inputs.clone();
        // Clearing one slot must not move the other slot under the pointer.
        remaining[index] = self.new_shortcut_input(key.clone(), String::new(), window, cx);
        let bindings = remaining
            .iter()
            .map(|input| input.read(cx).value().trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        self.remap_shortcut(key, Some(&bindings), cx);
        if !self.keyboard.errors.contains_key(key) {
            self.keyboard.inputs.insert(key.clone(), remaining);
            window.focus(&self.surface_focus, cx);
        }
        cx.notify();
    }
    fn shortcut_field(
        &self,
        command: CommandId,
        index: usize,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = input.read(cx).value();
        let focused = input.focus_handle(cx).is_focused(window);
        let parsed = jaywalk::keymap::strokes(&value);
        let show_keys = !focused && !value.trim().is_empty() && parsed.is_ok();
        let disabled = self.busy.is_some() || self.keyboard.loading;
        let focus = input.focus_handle(cx);
        let mut field = div()
            .relative()
            .flex_1()
            .min_w_0()
            .debug_selector(move || format!("keyboard-{}-{index}", command.key()))
            // Keep the real input laid out for tab focus, selection and unchanged
            // field geometry. Only its idle presentation changes to keycaps.
            .child(
                Input::new(&input)
                    .small()
                    .disabled(disabled)
                    .when(show_keys, |input| input.opacity(0.)),
            );
        if show_keys && let Ok(strokes) = parsed {
            let caps = div()
                .h_flex()
                .w_full()
                .min_w_0()
                .justify_start()
                .items_center()
                .gap(crate::spacing::edge(cx))
                .children(strokes.iter().map(|stroke| {
                    div().h_flex().gap(px(2.)).children(
                        keys::parts(stroke)
                            .into_iter()
                            .map(|key| Kbd::new(key).text_sm()),
                    )
                }));
            field = field.child(
                Button::new(SharedString::from(format!(
                    "shortcut-display-{}-{index}",
                    command.key()
                )))
                .absolute()
                .inset_0()
                .small()
                .ghost()
                .border_1()
                .border_color(gpui::component::Theme::global(cx).input)
                .bg(gpui::component::Theme::global(cx).input_background())
                .px(crate::spacing::interactive(cx))
                .py_0()
                .justify_start()
                .overflow_hidden()
                .accessibility_label(format!("Edit shortcut: {value}"))
                .child(caps)
                .disabled(disabled)
                .on_click(cx.listener(move |_, _, window, cx| {
                    window.focus(&focus, cx);
                    cx.notify();
                })),
            );
        }
        field.into_any_element()
    }
    fn render_keyboard_row(
        &mut self,
        name: &str,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = gpui::component::Theme::global(cx).border;
        let bindings = &self.keyboard.drafts[name].profile.keybindings[&command];
        let changed = self.keyboard.defaults.get(&command) != Some(bindings);
        let inputs = self.shortcut_inputs(name, command, window, cx);
        let key = (name.to_owned(), command);
        let mut controls = div().v_flex().gap(crate::spacing::interactive(cx));
        // Two fixed slots are enough for ordinary editing. Existing extra
        // alternatives remain editable; never truncate a user's profile.
        for (pair, inputs) in inputs.chunks(2).enumerate() {
            let mut slots = div().h_flex().gap(crate::spacing::edge(cx));
            for (slot, input) in inputs.iter().enumerate() {
                let index = pair * 2 + slot;
                let remove_key = key.clone();
                let removable = !input.read(cx).value().trim().is_empty();
                slots = slots.child(
                    div()
                        .v_flex()
                        .w(px(172.))
                        .gap(crate::spacing::interactive(cx))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap(crate::spacing::interactive(cx))
                                .child(self.shortcut_field(
                                    command,
                                    index,
                                    input.clone(),
                                    window,
                                    cx,
                                ))
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "shortcut-remove-{}-{index}",
                                        command.key()
                                    )))
                                    .debug_selector(move || {
                                        format!("shortcut-remove-{}-{index}", command.key())
                                    })
                                    .compact()
                                    .ghost()
                                    .icon(gpui::assets::IconName::X)
                                    .when(!removable, |button| button.invisible())
                                    .tooltip("Clear shortcut")
                                    .disabled(
                                        !removable || self.busy.is_some() || self.keyboard.loading,
                                    )
                                    .on_click(cx.listener(move |page, _, window, cx| {
                                        page.remove_shortcut(&remove_key, index, window, cx)
                                    })),
                                ),
                        ),
                );
            }
            controls = controls.child(slots);
        }
        let reset_key = key.clone();
        let actions =
            div()
                .h_flex()
                .items_start()
                .gap(crate::spacing::edge(cx))
                .child(controls.w(px(352.)))
                .child(
                    div().v_flex().gap(crate::spacing::interactive(cx)).child(
                        Button::new(SharedString::from(format!(
                            "shortcut-reset-{}",
                            command.key()
                        )))
                        .compact()
                        .icon(gpui::component::IconName::Undo2)
                        .self_start()
                        .mt(crate::spacing::interactive(cx))
                        .tooltip("Reset to default")
                        .disabled(!changed || self.busy.is_some())
                        .on_click(cx.listener(move |page, _, _, cx| {
                            page.remap_shortcut(&reset_key, None, cx)
                        })),
                    ),
                );
        let row = div()
            .h_flex()
            .w_full()
            .items_center()
            .gap(crate::spacing::edge(cx))
            .py(crate::spacing::edge(cx))
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .id(SharedString::from(format!(
                        "keyboard-command-{}",
                        command.key()
                    )))
                    .child(command.label())
                    .tooltip(move |window, cx| Tooltip::new(command.key()).build(window, cx)),
            )
            .child(actions);
        div()
            .v_flex()
            .w_full()
            .track_focus(&self.keyboard.row_focus[&command])
            .px(crate::spacing::space(3., cx))
            .pb(crate::spacing::edge(cx))
            .child(row)
            .when_some(self.keyboard.errors.get(&key), |row, error| {
                row.child(
                    div()
                        .text_sm()
                        .text_color(gpui::component::Theme::global(cx).danger)
                        .child(error.clone()),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_keyboard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.keyboard.active.clone();
        let query = self.keyboard.query.read(cx).value().to_lowercase();
        let rows = active.as_ref().map_or_else(Vec::new, |name| {
            self.keyboard
                .commands
                .iter()
                .copied()
                .filter_map(|command| {
                    let bindings = &self.keyboard.drafts[name].profile.keybindings[&command];
                    keys::matches(
                        command.label(),
                        &command.key(),
                        bindings,
                        &query,
                        jaywalk::keymap::native_os(),
                    )
                    .then(|| {
                        let key = (name.clone(), command);
                        KeyboardRow {
                            command,
                            slots: self
                                .keyboard
                                .inputs
                                .get(&key)
                                .map_or(bindings.len().max(2), Vec::len),
                            error: self.keyboard.errors.get(&key).cloned(),
                        }
                    })
                })
                .collect::<Vec<_>>()
        });
        let changed_commands = self.keyboard.row_profile != active
            || self
                .keyboard
                .rows
                .iter()
                .map(|row| row.command)
                .ne(rows.iter().map(|row| row.command));
        if changed_commands {
            self.keyboard.scroll.reset(0);
            self.keyboard.scroll.splice_focusable(
                0..0,
                rows.iter()
                    .map(|row| Some(self.keyboard.row_focus[&row.command].clone())),
            );
        } else {
            for (index, (old, new)) in self.keyboard.rows.iter().zip(&rows).enumerate() {
                if old != new {
                    self.keyboard.scroll.remeasure_items(index..index + 1);
                }
            }
        }
        let theme = gpui::component::Theme::global(cx);
        let geometry = RowGeometry {
            font: theme.font_family.clone(),
            size: theme.font_size,
            edge: crate::spacing::edge(cx),
            interactive: crate::spacing::interactive(cx),
        };
        let geometry_changed = self.keyboard.geometry.as_ref() != Some(&geometry);
        if geometry_changed {
            self.keyboard.scroll.remeasure_items(0..rows.len());
            self.keyboard.geometry = Some(geometry);
        }
        let width = self.keyboard.scroll.viewport_bounds().size.width;
        if changed_commands || geometry_changed || self.keyboard.hint_width != width {
            self.keyboard.hint_width = width;
            let owner = cx.entity().downgrade();
            // GPUI discards size hints when it first establishes the viewport
            // width (and on resize). Seed unmeasured rows after that layout so
            // the scrollbar represents the whole list without measuring it all.
            window.on_next_frame(move |_, cx| {
                let _ = owner.update(cx, |page, cx| {
                    page.keyboard
                        .scroll
                        .clone()
                        .with_uniform_item_height(px(80.));
                    cx.notify();
                });
            });
        }
        self.keyboard.rows = rows;
        self.keyboard.row_profile = active.clone();
        let owner = cx.entity().downgrade();
        let rows = list(self.keyboard.scroll.clone(), move |index, window, cx| {
            owner
                .update(cx, |page, cx| {
                    let command = page.keyboard.rows[index].command;
                    page.render_keyboard_row(
                        active
                            .as_deref()
                            .expect("keyboard rows require an active profile"),
                        command,
                        window,
                        cx,
                    )
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        let muted = gpui::component::Theme::global(cx).muted_foreground;
        let headings = div()
            .h_flex()
            .items_center()
            .px(crate::spacing::space(3., cx))
            .gap(crate::spacing::edge(cx))
            .text_xs()
            .text_color(muted)
            .child(div().flex_1().child("Command"))
            .child(
                div()
                    .h_flex()
                    .gap(crate::spacing::edge(cx))
                    .child(
                        div()
                            .w(px(172.))
                            .debug_selector(|| "keyboard-primary-heading".into())
                            .child("Primary"),
                    )
                    .child(div().w(px(172.)).child("Alternate")),
            )
            .child(
                Button::new("keyboard-reset-spacer")
                    .compact()
                    .icon(gpui::component::IconName::Undo2)
                    .invisible(),
            );
        div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .child(headings)
            .child(
                div()
                    .relative()
                    .debug_selector(|| "keyboard-content".into())
                    .flex_1()
                    .min_h_0()
                    // Keep fixed viewport breathing room outside the scroll/hit-test area.
                    .py(crate::spacing::edge(cx))
                    .child(rows)
                    .child(Scrollbar::vertical(&self.keyboard.scroll)),
            )
            .into_any_element()
    }
}
