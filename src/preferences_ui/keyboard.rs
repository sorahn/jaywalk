//! Application command remapping. Toolkit action/context rules remain in TOML.
use super::*;
use jaywalk::keybinding_profiles as profiles;
use std::collections::BTreeSet;

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
    scroll: ScrollHandle,
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
            scroll: ScrollHandle::new(),
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
        self.dirty.clear();
        self.errors.clear();
    }
}
impl SettingsPage {
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
        for binding in bindings.into_iter().chain(std::iter::once(String::new())) {
            // One empty field for an unbound command, otherwise only its bindings.
            if binding.is_empty() && !inputs.is_empty() {
                break;
            }
            inputs.push(self.new_shortcut_input(key.clone(), binding, window, cx));
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
                }
                _ => {}
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
    pub(super) fn render_keyboard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = gpui::component::Theme::global(cx).muted_foreground;
        let border = gpui::component::Theme::global(cx).border;
        let active = self.keyboard.active.clone();
        let owner = cx.entity().downgrade();
        let names = self.keyboard.names.clone();
        let default_bindings = profiles::default_bindings();
        let profile_button = Button::new("keyboard-profile")
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
            .label("Edit profile TOML…")
            .command_hint(CommandId::OpenKeybindings)
            .disabled(active.is_none())
            .on_click(cx.listener(|page, _, window, cx| page.edit_profile(window, cx)));
        let mut content=div().v_flex().gap(crate::spacing::edge(cx)).p(crate::spacing::space(4.,cx))
            .child(div().h_flex().items_center().justify_between().gap(crate::spacing::edge(cx))
                .child(profile_button).child(edit_button))
            .child(div().text_sm().text_color(muted)
                .child("Use GPUI shortcuts: secondary = Cmd/Ctrl; spaces separate chord steps. Empty fields unbind. Toolkit action/context rules remain in TOML."))
            .child(div().debug_selector(||"keyboard-search".into())
                .child(Input::new(&self.keyboard.query).small()));
        if let Some(name) = active {
            let query = self.keyboard.query.read(cx).value().to_lowercase();
            let mut commands = CommandId::ALL.to_vec();
            commands.sort_by_key(|command| command.label());
            for command in commands {
                let bindings = &self.keyboard.drafts[&name].profile.keybindings[&command];
                if !format!(
                    "{} {} {}",
                    command.label(),
                    command.key(),
                    bindings.join(" ")
                )
                .to_lowercase()
                .contains(query.trim())
                {
                    continue;
                }
                let changed = default_bindings.get(&command) != Some(bindings);
                let inputs = self.shortcut_inputs(&name, command, window, cx);
                let key = (name.clone(), command);
                let mut controls = div().v_flex().gap(crate::spacing::interactive(cx));
                for (index, input) in inputs.into_iter().enumerate() {
                    controls = controls.child(
                        div()
                            .debug_selector(move || format!("keyboard-{}-{index}", command.key()))
                            .child(
                                Input::new(&input)
                                    .small()
                                    .disabled(self.busy.is_some() || self.keyboard.loading),
                            ),
                    );
                }
                let add_key = key.clone();
                let reset_key = key.clone();
                let row = div()
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::edge(cx))
                    .py(crate::spacing::edge(cx))
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .v_flex()
                            .child(command.label())
                            .child(div().text_xs().text_color(muted).child(command.key())),
                    )
                    .child(controls.w(px(240.)))
                    .child(
                        Button::new(SharedString::from(format!(
                            "shortcut-add-{}",
                            command.key()
                        )))
                        .compact()
                        .icon(gpui::component::IconName::Plus)
                        .tooltip("Add shortcut")
                        .disabled(self.busy.is_some() || self.keyboard.loading)
                        .on_click(cx.listener(
                            move |page, _, window, cx| {
                                let input = page.new_shortcut_input(
                                    add_key.clone(),
                                    String::new(),
                                    window,
                                    cx,
                                );
                                page.keyboard
                                    .inputs
                                    .entry(add_key.clone())
                                    .or_default()
                                    .push(input.clone());
                                window.focus(&input.focus_handle(cx), cx);
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "shortcut-reset-{}",
                            command.key()
                        )))
                        .compact()
                        .icon(gpui::component::IconName::Undo2)
                        .tooltip("Reset to default")
                        .disabled(!changed || self.busy.is_some())
                        .on_click(cx.listener(move |page, _, _, cx| {
                            page.remap_shortcut(&reset_key, None, cx)
                        })),
                    );
                content = content.child(div().v_flex().child(row).when_some(
                    self.keyboard.errors.get(&key),
                    |row, error| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(gpui::component::Theme::global(cx).danger)
                                .child(error.clone()),
                        )
                    },
                ));
            }
        }
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("keyboard-content")
                    .debug_selector(|| "keyboard-content".into())
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.keyboard.scroll)
                    .child(content),
            )
            .child(Scrollbar::vertical(&self.keyboard.scroll))
            .into_any_element()
    }
}
