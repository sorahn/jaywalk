//! Application-owned configuration sources. Windows subscribe; closing one does
//! not cancel watching or let an invalid reload replace shared application state.
use crate::viewer::commands;
use gpui::*;
use jaywalk::settings::Settings;
use jaywalk::settings_reload::{SettingsSource, SettingsWatcher};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
struct SettingsService {
    sources: HashMap<SettingsSource, Entity<SourceState>>,
    active: Option<Arc<Settings>>,
}
impl Global for SettingsService {}

pub(crate) struct SourceState {
    pub(crate) current: Arc<Settings>,
    pub(crate) error: Option<String>,
    task: Option<Task<()>>,
}
pub(crate) fn init(cx: &mut App) {
    cx.set_global(SettingsService::default());
}

/// Explicit opens and successful edits retain the existing last-applied-source
/// policy. Window-specific layout and reading state are not stored here.
pub(crate) fn publish(
    source: SettingsSource,
    settings: Settings,
    cx: &mut App,
) -> Result<Entity<SourceState>, String> {
    apply_shared(&settings, cx)?;
    if let Some(state) = cx.global::<SettingsService>().sources.get(&source).cloned() {
        state.update(cx, |state, cx| {
            if *state.current != settings || state.error.is_some() {
                state.current = Arc::new(settings);
                state.error = None;
                cx.notify();
            }
        });
        return Ok(state);
    }
    let key = source.clone();
    let state = cx.new(|cx: &mut Context<SourceState>| {
        let mut state = SourceState { current: Arc::new(settings), error: None, task: None };
        state.task = Some(cx.spawn(async move |this, cx| {
            let mut watcher = SettingsWatcher::new(source);
            loop {
                let Ok(interval) = this.update(cx, |state, _| state.current.settings.poll_interval_ms) else { break; };
                cx.background_executor().timer(Duration::from_millis(interval)).await;
                let (next, result) = cx.background_spawn(async move { let result = watcher.poll(); (watcher, result) }).await;
                watcher = next;
                if let Some(result) = result {
                    let _ = this.update(cx, |state, cx| {
                        if result.as_ref().is_ok_and(|settings| *state.current == *settings) && state.error.is_none() { return; }
                        let result = result.and_then(|settings| { apply_shared(&settings, cx)?; Ok(settings) });
                        match result {
                            Ok(settings) => {
                                if *state.current == settings && state.error.is_none() { return; }
                                state.current = Arc::new(settings);
                                state.error = None;
                            }
                            Err(error) => state.error = Some(format!("Settings not applied; keeping the last valid configuration. {error}")),
                        }
                        cx.notify();
                    });
                }
            }
        }));
        state
    });
    cx.global_mut::<SettingsService>()
        .sources
        .insert(key, state.clone());
    Ok(state)
}

pub(crate) fn source(
    source: SettingsSource,
    settings: &Settings,
    cx: &mut App,
) -> Result<Entity<SourceState>, String> {
    if let Some(state) = cx.global::<SettingsService>().sources.get(&source) {
        return Ok(state.clone());
    }
    publish(source, settings.clone(), cx)
}

fn apply_shared(settings: &Settings, cx: &mut App) -> Result<(), String> {
    // Validate/install the keymap before mutating any other shared state.
    let previous = cx.global::<SettingsService>().active.clone();
    if previous
        .as_ref()
        .is_none_or(|old| old.keybindings != settings.keybindings || old.keymap != settings.keymap)
    {
        commands::install_bindings(settings, cx)?;
    }
    if previous
        .as_ref()
        .is_none_or(|old| old.application != settings.application)
    {
        crate::windows::apply_application_settings(&settings.application, cx);
    }
    if previous
        .as_ref()
        .is_none_or(|old| old.ui != settings.ui || old.editor != settings.editor)
    {
        gpui::component::Theme::update(cx, |theme| {
            crate::appearance::apply_accent(theme, settings.ui.accent_color);
            theme.font_size = px(settings.ui.font_size);
            theme.mono_font_size = px(settings.editor.font_size);
            theme.font_family = settings
                .ui
                .font_family
                .clone()
                .map(SharedString::from)
                .unwrap_or_else(|| gpui::component::Theme::default().font_family);
            theme.mono_font_family = settings
                .editor
                .font_family
                .clone()
                .map(SharedString::from)
                .unwrap_or_else(|| gpui::component::Theme::default().mono_font_family);
        });
    }
    cx.global_mut::<SettingsService>().active = Some(Arc::new(settings.clone()));
    Ok(())
}
