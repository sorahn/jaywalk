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
    active_source: Option<SettingsSource>,
}
impl Global for SettingsService {}

#[derive(Clone, PartialEq)]
struct Preview {
    owner: EntityId,
    patches: Vec<jaywalk::preferences::Patch>,
    profile: Option<jaywalk::keybinding_profiles::Profile>,
}

pub(crate) struct SourceState {
    pub(crate) current: Arc<Settings>,
    pub(crate) error: Option<String>,
    pub(crate) snapshot: Option<Arc<jaywalk::preferences::Snapshot>>,
    preview: Option<Preview>,
    pub(crate) transient: bool,
    task: Option<Task<()>>,
}
impl SourceState {
    pub(crate) fn previewing(&self) -> bool {
        self.preview.is_some()
    }
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
        && state.read(cx).snapshot.is_some()
    {
        let current = state.read(cx).current.clone();
        apply_shared(&current, &source, cx)?;
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
        let mut state = SourceState { current: Arc::new(settings), error: None, snapshot: None, preview: None, transient: false, task: None };
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
                            let current = effective(&snapshot, state.preview.as_ref())?;
                            apply_shared(&current, &source, cx)?;
                            Ok(Some((snapshot, current)))
                        });
                        match result {
                            Ok(Some((snapshot, current))) => {
                                state.current = Arc::new(current);
                                state.snapshot = Some(Arc::new(snapshot));
                                state.transient = state.preview.is_some();
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
    let current = effective(&snapshot, state.read(cx).preview.as_ref())?;
    apply_shared(&current, &source, cx)?;
    state.update(cx, |state, cx| {
        state.transient = state.preview.is_some();
        state.current = Arc::new(current);
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
    crate::theme_service::request(source.path().to_owned(), settings, cx);
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
    let service = cx.global_mut::<SettingsService>();
    service.active = Some(Arc::new(settings.clone()));
    service.active_source = Some(source.clone());
    Ok(())
}

/// Only reversible presentation and keymap changes are previewed. Never grant
/// network consent, switch checkouts, launch tools or trim navigation in a draft.
pub(crate) fn previewable(key: &str) -> bool {
    key != "layout.remember_per_repository"
        && [
            "ui.",
            "theme.",
            "editor.",
            "panels.",
            "window.",
            "layout.",
            "diff.",
            "overview.",
            "files.",
            "history.",
            "evolution.",
            "file_evolution.",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
        || key == "keyboard.profile"
}
fn effective(
    snapshot: &jaywalk::preferences::Snapshot,
    preview: Option<&Preview>,
) -> Result<Settings, String> {
    let Some(preview) = preview else {
        return Ok(snapshot.settings.clone());
    };
    let visual = preview
        .patches
        .iter()
        .filter(|patch| previewable(&patch.key))
        .cloned()
        .collect::<Vec<_>>();
    let mut settings = snapshot.preview(&visual)?.settings;
    // Remembering layout is persistence policy, not a visual preview.
    settings.layout.remember_per_repository = snapshot.settings.layout.remember_per_repository;
    if let Some(profile) = &preview.profile {
        settings = settings.with_keybindings(profile.clone())?;
    }
    Ok(settings)
}
pub(crate) fn preview(
    source: &SettingsSource,
    owner: EntityId,
    patches: Vec<jaywalk::preferences::Patch>,
    profile: Option<jaywalk::keybinding_profiles::Profile>,
    cx: &mut App,
) -> Result<(), String> {
    if patches.is_empty() && profile.is_none() {
        return clear_preview(source, owner, cx);
    }
    let Some(state) = cx
        .try_global::<SettingsService>()
        .and_then(|service| service.sources.get(source))
        .cloned()
    else {
        return Err("Settings are still loading".into());
    };
    let candidate = Preview {
        owner,
        patches,
        profile,
    };
    if state.read(cx).preview.as_ref() == Some(&candidate) {
        return Ok(());
    }
    let snapshot = state
        .read(cx)
        .snapshot
        .clone()
        .ok_or("Settings are still loading")?;
    let current = effective(&snapshot, Some(&candidate))?;
    commands::validate_bindings(&current, cx)?;
    apply_shared(&current, source, cx)?;
    state.update(cx, |state, cx| {
        state.preview = Some(candidate);
        state.transient = true;
        state.current = Arc::new(current);
        cx.notify();
    });
    Ok(())
}
pub(crate) fn clear_preview(
    source: &SettingsSource,
    owner: EntityId,
    cx: &mut App,
) -> Result<(), String> {
    let Some(state) = cx
        .try_global::<SettingsService>()
        .and_then(|service| service.sources.get(source))
        .cloned()
    else {
        return Ok(());
    };
    if !state
        .read(cx)
        .preview
        .as_ref()
        .is_some_and(|preview| preview.owner == owner)
    {
        return Ok(());
    }
    let saved = state
        .read(cx)
        .snapshot
        .clone()
        .ok_or("Settings are still loading")?;
    // An older window's cancellation must not overwrite a newer source's globals.
    if cx.global::<SettingsService>().active_source.as_ref() == Some(source) {
        apply_shared(&saved.settings, source, cx)?;
    }
    state.update(cx, |state, cx| {
        state.preview = None;
        state.transient = true;
        state.current = Arc::new(saved.settings.clone());
        cx.notify();
    });
    Ok(())
}
