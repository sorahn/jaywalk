//! Content-based config polling tolerates editor atomic replacements and missing defaults.
use crate::settings::{Settings, default_path};
use std::path::{Path, PathBuf};

#[derive(Clone)]
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

    fn contents(&self) -> Result<String, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => Ok(contents),
            Err(error) if self.optional && error.kind() == std::io::ErrorKind::NotFound => {
                Ok(String::new())
            }
            Err(error) => Err(format!("Could not read {}: {error}", self.path.display())),
        }
    }

    fn parse(&self, contents: &str) -> Result<Settings, String> {
        Settings::parse(contents).map_err(|error| format!("{}: {error}", self.path.display()))
    }

    pub fn read(&self) -> Result<Settings, String> {
        self.parse(&self.contents()?)
    }
}

pub struct SettingsWatcher {
    source: SettingsSource,
    observed: Option<Result<String, String>>,
    settled: Option<Result<String, String>>,
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
        Some(contents.and_then(|contents| self.source.parse(&contents)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "jaywalk-config-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn source(&self, optional: bool) -> SettingsSource {
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
