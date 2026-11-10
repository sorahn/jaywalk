//! Content-based config polling tolerates editor atomic replacements and missing defaults.
use crate::keybinding_profiles::{self as profiles, Profile};
use crate::settings::{Config, Settings, default_path};
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SettingsSource {
    path: PathBuf,
    optional: bool,
}

impl SettingsSource {
    pub fn resolve(explicit: Option<&Path>) -> Result<Self, String> {
        let path = explicit
            .map(Path::to_path_buf)
            .map_or_else(default_path, Ok)?;
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|error| format!("Could not resolve settings directory: {error}"))?
                .join(path)
        };
        Ok(Self {
            path,
            optional: explicit.is_none(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> Result<(), String> {
        if self.optional {
            profiles::create_if_missing(&self.path, profiles::USER_CONFIG)?;
        }
        // Explicit configs must already exist; never create one on a mistyped path.
        let config = self.config_contents()?;
        // Keep the settings editor usable even while its TOML is invalid.
        if let Ok(settings) = Config::parse(&config)
            && settings.keyboard.profile == "keybindings/default.toml"
        {
            profiles::create_if_missing(
                &profiles::path(&self.path, &settings.keyboard.profile)?,
                profiles::USER_PROFILE,
            )?;
        }
        Ok(())
    }

    fn config_contents(&self) -> Result<String, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => Ok(contents),
            Err(error) if self.optional && error.kind() == std::io::ErrorKind::NotFound => {
                Ok(String::new())
            }
            Err(error) => Err(format!("Could not read {}: {error}", self.path.display())),
        }
    }

    fn contents(&self) -> Result<Contents, String> {
        let config = self.config_contents()?;
        let settings =
            Config::parse(&config).map_err(|error| format!("{}: {error}", self.path.display()))?;
        let profile_path = profiles::path(&self.path, &settings.keyboard.profile)?;
        let profile = std::fs::read_to_string(&profile_path)
            .map_err(|error| format!("Could not read {}: {error}", profile_path.display()))?;
        Ok(Contents {
            config,
            profile,
            profile_path,
        })
    }

    fn parse(&self, contents: Contents) -> Result<Settings, String> {
        let profile = Profile::parse(&contents.profile)
            .map_err(|error| format!("{}: {error}", contents.profile_path.display()))?;
        Settings::resolve(Config::parse(&contents.config)?, profile)
    }

    pub fn read(&self) -> Result<Settings, String> {
        self.initialize()?;
        self.parse(self.contents()?)
    }
}

#[derive(Clone, PartialEq)]
struct Contents {
    config: String,
    profile: String,
    profile_path: PathBuf,
}

pub struct SettingsWatcher {
    source: SettingsSource,
    observed: Option<Result<Contents, String>>,
    settled: Option<Result<Contents, String>>,
}

impl SettingsWatcher {
    pub fn new(source: SettingsSource) -> Self {
        Self {
            source,
            observed: None,
            settled: None,
        }
    }

