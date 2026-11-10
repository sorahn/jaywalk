//! Sparse keyboard profiles; shipped defaults remain separate from user files.
use crate::commands::CommandId;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_BINDINGS: &str = include_str!("../config/keybindings.toml");
pub const USER_CONFIG: &str = include_str!("../config/user.toml");
pub const USER_PROFILE: &str = include_str!("../config/user-keybindings.toml");

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default = "default_bindings")]
    pub keybindings: BTreeMap<CommandId, Vec<String>>,
    #[serde(default)]
    pub keymap: Vec<crate::keymap::Override>,
}
pub fn default_bindings() -> BTreeMap<CommandId, Vec<String>> {
    // Parse the shipped table directly; Profile's default must not recurse.
    #[derive(Deserialize)]
    struct Defaults {
        keybindings: BTreeMap<CommandId, Vec<String>>,
    }
    toml::from_str::<Defaults>(DEFAULT_BINDINGS)
        .expect("shipped keybindings are valid TOML")
        .keybindings
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            keybindings: default_bindings(),
            keymap: Vec::new(),
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        crate::keymap::validate(&self.keymap)?;
        let mut shortcuts = BTreeMap::new();
        for (command, bindings) in &self.keybindings {
            for binding in bindings {
                if binding.trim().is_empty() {
                    return Err(format!(
                        "Empty shortcut for {}; use [] to unbind it",
                        command.label()
                    ));
                }
                let mut normalized = Vec::new();
                for stroke in binding.split_whitespace() {
                    let stroke = gpui_kit::Keystroke::parse(stroke).map_err(|error| {
                        format!(
                            "Invalid shortcut {binding:?} for {}: {error}",
                            command.label()
                        )
                    })?;
                    normalized.push(stroke.unparse());
                }
                if let Some(previous) = shortcuts.insert(normalized, command)
                    && previous != command
                {
                    return Err(format!(
                        "Shortcut {binding:?} is assigned to both {} and {}; unbind or reassign one",
                        previous.label(),
                        command.label()
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut value: toml::Value =
            toml::from_str(input).map_err(|error| format!("Invalid keybindings TOML: {error}"))?;
        let table = value.as_table_mut().ok_or("Expected a keybindings table")?;
        if let Some(overrides) = table
            .get_mut("keybindings")
            .and_then(toml::Value::as_table_mut)
        {
            let defaults: toml::Value =
                toml::from_str(DEFAULT_BINDINGS).expect("shipped keybindings are valid TOML");
            for (name, keys) in defaults["keybindings"]
                .as_table()
                .expect("shipped commands form a table")
            {
                overrides
                    .entry(name.clone())
                    .or_insert_with(|| keys.clone());
            }
        }
        let profile: Self = value
            .try_into()
            .map_err(|error| format!("Invalid keybindings profile: {error}"))?;
        profile.validate()?;
        Ok(profile)
    }
}

pub fn validate_reference(reference: &str) -> Result<(), String> {
    if reference.trim().is_empty() || reference.contains('\0') {
        return Err("keyboard.profile must be a nonempty file path without NUL characters".into());
    }
    Ok(())
}
pub fn path(config: &Path, reference: &str) -> Result<PathBuf, String> {
    validate_reference(reference)?;
    Ok(config.parent().unwrap_or(Path::new(".")).join(reference))
}

/// Never overwrite user contents, including malformed or empty files.
pub fn create_if_missing(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
    {
        Ok(mut file) => file
            .write_all(contents.as_bytes())
            .map_err(|error| format!("Could not write {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(format!("Could not create {}: {error}", path.display())),
    }
}

/// Discover names only. A malformed profile remains discoverable for editing.
pub fn list(config: &Path, active: &str, additional: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let active_path = path(config, active)?;
    let active_canonical = match active_path.canonicalize() {
        Ok(path) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Could not resolve {}: {error}",
                active_path.display()
            ));
        }
    };
    let folders =
        std::iter::once(Path::new("keybindings")).chain(additional.iter().map(PathBuf::as_path));
    for folder in folders {
        let directory = config.parent().unwrap_or(Path::new(".")).join(folder);
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => Some(entries),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("Could not list {}: {error}", directory.display())),
        };
        for entry in entries.into_iter().flatten() {
            let entry = entry
                .map_err(|error| format!("Could not read {}: {error}", directory.display()))?;
            let entry_path = entry.path();
            if entry_path
                .extension()
                .is_some_and(|extension| extension == "toml")
                && entry_path.is_file()
            {
                let canonical = entry_path.canonicalize().map_err(|error| {
                    format!("Could not resolve {}: {error}", entry_path.display())
                })?;
                // Keep the active spelling so its row remains marked Active.
                if active_canonical.as_ref() == Some(&canonical) {
                    seen.insert(canonical);
                    continue;
                }
                if seen.insert(canonical) {
                    let reference = folder.join(entry.file_name());
                    names.push(
                        reference
                            .to_str()
                            .ok_or_else(|| {
                                format!(
                                    "Profile filename is not valid UTF-8: {}",
                                    entry_path.display()
                                )
                            })?
                            .to_owned(),
                    );
                }
            }
        }
    }
    names.push(active.to_owned());
    names.sort();
    Ok(names)
}

/// Validate the candidate before changing the config. The UI also preflights
/// toolkit-action remapping against registered bindings before calling save.
pub struct Selection {
    pub settings: crate::settings::Settings,
    config_path: PathBuf,
    original: String,
    updated: String,
    profile_path: PathBuf,
    profile: String,
}
impl Selection {
    pub fn prepare(config: &Path, name: &str) -> Result<Self, String> {
        let profile_path = path(config, name)?;
        let profile = std::fs::read_to_string(&profile_path)
            .map_err(|error| format!("Could not read {}: {error}", profile_path.display()))?;
        let bindings = Profile::parse(&profile)
            .map_err(|error| format!("{}: {error}", profile_path.display()))?;
        let config_path = std::fs::canonicalize(config)
            .map_err(|error| format!("Could not resolve {}: {error}", config.display()))?;
        let original = std::fs::read_to_string(&config_path)
            .map_err(|error| format!("Could not read {}: {error}", config_path.display()))?;
        crate::settings::Settings::parse(&original)?;
        let mut document: toml_edit::DocumentMut = original
            .parse()
            .map_err(|error| format!("Could not edit {}: {error}", config_path.display()))?;
        let decor = document
            .get("keyboard")
            .and_then(|item| item.get("profile"))
            .and_then(toml_edit::Item::as_value)
            .map(|value| value.decor().clone());
        let mut replacement = toml_edit::Value::from(name);
        if let Some(decor) = decor {
            *replacement.decor_mut() = decor;
        }
        document["keyboard"]["profile"] = toml_edit::Item::Value(replacement);
        let updated = document.to_string();
        let settings = crate::settings::Settings::parse(&updated)?.with_keybindings(bindings)?;
        Ok(Self {
            settings,
            config_path,
            original,
            updated,
            profile_path,
            profile,
        })
    }
    pub fn save(self) -> Result<crate::settings::Settings, String> {
        let read = |path: &Path| {
            std::fs::read_to_string(path)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))
        };
        if read(&self.config_path)? != self.original || read(&self.profile_path)? != self.profile {
            return Err(
                "Settings or profile changed while selecting; select the profile again.".into(),
            );
        }
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = self.config_path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let result = (|| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.set_permissions(std::fs::metadata(&self.config_path)?.permissions())?;
            file.write_all(self.updated.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.config_path)
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!(
                "Could not save {}: {error}",
                self.config_path.display()
            ));
        }
        Ok(self.settings)
    }
}

