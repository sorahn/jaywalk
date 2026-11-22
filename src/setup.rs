//! Public first-run preferences; no network requests or credential access.
use crate::settings::{Appearance, Config};
use serde::Deserialize;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetupSettings {
    pub completed: bool,
    #[serde(default)]
    pub seen: bool,
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
    pub fn may_check_automatically(self) -> bool {
        self.automatic_checks
    }
}
/// Save completion and appearance only; update consent is owned by checkbox writes.
/// Serialize app-owned writers, preserve comments/symlinks/permissions, and reject
/// edits made while preparing the replacement. Explicit configs must exist.
pub fn save(path: &Path, completed: bool, appearance: Appearance) -> Result<Config, String> {
    edit(path, |doc, _| {
        doc["setup"]["completed"] = toml_edit::value(completed);
        doc["ui"]["appearance"] = toml_edit::value(match appearance {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        });
        if completed {
            doc["setup"]["seen"] = toml_edit::value(true);
        }
        Ok(())
    })
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
    edit(path, |doc, config| {
        let mut updates = config.updates;
        match choice {
            UpdateChoice::Checks(enabled) => {
                updates.automatic_checks = enabled;
                if !enabled {
                    updates.automatic_downloads = false;
                    updates.automatic_installation = false;
                }
            }
            UpdateChoice::Downloads(enabled) => {
                updates.automatic_downloads = enabled;
                if !enabled {
                    updates.automatic_installation = false;
                }
            }
            UpdateChoice::Installation(enabled) => updates.automatic_installation = enabled,
        }
        updates.validate()?;
        doc["updates"]["automatic_checks"] = toml_edit::value(updates.automatic_checks);
        doc["updates"]["automatic_downloads"] = toml_edit::value(updates.automatic_downloads);
        doc["updates"]["automatic_installation"] = toml_edit::value(updates.automatic_installation);
        Ok(())
    })
}
/// Claim the first automatic presentation atomically, without saving draft choices
/// or authorizing any network traffic. Call on the window-preparation worker.
pub fn claim_first_open(path: &Path) -> Result<bool, String> {
    let mut first = false;
    edit(path, |doc, config| {
        if !config.setup.completed && !config.setup.seen {
            doc["setup"]["seen"] = toml_edit::value(true);
            first = true;
        }
        Ok(())
    })?;
    Ok(first)
}
fn edit(
    path: &Path,
    change: impl FnOnce(&mut toml_edit::DocumentMut, &Config) -> Result<(), String>,
) -> Result<Config, String> {
    let _writer = crate::settings_reload::CONFIG_WRITER
        .lock()
        .expect("config writer mutex poisoned");
    let target =
        std::fs::canonicalize(path).map_err(|e| format!("Could not resolve setup config: {e}"))?;
    let original = std::fs::read_to_string(&target)
        .map_err(|e| format!("Could not read setup config: {e}"))?;
    let config = Config::parse(&original)?;
    let mut doc: toml_edit::DocumentMut = original
        .parse()
        .map_err(|e| format!("Could not edit setup config: {e}"))?;
    change(&mut doc, &config)?;
    let updated = doc.to_string();
    let config = Config::parse(&updated)?;
    if updated == original {
        return Ok(config);
    }
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
        assert!(!config.updates.may_check_automatically());
        let mut updates = config.updates;
        updates.automatic_checks = true;
        assert!(updates.may_check_automatically());
        updates.automatic_checks = false;
        updates.automatic_downloads = true;
        assert!(updates.validate().is_err());
    }
    #[test]
    fn first_presentation_is_remembered_without_completing_setup_or_authorizing_updates() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep\n[ui]\nappearance='light'\n[updates]\nautomatic_checks=true\n",
        )
        .unwrap();
        assert!(claim_first_open(&path).unwrap());
        let saved = Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved.setup.seen && !saved.setup.completed);
        assert!(saved.updates.may_check_automatically());
        assert_eq!(saved.ui.appearance, Appearance::Light);
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("# keep"));
        assert!(!claim_first_open(&path).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        // Older completed configs never need a second first-open page.
        std::fs::write(&path, "[setup]\ncompleted=true\n").unwrap();
        assert!(!claim_first_open(&path).unwrap());
        std::fs::write(&path, "invalid = [").unwrap();
        assert!(claim_first_open(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid = [");
    }
    #[test]
    fn simultaneous_open_requests_claim_only_one_setup_page() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let other = path.clone();
        let worker = std::thread::spawn(move || claim_first_open(&other).unwrap());
        let first = claim_first_open(&path).unwrap();
        let second = worker.join().unwrap();
        assert_ne!(first, second);
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
        assert!(saved.setup.seen && !saved.setup.completed);
        assert!(saved.updates.may_check_automatically());
        assert_eq!(saved.ui.appearance, Appearance::Light);
        let saved = save_update_choice(&path, UpdateChoice::Checks(false)).unwrap();
        assert!(!saved.updates.may_check_automatically());
        assert!(!saved.setup.completed);
        assert!(std::fs::read_to_string(&path).unwrap().contains("# keep"));
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
        let saved = save(&path, true, Appearance::Light).unwrap();
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
            save(&link, true, Appearance::Dark).unwrap();
            assert!(link.is_symlink());
        }
        std::fs::write(&path, "invalid = [").unwrap();
        assert!(save(&path, true, Appearance::Light).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid = [");
    }
}
