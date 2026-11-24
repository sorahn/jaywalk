//! Application-owned local theme loading. Workers never mutate appearance;
//! generation checks keep old loads from overriding a newer preview or Discard.
use gpui::*;
use jaywalk::{
    settings::Settings,
    theme::{
        self,
        assets::{self, Asset, Catalog, Library},
    },
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Default)]
pub(crate) struct ThemeService {
    pub catalog: Catalog,
    pub diagnostics: Vec<String>,
    pub selected: [Option<Arc<Asset>>; 2],
    request: Option<(PathBuf, theme::ThemeSettings)>,
    serial: u64,
    library: Arc<Mutex<Library>>,
    task: Option<Task<()>>,
}
#[derive(Default)]
struct Service(Option<Entity<ThemeService>>);
impl Global for Service {}
pub(crate) fn service(cx: &mut App) -> Entity<ThemeService> {
    if !cx.has_global::<Service>() {
        cx.set_global(Service::default());
    }
    if let Some(service) = cx.global::<Service>().0.clone() {
        return service;
    }
    let entity = cx.new(|_| ThemeService::default());
    cx.global_mut::<Service>().0 = Some(entity.clone());
    entity
}
pub(crate) fn request(config: PathBuf, settings: &Settings, cx: &mut App) {
    let service = service(cx);
    let theme = settings.theme.clone();
    service.update(cx, |state, cx| {
        if state.request.as_ref() == Some(&(config.clone(), theme.clone())) {
            return;
        }
        state.serial += 1;
        let serial = state.serial;
        state.request = Some((config.clone(), theme.clone()));
        let library = state.library.clone();
        state.task = Some(cx.spawn(async move |this, cx| {
            let mut previous = None;
            loop {
                let config = config.clone();
                let theme = theme.clone();
                let library = library.clone();
                let result = cx
                    .background_spawn(async move {
                        let catalog = assets::catalog(&config);
                        let mut diagnostics = catalog.diagnostics.clone();
                        let mut library = library
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let selected = std::array::from_fn::<_, 2, _>(|i| {
                            let (asset, warnings) =
                                library.selected(&catalog, theme.code.get(i == 1), i == 1);
                            diagnostics.extend(warnings);
                            asset
                        });
                        drop(library);
                        let code = std::array::from_fn(|i| {
                            selected[i]
                                .as_ref()
                                .map(|asset| crate::appearance::code_asset(asset, i == 1))
                        });
                        let inputs = theme::CodeThemes {
                            light: code_colors(selected[0].as_deref(), false),
                            dark: code_colors(selected[1].as_deref(), true),
                        };
                        let resolved = Arc::new(theme::resolve_with_code(&theme, inputs));
                        let identities = selected
                            .each_ref()
                            .map(|asset| asset.as_ref().map(|asset| asset.identity.clone()));
                        (catalog, diagnostics, identities, code, resolved, selected)
                    })
                    .await;
                let (catalog, diagnostics, identities, code, resolved, selected) = result;
                let fingerprint = (catalog.clone(), diagnostics.clone(), identities);
                let changed = previous.as_ref() != Some(&fingerprint);
                previous = Some(fingerprint);
                let Ok(current) = this.update(cx, |state, cx| {
                    if state.serial != serial {
                        return false;
                    }
                    if changed {
                        state.catalog = catalog;
                        state.diagnostics = diagnostics;
                        state.selected = selected;
                        crate::appearance::publish_code(code, resolved, cx);
                        cx.notify();
                    }
                    true
                }) else {
                    break;
                };
                if !current {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        }));
    });
}
fn code_colors(asset: Option<&Asset>, dark: bool) -> theme::CodeColors {
    let defaults = theme::CodeThemes::default();
    let fallback = if dark { defaults.dark } else { defaults.light };
    let Some(asset) = asset else {
        return fallback;
    };
    let color = |key| asset.editor.get(key).copied().map(theme::Color);
    let background = color("editor.background").unwrap_or(fallback.background);
    theme::CodeColors {
        background,
        foreground: color("editor.foreground")
            .map(|fg| background.blend(fg, f64::from((fg.0 & 255) as u8) / 255.)),
    }
}
/// Force a catalog rescan after an import without changing user settings.
pub(crate) fn rescan(cx: &mut App) {
    let service = service(cx);
    let request = service.update(cx, |state, _| {
        let request = state.request.clone();
        state.request = None;
        request
    });
    if let Some((path, theme)) = request {
        let mut settings = Settings::default();
        settings.theme = theme;
        self::request(path, &settings, cx);
    }
}
