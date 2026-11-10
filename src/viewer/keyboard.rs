//! Connect profile storage to the existing command palette and editor launcher.
use super::*;
use jaywalk::keybinding_profiles as profiles;
use jaywalk::settings_reload::SettingsSource;

pub(super) enum ConfigurationFile {
    Settings,
    Keybindings,
}
impl Viewer {
    pub(super) fn edit_configuration_file(
        &mut self,
        kind: ConfigurationFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = SettingsSource::resolve(self.arguments.config.as_deref());
        let profile = self.settings.keyboard.profile.clone();
        let editor = self.settings.settings.editor.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let source = source?;
                    source.initialize()?;
                    let path = match kind {
                        ConfigurationFile::Settings => source.path().to_owned(),
                        ConfigurationFile::Keybindings => profiles::path(source.path(), &profile)?,
                    };
                    std::fs::metadata(&path)
                        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
                    crate::platform::open_text_file(&path, editor.as_ref())
                })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |viewer, cx| {
                    viewer.config.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn choose_keybindings_profile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = SettingsSource::resolve(self.arguments.config.as_deref());
        let active = self.settings.keyboard.profile.clone();
        let additional = self.settings.keyboard.additional_directories.clone();
        self.config.profile_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let source = source?;
                    source.initialize()?;
                    profiles::list(source.path(), &active, &additional)
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| match result {
                Ok(profiles) => {
                    viewer.palette.profiles = profiles;
                    viewer.open_palette(palette::PaletteKind::Profiles, window, cx);
                }
                Err(error) => {
                    viewer.config.error = Some(error);
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn select_keybindings_profile(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = SettingsSource::resolve(self.arguments.config.as_deref());
        self.config.profile_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let source = source?;
                    profiles::Selection::prepare(source.path(), &name)
                })
                .await;
            let result = match result {
                Ok(selection) => {
                    let valid = this.update(cx, |_, cx| {
                        commands::validate_bindings(&selection.settings, cx)
                    });
                    match valid {
                        Ok(Ok(())) => cx.background_spawn(async move { selection.save() }).await,
                        Ok(Err(error)) => Err(error),
                        Err(_) => return,
                    }
                }
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |viewer, window, cx| {
                match result {
                    Ok(settings) => {
                        viewer.config.error = None;
                        viewer.apply_settings(settings, window, cx);
                    }
                    Err(error) => {
                        viewer.config.error =
                            Some(format!("Keybindings profile not changed. {error}"))
                    }
                }
                cx.notify();
            });
        }));
    }
}