    /// Read off the UI thread. Require two matching observations so an in-place
    /// save can settle before parsing. Content equality also catches replacements
    /// with unchanged timestamps/length and avoids repeatedly reporting one error.
    pub fn poll(&mut self) -> Option<Result<Settings, String>> {
        let contents = self.source.contents();
        if self.observed.as_ref() != Some(&contents) {
            self.observed = Some(contents);
            return None;
        }
        if self.settled.as_ref() == Some(&contents) {
            return None;
        }
        self.settled = Some(contents.clone());
        Some(contents.and_then(|contents| self.source.parse(contents)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    pub(super) struct Fixture(PathBuf);
    impl Fixture {
        pub(super) fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "jaywalk-config-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            profiles::create_if_missing(
                &profiles::path(&path.join("config.toml"), "keybindings/default.toml").unwrap(),
                profiles::USER_PROFILE,
            )
            .unwrap();
            Self(path)
        }
        pub(super) fn source(&self, optional: bool) -> SettingsSource {
            SettingsSource {
                path: self.0.join("config.toml"),
                optional,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn watches_creation_replacement_removal_and_recovery_without_metadata_assumptions() {
        let fixture = Fixture::new();
        let source = fixture.source(true);
        let path = source.path.clone();
        let mut watcher = SettingsWatcher::new(source);
        assert!(watcher.poll().is_none());
        assert_eq!(watcher.poll().unwrap().unwrap(), Settings::default());
        assert!(watcher.poll().is_none());
        std::fs::write(&path, "[ui]\nfont_size=18").unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(watcher.poll().unwrap().unwrap().ui.font_size, 18.);
        let replacement = fixture.0.join("replacement");
        std::fs::write(&replacement, "[ui]\nfont_size=19").unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(watcher.poll().unwrap().unwrap().ui.font_size, 19.);
        std::fs::remove_file(&path).unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(watcher.poll().unwrap().unwrap(), Settings::default());
    }

    #[test]
    fn invalid_or_missing_explicit_configs_report_once_and_recover() {
        let fixture = Fixture::new();
        let source = fixture.source(false);
        let path = source.path.clone();
        let mut watcher = SettingsWatcher::new(source);
        assert!(watcher.poll().is_none());
        assert!(watcher.poll().unwrap().unwrap_err().contains("config.toml"));
        assert!(watcher.poll().is_none());
        std::fs::write(&path, "[ui]\nfont_size=").unwrap();
        assert!(watcher.poll().is_none());
        assert!(watcher.poll().unwrap().is_err());
        assert!(watcher.poll().is_none());
        std::fs::write(&path, "[ui]\nfont_size=20").unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(watcher.poll().unwrap().unwrap().ui.font_size, 20.);
    }

    #[test]
    fn intermediate_save_contents_are_coalesced_and_equal_saves_are_ignored() {
        let fixture = Fixture::new();
        let source = fixture.source(false);
        std::fs::write(&source.path, "[ui]").unwrap();
        let mut watcher = SettingsWatcher::new(source.clone());
        assert!(watcher.poll().is_none());
        std::fs::write(&source.path, "[ui]\nfont_size=21").unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(watcher.poll().unwrap().unwrap().ui.font_size, 21.);
        std::fs::write(&source.path, "[ui]\nfont_size=21").unwrap();
        assert!(watcher.poll().is_none());
    }
}

#[cfg(test)]
mod profile_tests {
    use super::SettingsWatcher;
    use crate::{commands::CommandId, keybinding_profiles as profiles};
    #[test]
    fn watches_active_profile_edits_switches_errors_and_recovery() {
        let fixture = super::tests::Fixture::new();
        let source = fixture.source(false);
        std::fs::write(source.path(), "").unwrap();
        source.initialize().unwrap();
        let custom = profiles::path(source.path(), "keybindings/custom.toml").unwrap();
        std::fs::write(&custom, "[keybindings]\nfind_file=['secondary-j']").unwrap();
        let mut watcher = SettingsWatcher::new(source.clone());
        assert!(watcher.poll().is_none());
        watcher.poll().unwrap().unwrap();
        std::fs::write(
            source.path(),
            "[keyboard]\nprofile='keybindings/custom.toml'",
        )
        .unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(
            watcher.poll().unwrap().unwrap().keybindings[&CommandId::FindFile],
            ["secondary-j"]
        );
        std::fs::write(&custom, "[keybindings]\nfind_file=").unwrap();
        assert!(watcher.poll().is_none());
        assert!(watcher.poll().unwrap().is_err());
        assert!(watcher.poll().is_none());
        std::fs::remove_file(&custom).unwrap();
        assert!(watcher.poll().is_none());
        assert!(watcher.poll().unwrap().unwrap_err().contains("custom.toml"));
        std::fs::write(&custom, "[keybindings]\nfind_file=[]").unwrap();
        assert!(watcher.poll().is_none());
        assert!(watcher.poll().unwrap().unwrap().keybindings[&CommandId::FindFile].is_empty());
        let replacement = custom.with_extension("new");
        std::fs::write(&replacement, "[keybindings]\nfind_file=['secondary-u']").unwrap();
        std::fs::rename(replacement, &custom).unwrap();
        assert!(watcher.poll().is_none());
        assert_eq!(
            watcher.poll().unwrap().unwrap().keybindings[&CommandId::FindFile],
            ["secondary-u"]
        );
        std::fs::write(
            profiles::path(source.path(), "keybindings/default.toml").unwrap(),
            "not valid =",
        )
        .unwrap();
        assert!(watcher.poll().is_none());
    }
    #[test]
    fn bootstraps_sparse_files_without_overwriting_existing_contents() {
        let fixture = super::tests::Fixture::new();
        let source = fixture.source(true);
        source.read().unwrap();
        assert_eq!(
            std::fs::read_to_string(source.path()).unwrap(),
            profiles::USER_CONFIG
        );
        let profile = profiles::path(source.path(), "keybindings/default.toml").unwrap();
        std::fs::write(&profile, "# keep this\n[keybindings]\nfind_file=[]").unwrap();
        std::fs::write(source.path(), "# preserve\n[editor]\nfont_family='Menlo'").unwrap();
        source.read().unwrap();
        assert!(
            std::fs::read_to_string(&profile)
                .unwrap()
                .contains("# keep this")
        );
        assert!(
            std::fs::read_to_string(source.path())
                .unwrap()
                .contains("# preserve")
        );
    }
}
