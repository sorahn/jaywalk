//! Window-local intents; only Save publishes them to the shared configuration.
use super::*;

pub(super) fn ordered_patches(pending: &BTreeMap<usize, Patch>) -> Vec<Patch> {
    let mut patches: Vec<_> = pending.values().cloned().collect();
    patches.sort_by_key(|patch| patch.key.split('.').count());
    patches
}

#[derive(IntoElement)]
pub(super) struct SaveHint;
impl RenderOnce for SaveHint {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        div().children(gpui::component::kbd::Kbd::binding_for_action(
            &SaveDraft,
            Some("JaywalkSettings"),
            window,
        ))
    }
}

impl SettingsPage {
    pub(crate) fn has_changes(&self) -> bool {
        !self.pending.is_empty()
            || self.drafts.values().any(|draft| draft.dirty)
            || self.keyboard.changed()
    }

    pub(super) fn try_preview(&self, cx: &mut Context<Self>) -> Result<(), String> {
        let mut patches = self.pending.clone();
        patches.extend(self.raw_preview.clone());
        crate::settings_service::preview(
            &self.source,
            cx.entity_id(),
            ordered_patches(&patches),
            self.keyboard.active.as_ref().and_then(|name| {
                self.keyboard
                    .drafts
                    .get(name)
                    .filter(|draft| {
                        draft.changed()
                            || self
                                .persisted
                                .as_ref()
                                .is_some_and(|saved| saved.settings.keyboard.profile != *name)
                    })
                    .map(|draft| draft.profile.clone())
            }),
            cx,
        )
    }
    pub(super) fn preview_input(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(draft) = self.drafts.get(&index) else {
            return;
        };
        if !draft.dirty {
            self.raw_preview.remove(&index);
            self.refresh_preview(cx);
            return;
        }
        if draft.error.is_some() {
            return;
        }
        let result = controls::color_edit(
            index,
            self.value(index),
            draft.input.read(cx).value().as_ref(),
            draft.color_dark,
        );
        if let Ok(value) = result {
            let previous = self
                .raw_preview
                .insert(index, Patch::set(&catalogue::SETTINGS[index].key, value));
            if let Err(error) = self.try_preview(cx) {
                if let Some(previous) = previous {
                    self.raw_preview.insert(index, previous);
                } else {
                    self.raw_preview.remove(&index);
                }
                self.message = Some(error);
            } else {
                self.message = None;
            }
        }
    }
    pub(super) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.try_preview(cx) {
            self.message = Some(error);
        }
    }

    pub(super) fn stage(
        &mut self,
        index: usize,
        value: Option<toml::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() || self.error.is_some() || self.defaults {
            return;
        }
        let Some(saved) = &self.persisted else {
            return;
        };
        let key = catalogue::SETTINGS[index].key.clone();
        if key.starts_with("theme.code.") {
            self.theme_prompt_serial += 1;
        }
        let mut pending = self.pending.clone();
        // A section reset supersedes earlier edits within that section. Later
        // child edits apply after the section reset, even if equal to the old file.
        pending.retain(|_, patch| !patch.key.starts_with(&format!("{key}.")));
        let parent_reset = pending
            .values()
            .any(|patch| key.starts_with(&format!("{}.", patch.key)));
        let unchanged = !parent_reset
            && (value.as_ref() == saved.explicit(&key)
                || (value.is_some() && value.as_ref() == saved.value(&key)));
        if unchanged {
            pending.remove(&index);
        } else {
            let expected = self
                .pending
                .get(&index)
                .map(|patch| patch.expected.clone())
                .unwrap_or_else(|| {
                    Expect::Explicit(
                        self.drafts
                            .get(&index)
                            .filter(|draft| draft.dirty)
                            .map(|draft| draft.baseline.clone())
                            .unwrap_or_else(|| saved.explicit(&key).cloned()),
                    )
                });
            pending.insert(
                index,
                Patch {
                    key: key.clone(),
                    value,
                    expected,
                },
            );
        }
        // Make implied consent revocations explicit in the draft and its conflict checks.
        let revoking = pending.get(&index).is_some_and(|patch| {
            patch
                .value
                .as_ref()
                .is_none_or(|value| value.as_bool() == Some(false))
        });
        if revoking
            && matches!(
                key.as_str(),
                "updates.automatic_checks" | "updates.automatic_downloads"
            )
        {
            for dependent in [
                "updates.automatic_downloads",
                "updates.automatic_installation",
            ] {
                if dependent == key {
                    continue;
                }
                let dependent_index = catalogue::SETTINGS
                    .iter()
                    .position(|setting| setting.key == dependent)
                    .expect("consent setting exists");
                pending.insert(
                    dependent_index,
                    Patch {
                        key: dependent.into(),
                        value: Some(false.into()),
                        expected: self
                            .pending
                            .get(&dependent_index)
                            .map(|patch| patch.expected.clone())
                            .unwrap_or_else(|| {
                                Expect::Explicit(saved.explicit(dependent).cloned())
                            }),
                    },
                );
            }
        }
        match saved.preview(&ordered_patches(&pending)) {
            Ok(_) => {
                let raw_preview = self.raw_preview.remove(&index);
                let was_dirty = self.drafts.get(&index).is_some_and(|draft| draft.dirty);
                if let Some(draft) = self.drafts.get_mut(&index) {
                    draft.dirty = false;
                }
                let old = std::mem::replace(&mut self.pending, pending);
                if let Err(error) = self.try_preview(cx) {
                    self.pending = old;
                    if let Some(patch) = raw_preview {
                        self.raw_preview.insert(index, patch);
                    }
                    if let Some(draft) = self.drafts.get_mut(&index) {
                        draft.dirty = was_dirty;
                    }
                    self.message = Some(error);
                    cx.notify();
                    return;
                }
                if let Some(draft) = self.drafts.get_mut(&index) {
                    draft.dirty = false;
                    draft.error = None;
                }
                self.message = None;
                self.accept(saved.clone(), window, cx);
            }
            Err(error) => {
                self.message = Some(error.clone());
                if let Some(draft) = self.drafts.get_mut(&index) {
                    draft.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    pub(super) fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_prompt_serial += 1;
        self.structured.prompt_serial += 1;
        if self.busy.is_some() {
            return;
        }
        if let Err(error) = crate::settings_service::clear_preview(&self.source, cx.entity_id(), cx)
        {
            self.message = Some(error);
            cx.notify();
            return;
        }
        self.pending.clear();
        self.raw_preview.clear();
        self.keyboard.reset();
        for draft in self.drafts.values_mut() {
            draft.dirty = false;
            draft.error = None;
        }
        self.message = None;
        if let Some(saved) = self.persisted.clone() {
            self.accept(saved, window, cx);
        }
        if self.destination == Destination::Keyboard {
            self.show_keyboard(window, cx);
        }
    }

    pub(super) fn save_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() || self.error.is_some() {
            return;
        }
        self.structured.prompt_serial += 1;
        // Save includes the active text field even if it has not blurred yet.
        let defaults = self.defaults;
        self.defaults = false;
        let dirty: Vec<_> = self
            .drafts
            .iter()
            .filter(|(_, draft)| draft.dirty)
            .map(|(index, _)| *index)
            .collect();
        for index in dirty {
            self.commit_draft(index, window, cx);
        }
        self.defaults = defaults;
        let shortcuts_valid = self.flush_shortcuts(window, cx);
        if !shortcuts_valid
            || self
                .drafts
                .values()
                .any(|draft| draft.dirty || draft.error.is_some())
        {
            self.close_after_save = None;
            self.message = Some("Fix invalid or conflicting fields before saving.".into());
            cx.notify();
            return;
        }
        if self.pending.is_empty() && !self.keyboard.changed() {
            if let Some(quit) = self.close_after_save.take() {
                cx.emit(Event::Close { quit });
            }
            return;
        }
        let profiles = self.keyboard.edits();
        let patches = ordered_patches(&self.pending);
        let source = self.source.clone();
        self.busy = Some(usize::MAX);
        self.message = Some("Saving…".into());
        let (done, finished) = async_channel::bounded(1);
        let quit_guard = App::on_app_quit(cx, move |_| {
            let finished = finished.clone();
            async move {
                let _ = finished.recv().await;
            }
        });
        let work = cx.background_spawn(async move {
            let result = preferences::save(&source, &patches, &profiles);
            let _ = done.try_send(());
            result
        });
        cx.spawn_in(window, async move |page, cx| {
            let result = work.await;
            drop(quit_guard);
            let _ = page.update_in(cx, |page, window, cx| {
                page.busy = None;
                match result {
                    Ok(snapshot) => {
                        page.pending.clear();
                        let keyboard_active = page.destination == Destination::Keyboard;
                        page.keyboard.reset();
                        match crate::settings_service::publish_snapshot(
                            page.source.clone(),
                            snapshot,
                            cx,
                        ) {
                            Ok(state) => {
                                if let Some(snapshot) = state.read(cx).snapshot.clone() {
                                    page.accept(snapshot, window, cx);
                                }
                                if let Err(error) = crate::settings_service::clear_preview(
                                    &page.source,
                                    cx.entity_id(),
                                    cx,
                                ) {
                                    page.message = Some(error);
                                    page.close_after_save = None;
                                    return;
                                }
                                if keyboard_active {
                                    page.show_keyboard(window, cx);
                                }
                                page.message = Some("Saved".into());
                                if let Some(quit) = page.close_after_save.take() {
                                    cx.emit(Event::Close { quit });
                                }
                            }
                            Err(error) => {
                                page.message = Some(error);
                                page.close_after_save = None;
                            }
                        }
                    }
                    Err(error) => {
                        page.message = Some(error);
                        page.close_after_save = None;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn request_close(
        &mut self,
        quit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            self.close_after_save = Some(quit);
            return;
        }
        if !self.has_changes() {
            cx.emit(Event::Close { quit });
            return;
        }
        if self.close_prompt {
            return;
        }
        self.close_prompt = true;
        let response = window.prompt(
            PromptLevel::Warning,
            "Save changes to Jaywalk settings?",
            Some("Your changes have not been saved."),
            &["Save", "Discard", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |page, cx| {
            let answer = response.await;
            let _ = page.update_in(cx, |page, window, cx| {
                page.close_prompt = false;
                match answer {
                    Ok(0) => {
                        page.close_after_save = Some(quit);
                        page.save_all(window, cx);
                    }
                    Ok(1) => {
                        page.discard(window, cx);
                        if !page.has_changes() {
                            cx.emit(Event::Close { quit });
                        }
                    }
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::ordered_patches;
    use jaywalk::preferences::Patch;
    use std::collections::BTreeMap;
    #[test]
    fn section_patches_precede_child_patches_independently_of_catalogue_order() {
        let pending = BTreeMap::from([
            (
                1,
                Patch::set("diff.colors.added_background", "#123456".into()),
            ),
            (
                2,
                Patch {
                    key: "diff.colors".into(),
                    value: None,
                    expected: jaywalk::preferences::Expect::Any,
                },
            ),
        ]);
        let patches = ordered_patches(&pending);
        assert_eq!(patches[0].key, "diff.colors");
        assert_eq!(patches[1].key, "diff.colors.added_background");
    }
}
