//! Ordered local repository history, independent of comparison/view history.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    version: u32,
    repositories: Vec<PathBuf>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            version: 1,
            repositories: Vec::new(),
        }
    }
}
pub struct RecentStore {
    path: PathBuf,
    history: History,
    read_error: Option<String>,
}
impl RecentStore {
    pub fn default_path() -> Result<PathBuf, String> {
        let config = crate::settings::default_path()?;
        let directory = config
            .parent()
            .ok_or("Settings path has no parent directory")?;
        Ok(directory.join("recent-repositories.toml"))
    }
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let history = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut history: History = toml::from_str(&text).map_err(|error| {
                    format!(
                        "Could not read recent repositories in {}: {error}",
                        path.display()
                    )
                })?;
                if history.version != 1 {
                    return Err("Unsupported recent-repository history version".into());
                }
                if history.repositories.iter().any(|path| !path.is_absolute()) {
                    return Err("Recent repository paths must be absolute".into());
                }
                let mut seen = std::collections::HashSet::new();
                history
                    .repositories
                    .retain(|path| seen.insert(path.clone()));
                history
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => History::default(),
            Err(error) => {
                return Err(format!(
                    "Could not read recent repositories in {}: {error}",
                    path.display()
                ));
            }
        };
        Ok(Self {
            path,
            history,
            read_error: None,
        })
    }
    /// Allow in-memory use after a failed read, without overwriting that file.
    /// Only an explicit Clear action authorizes replacing unreadable history.
    pub fn preserve_failed(path: PathBuf, error: String) -> Self {
        Self {
            path,
            history: History::default(),
            read_error: Some(error),
        }
    }
    pub fn repositories(&self) -> &[PathBuf] {
        &self.history.repositories
    }
    pub fn error(&self) -> Option<&str> {
        self.read_error.as_deref()
    }
    pub fn record(&mut self, directory: &Path) -> Result<(), String> {
        if !directory.is_absolute() {
            return Err("Recent repository paths must be absolute".into());
        }
        self.history.repositories.retain(|path| path != directory);
        self.history.repositories.insert(0, directory.to_owned());
        self.save()
    }
    pub fn clear(&mut self) -> Result<(), String> {
        self.history = History::default();
        self.read_error = None;
        self.save()
    }
    fn save(&self) -> Result<(), String> {
        if let Some(error) = &self.read_error {
            return Err(format!(
                "{error}. Saved history was preserved; Clear recent repositories to reset it."
            ));
        }
        let text = toml::to_string_pretty(&self.history)
            .map_err(|error| format!("Could not encode recent repositories: {error}"))?;
        let directory = self
            .path
            .parent()
            .ok_or("Recent history path has no parent directory")?;
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .map_err(|error| {
                format!(
                    "Could not save recent repositories in {}: {error}",
                    self.path.display()
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::RecentStore;
    use std::path::PathBuf;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("jw-recent-{}-{serial}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> PathBuf {
            self.0.join("recent.toml")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn history_roundtrips_in_mru_order_without_duplicate_or_missing_path_pruning() {
        let fixture = Fixture::new();
        let mut store = RecentStore::load(fixture.path()).unwrap();
        let a = fixture.0.join("first repo");
        let b = fixture.0.join("second 漢字");
        store.record(&a).unwrap();
        store.record(&b).unwrap();
        store.record(&a).unwrap();
        assert_eq!(
            RecentStore::load(fixture.path()).unwrap().repositories(),
            [a, b]
        );
        store.clear().unwrap();
        assert!(
            RecentStore::load(fixture.path())
                .unwrap()
                .repositories()
                .is_empty()
        );
    }
    #[test]
    fn unreadable_history_is_preserved_until_explicitly_cleared() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path(), "not valid toml [").unwrap();
        let error = RecentStore::load(fixture.path()).err().unwrap();
        let mut store = RecentStore::preserve_failed(fixture.path(), error);
        assert!(store.record(&fixture.0.join("repo")).is_err());
        assert_eq!(
            std::fs::read_to_string(fixture.path()).unwrap(),
            "not valid toml ["
        );
        assert_eq!(store.repositories().len(), 1);
        store.clear().unwrap();
        assert!(
            RecentStore::load(fixture.path())
                .unwrap()
                .repositories()
                .is_empty()
        );
    }
    #[test]
    fn incompatible_state_and_relative_paths_are_rejected() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path(), "version = 2\nrepositories = []").unwrap();
        assert!(RecentStore::load(fixture.path()).is_err());
        std::fs::write(fixture.path(), "version = 1\nrepositories = ['relative']").unwrap();
        assert!(RecentStore::load(fixture.path()).is_err());
        let mut store = RecentStore::preserve_failed(fixture.path(), "bad data".into());
        assert!(store.record(std::path::Path::new("relative")).is_err());
    }
}
