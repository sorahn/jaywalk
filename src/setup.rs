//! Public first-run preferences; no network requests or credential access.
use crate::settings::{Appearance, Config};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettings {
    pub automatic_checks: bool,
    pub automatic_downloads: bool,
    pub automatic_installation: bool,
    pub check_interval_hours: u32,
}
impl UpdateSettings {
    pub fn validate(self) -> Result<(), String> {
        if self.automatic_downloads && !self.automatic_checks {
            return Err("updates.automatic_downloads requires automatic_checks".into());
        }
        if self.automatic_installation && !self.automatic_downloads {
            return Err("updates.automatic_installation requires automatic_downloads".into());
        }
        if !(1..=168).contains(&self.check_interval_hours) {
            return Err("updates.check_interval_hours must be 1–168".into());
        }
        Ok(())
    }
    pub fn may_check_automatically(self) -> bool {
        self.automatic_checks
    }
}
/// Save appearance only; update consent is owned by checkbox writes.
/// Serialize app-owned writers, preserve comments/symlinks/permissions, and reject
/// edits made while preparing the replacement. Explicit configs must exist.
pub fn save(path: &Path, appearance: Appearance) -> Result<Config, String> {
    let value = match appearance {
        Appearance::System => "system",
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    };
    let source = crate::settings_reload::SettingsSource::resolve(Some(path))?;
    let snapshot = crate::preferences::patch(
        &source,
        crate::preferences::Patch::set("ui.appearance", value.into()),
    )?;
    Ok(Config::clone(&snapshot.settings))
}
/// A single explicit checkbox action, applied to the latest committed preferences.
#[derive(Clone, Copy, Debug)]
pub enum UpdateChoice {
    Checks(bool),
    Downloads(bool),
    Installation(bool),
}
/// Never replay another window's consent snapshot. Disabling a prerequisite also
/// disables its dependants; enabling one never implicitly grants prerequisites.
pub fn save_update_choice(path: &Path, choice: UpdateChoice) -> Result<Config, String> {
    let (key, value) = match choice {
        UpdateChoice::Checks(value) => ("updates.automatic_checks", value),
        UpdateChoice::Downloads(value) => ("updates.automatic_downloads", value),
        UpdateChoice::Installation(value) => ("updates.automatic_installation", value),
    };
    let source = crate::settings_reload::SettingsSource::resolve(Some(path))?;
    let snapshot =
        crate::preferences::patch(&source, crate::preferences::Patch::set(key, value.into()))?;
    Ok(Config::clone(&snapshot.settings))
}
/// Use the same Finder-aware subprocess environment as repository commands.
pub fn tool_version(tool: &str) -> Result<String, String> {
    if !matches!(tool, "git" | "jj") {
        return Err("Unknown repository tool".into());
    }
    let output = crate::tools::command(tool)?
        .arg("--version")
        .output()
        .map_err(|e| format!("{tool}: not available ({e})"))?;
    if !output.status.success() {
        return Err(format!("{tool}: version check failed"));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_config_does_not_authorize_traffic_and_invalid_combinations_fail() {
        let config = Config::parse("").unwrap();
        assert!(!config.updates.may_check_automatically());
        let mut updates = config.updates;
        updates.automatic_checks = true;
        assert!(updates.may_check_automatically());
        updates.automatic_checks = false;
        updates.automatic_downloads = true;
        assert!(updates.validate().is_err());
    }
    #[test]
    fn checkbox_choices_save_without_completing_setup_or_changing_appearance() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep\n[setup]\nseen=true\ncompleted=false\n[ui]\nappearance='light'\n",
        )
        .unwrap();
        let saved = save_update_choice(&path, UpdateChoice::Checks(true)).unwrap();
        assert!(saved.updates.may_check_automatically());
        assert_eq!(saved.ui.appearance, Appearance::Light);
        let saved = save_update_choice(&path, UpdateChoice::Checks(false)).unwrap();
        assert!(!saved.updates.may_check_automatically());
        assert!(std::fs::read_to_string(&path).unwrap().contains("# keep"));
    }
    #[test]
    fn setup_writes_standard_sections_and_preserves_comments_and_legacy_choices() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, "# keep header\nsetup = { completed = true }\n# keep ui heading\nui = { appearance = 'system', font_size = 18 } # keep ui note\n[updates]\nautomatic_checks = false # keep consent comment\ncheck_interval_hours = 48\n").unwrap();
        save(&path, Appearance::Dark).unwrap();
        save_update_choice(&path, UpdateChoice::Checks(true)).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("[ui]"), "{saved}");
        assert!(saved.contains("[updates]"));
        assert!(saved.contains("# keep header"));
        assert!(saved.contains("# keep consent comment"));
        assert!(saved.contains("# keep ui heading"));
        assert!(saved.contains("# keep ui note"));
        assert!(!saved.contains("setup ="));
        let config = Config::parse(&saved).unwrap();
        assert_eq!(config.ui.font_size, 18.);
        assert_eq!(config.updates.check_interval_hours, 48);
        assert!(config.updates.automatic_checks);
        assert_eq!(config.ui.appearance, Appearance::Dark);
        std::fs::write(&path, "").unwrap();
        save_update_choice(&path, UpdateChoice::Checks(false)).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("[updates]")
        );
    }
    #[test]
    fn editing_commented_reference_keeps_fields_in_documented_order() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, crate::keybinding_profiles::user_config()).unwrap();
        save_update_choice(&path, UpdateChoice::Checks(true)).unwrap();
        save(&path, Appearance::Dark).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.find("[ui]\n").unwrap() < text.find("[theme]").unwrap());
        assert!(text.find("[updates]\n").unwrap() < text.find("[hosting]").unwrap());
        assert_eq!(text.matches("[updates]").count(), 1);
        let config = Config::parse(&text).unwrap();
        assert!(config.updates.automatic_checks);
        assert!(!config.updates.automatic_downloads);
        assert_eq!(config.ui.appearance, Appearance::Dark);
    }
    #[test]
    fn stale_checkbox_actions_cannot_restore_revoked_consent() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, "[updates]\nautomatic_checks=true\nautomatic_downloads=true\nautomatic_installation=true\ncheck_interval_hours=48\n").unwrap();
        // Two windows began with every option enabled. A revokes checks; B
        // disables downloads using its old form. Only B's choice is submitted.
        save_update_choice(&path, UpdateChoice::Checks(false)).unwrap();
        let saved = save_update_choice(&path, UpdateChoice::Downloads(false)).unwrap();
        assert!(!saved.updates.automatic_checks);
        assert!(!saved.updates.automatic_downloads);
        assert!(!saved.updates.automatic_installation);
        assert_eq!(saved.updates.check_interval_hours, 48);
        let revoked = std::fs::read_to_string(&path).unwrap();
        // Stale enabled controls also cannot implicitly opt back into checks.
        assert!(save_update_choice(&path, UpdateChoice::Downloads(true)).is_err());
        assert!(save_update_choice(&path, UpdateChoice::Installation(true)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), revoked);
    }
    #[test]
    fn setup_saves_choices_without_destroying_other_settings_or_symlink() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, "# keep me\n[editor]\nfont_size=17\n").unwrap();
        let saved = save(&path, Appearance::Light).unwrap();
        assert_eq!(saved.ui.appearance, Appearance::Light);
        assert_eq!(saved.editor.font_size, 17.);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("# keep me")
        );
        #[cfg(unix)]
        {
            let link = fixture.path().join("alias.toml");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            save(&link, Appearance::Dark).unwrap();
            assert!(link.is_symlink());
        }
        std::fs::write(&path, "invalid = [").unwrap();
        assert!(save(&path, Appearance::Light).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid = [");
    }
}
