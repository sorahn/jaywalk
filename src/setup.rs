//! Public first-run preferences; no network requests or credential access.
use crate::settings::{Appearance, Config};
use serde::Deserialize;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetupSettings {
    pub completed: bool,
}
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
    pub fn may_check_automatically(self, setup: SetupSettings) -> bool {
        setup.completed && self.automatic_checks
    }
}
/// Serialize app-owned writers, preserve comments/symlinks/permissions, and reject
/// edits made while preparing the replacement. Explicit configs must exist.
pub fn save(
    path: &Path,
    completed: bool,
    updates: UpdateSettings,
    appearance: Appearance,
) -> Result<Config, String> {
    updates.validate()?;
    let _writer = crate::settings_reload::CONFIG_WRITER
        .lock()
        .expect("config writer mutex poisoned");
    let target =
        std::fs::canonicalize(path).map_err(|e| format!("Could not resolve setup config: {e}"))?;
    let original = std::fs::read_to_string(&target)
        .map_err(|e| format!("Could not read setup config: {e}"))?;
    Config::parse(&original)?;
    let mut doc: toml_edit::DocumentMut = original
        .parse()
        .map_err(|e| format!("Could not edit setup config: {e}"))?;
    doc["setup"]["completed"] = toml_edit::value(completed);
    doc["updates"]["automatic_checks"] = toml_edit::value(updates.automatic_checks);
    doc["updates"]["automatic_downloads"] = toml_edit::value(updates.automatic_downloads);
    doc["updates"]["automatic_installation"] = toml_edit::value(updates.automatic_installation);
    doc["updates"]["check_interval_hours"] =
        toml_edit::value(i64::from(updates.check_interval_hours));
    doc["ui"]["appearance"] = toml_edit::value(match appearance {
        Appearance::System => "system",
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    });
    let updated = doc.to_string();
    let config = Config::parse(&updated)?;
    let mut temp = tempfile::NamedTempFile::new_in(target.parent().ok_or("Config has no parent")?)
        .map_err(|e| e.to_string())?;
    temp.as_file()
        .set_permissions(
            std::fs::metadata(&target)
                .map_err(|e| e.to_string())?
                .permissions(),
        )
        .map_err(|e| e.to_string())?;
    temp.write_all(updated.as_bytes())
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    if std::fs::read_to_string(&target).map_err(|e| e.to_string())? != original {
        return Err("Configuration changed during setup; keeping your edit. Try again.".into());
    }
    temp.persist(target).map_err(|e| e.to_string())?;
    Ok(config)
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
        assert!(!config.updates.may_check_automatically(config.setup));
        let mut updates = config.updates;
        updates.automatic_checks = true;
        assert!(!updates.may_check_automatically(config.setup));
        assert!(updates.may_check_automatically(SetupSettings { completed: true }));
        updates.automatic_checks = false;
        updates.automatic_downloads = true;
        assert!(updates.validate().is_err());
    }
    #[test]
    fn setup_saves_choices_without_destroying_other_settings_or_symlink() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, "# keep me\n[editor]\nfont_size=17\n").unwrap();
        let config = Config::parse("").unwrap();
        let saved = save(&path, true, config.updates, Appearance::Light).unwrap();
        assert!(saved.setup.completed);
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
            save(&link, true, config.updates, Appearance::Dark).unwrap();
            assert!(link.is_symlink());
        }
        std::fs::write(&path, "invalid = [").unwrap();
        assert!(save(&path, true, config.updates, Appearance::Light).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid = [");
    }
}