#[cfg(test)]
mod storage_tests {
    use super::{Selection, USER_PROFILE, create_if_missing, list};
    #[test]
    fn discovers_extra_folders_and_preserves_comments_when_selecting_an_external_profile() {
        let root = std::env::temp_dir().join(format!("jw-profile-storage-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("config.toml");
        let original = "# keep my comments\n[editor]\nfont_family='Example' # font\n\n[keyboard]\nprofile='keybindings/default.toml' # choice\nadditional_directories=['shared', 'shared']\n";
        std::fs::write(&config, original).unwrap();
        create_if_missing(&root.join("keybindings/default.toml"), USER_PROFILE).unwrap();
        create_if_missing(
            &root.join("shared/custom.toml"),
            "[keybindings]\nfind_file=[]",
        )
        .unwrap();
        create_if_missing(&root.join("shared/ignore.txt"), "not a profile").unwrap();
        create_if_missing(&root.join("shared/broken.toml"), "invalid=").unwrap();
        let candidates = list(
            &config,
            "keybindings/default.toml",
            &["shared".into(), "shared".into(), "absent".into()],
        )
        .unwrap();
        assert_eq!(
            candidates,
            [
                "keybindings/default.toml",
                "shared/broken.toml",
                "shared/custom.toml"
            ]
        );
        assert!(Selection::prepare(&config, "shared/broken.toml").is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        let pending = Selection::prepare(&config, "shared/custom.toml").unwrap();
        std::fs::write(&config, format!("{original}# concurrent edit\n")).unwrap();
        assert!(pending.save().is_err());
        std::fs::write(&config, original).unwrap();
        let selected = Selection::prepare(&config, "shared/custom.toml")
            .unwrap()
            .save()
            .unwrap();
        assert_eq!(selected.keyboard.profile, "shared/custom.toml");
        let updated = std::fs::read_to_string(&config).unwrap();
        assert!(updated.contains("# keep my comments"));
        assert!(updated.contains("font_family='Example' # font"));
        assert!(updated.contains("# choice"));
        assert!(selected.keybindings[&crate::commands::CommandId::FindFile].is_empty());
        assert_eq!(
            list(&config, "shared/custom.toml", &[]).unwrap(),
            ["keybindings/default.toml", "shared/custom.toml"]
        );
        #[cfg(unix)]
        {
            let alias = root.join("alias.toml");
            std::os::unix::fs::symlink(&config, &alias).unwrap();
            Selection::prepare(&alias, "keybindings/default.toml")
                .unwrap()
                .save()
                .unwrap();
            assert!(alias.is_symlink());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::{Profile, path};
    use crate::{commands::CommandId, settings::Settings};
    #[test]
    fn sparse_profiles_inherit_and_replace_arrays_without_accepting_app_settings() {
        let profile = Profile::parse(
            "[keybindings]\nfind_file=['secondary-k secondary-p']\ntoggle_history=[]",
        )
        .unwrap();
        assert_eq!(profile.keybindings.len(), CommandId::ALL.len());
        assert!(profile.keybindings[&CommandId::ToggleHistory].is_empty());
        assert_eq!(
            profile.keybindings[&CommandId::OpenSettings],
            ["secondary-,"]
        );
        for input in [
            "[editor]\nfont_size=13",
            "[keybindings]\nfind_file=['secondary-b']",
            "[keybindings]\nfind_file=['']",
            "[keybindings]\nfind_file=['mystery-p']",
            "[keybindings]\nunknown=[]",
        ] {
            assert!(Profile::parse(input).is_err(), "{input}");
        }
        assert!(
            Settings::parse("[keybindings]\nfind_file=[]")
                .unwrap_err()
                .contains("profile")
        );
        assert!(Settings::parse("keymap=[]").is_err());
    }
    #[test]
    fn profile_paths_can_be_relative_or_absolute() {
        for reference in ["", " ", "bad\0name"] {
            assert!(path(std::path::Path::new("/config/config.toml"), reference).is_err());
        }
        assert_eq!(
            path(
                std::path::Path::new("/config/custom.toml"),
                "../shared/my keys.toml"
            )
            .unwrap(),
            std::path::Path::new("/config/../shared/my keys.toml")
        );
        assert_eq!(
            path(
                std::path::Path::new("/config/custom.toml"),
                "/shared/keys.toml"
            )
            .unwrap(),
            std::path::Path::new("/shared/keys.toml")
        );
    }
}
