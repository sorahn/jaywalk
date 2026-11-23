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
    pub(crate) snapshot: Option<Arc<jaywalk::preferences::Snapshot>>,
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
    if let Some(state) = cx.global::<SettingsService>().sources.get(&source).cloned()
        && let Some(snapshot) = state.read(cx).snapshot.clone()
    {
        apply_shared(&snapshot.settings, &source, cx)?;
        return Ok(state);
    }
    apply_shared(&settings, &source, cx)?;
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
        let mut state = SourceState { current: Arc::new(settings), error: None, snapshot: None, task: None };
        state.task = Some(cx.spawn(async move |this, cx| {
            let mut watcher = SettingsWatcher::new(source.clone());
            let mut sync_failed = false;
            loop {
                let Ok(interval) = this.update(cx, |state, _| state.current.settings.poll_interval_ms) else { break; };
                cx.background_executor().timer(Duration::from_millis(interval)).await;
                let backend = cx.update(|cx| cx.try_global::<jaywalk::accounts::application::Accounts>().and_then(|accounts| accounts.backend.as_ref().ok()).cloned());
                let starting_revision = this.update(cx,|state,_|state.snapshot.as_ref().map_or(0,|snapshot|snapshot.revision)).unwrap_or_default();
                let worker_source = source.clone();
                let (next, result, failed) = cx.background_spawn(async move {
                    let sync = backend.map_or(Ok(()), |backend| backend.synchronize_account_config(worker_source.path()));
                    let failed = sync.is_err();
                    let result = match sync {
                        Err(error) => { let _ = watcher.poll_snapshot(); Some(Err(error)) },
                        Ok(()) => watcher.poll_snapshot().or_else(|| sync_failed.then(|| worker_source.read_snapshot())),
                    };
                    (watcher, result, failed)
                }).await;
                watcher = next;
                sync_failed = failed;
                if let Some(result) = result {
                    let _ = this.update(cx, |state, cx| {
                        if result.is_err() && state.snapshot.as_ref().is_some_and(|snapshot|snapshot.revision>starting_revision) {return;}
                        let result = result.and_then(|snapshot| {
                            if state.snapshot.as_ref().is_some_and(|current| current.revision > snapshot.revision) { return Ok(None); }
                            apply_shared(&snapshot.settings, &source, cx)?;
                            Ok(Some(snapshot))
                        });
                        match result {
                            Ok(Some(snapshot)) => {
                                state.current = Arc::new(snapshot.settings.clone());
                                state.snapshot = Some(Arc::new(snapshot));
                                state.error = None;
                            }
                            Ok(None) => return,
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

/// Publish a committed document once; stale workers cannot replace newer edits.
pub(crate) fn publish_snapshot(
    source: SettingsSource,
    snapshot: jaywalk::preferences::Snapshot,
    cx: &mut App,
) -> Result<Entity<SourceState>, String> {
    let state = self::source(source.clone(), &snapshot.settings, cx)?;
    if state
        .read(cx)
        .snapshot
        .as_ref()
        .is_some_and(|current| current.revision > snapshot.revision)
    {
        return Ok(state);
    }
    apply_shared(&snapshot.settings, &source, cx)?;
    state.update(cx, |state, cx| {
        state.current = Arc::new(snapshot.settings.clone());
        state.snapshot = Some(Arc::new(snapshot));
        state.error = None;
        cx.notify();
    });
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

fn apply_shared(settings: &Settings, source: &SettingsSource, cx: &mut App) -> Result<(), String> {
    // Validate/install the keymap before mutating any other shared state.
    let previous = cx.global::<SettingsService>().active.clone();
    if previous
        .as_ref()
        .is_none_or(|old| old.keybindings != settings.keybindings || old.keymap != settings.keymap)
    {
        commands::install_bindings(settings, cx)?;
    }
    crate::updater::configure(settings, cx);
    if previous
        .as_ref()
        .is_none_or(|old| old.application != settings.application)
    {
        crate::windows::apply_application_settings(&settings.application, cx);
    }
    if previous.as_ref().is_none_or(|old| {
        old.theme != settings.theme
            || old.ui != settings.ui
            || old.editor != settings.editor
            || old.diff.colors != settings.diff.colors
    }) {
        crate::appearance::apply(settings, cx);
    }
    if let Some(backend) = cx
        .try_global::<jaywalk::accounts::application::Accounts>()
        .and_then(|accounts| accounts.backend.as_ref().ok())
    {
        backend.configure_accounts(
            source.path().to_path_buf(),
            settings.accounts.clone(),
            settings.hosting.concurrent_accounts,
        )?;
    }
    cx.global_mut::<SettingsService>().active = Some(Arc::new(settings.clone()));
    Ok(())
}
