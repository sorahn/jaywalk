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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let arguments = self.arguments.clone();
        let settings = self.settings.clone();
        cx.defer(move |cx| {
            if let Err(error) = crate::preferences_window::show_keyboard(arguments, settings, cx) {
                eprintln!("Could not open Keyboard Shortcuts: {error}");
            }
        });
    }
}
