//! Local UI state is separate from hand-edited startup settings.
use crate::{
    panel_layout::Placement,
    settings::{DiffView, FileView, Settings},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Mutex};
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutState {
    pub history_hidden: bool,
    pub files_hidden: bool,
    pub change_evolution: Option<bool>,
    pub file_evolution: Option<bool>,
    pub history_width: Option<f32>,
    pub files_width: Option<f32>,
    pub change_height: Option<f32>,
    pub file_height: Option<f32>,
    pub before_fraction: Option<f32>,
    pub placement: Option<Placement>,
    pub bottom_height: Option<f32>,
    pub bottom_fraction: Option<f32>,
    pub view: Option<DiffView>,
    pub file_view: Option<FileView>,
    pub wrap_before: Option<bool>,
    pub wrap_after: Option<bool>,
    pub wrap_inline: Option<bool>,
    pub line_numbers: Option<bool>,
    pub change_grouping: Option<bool>,
    pub file_grouping: Option<bool>,
    pub change_rebases: Option<bool>,
    pub file_rebases: Option<bool>,
}
impl LayoutState {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            history_hidden: !settings.layout.history_visible,
            files_hidden: !settings.layout.files_visible,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), String> {
        let value = self;
        for size in [
            value.history_width,
            value.files_width,
            value.change_height,
            value.file_height,
            value.bottom_height,
        ]
        .into_iter()
        .flatten()
        {
            if !size.is_finite() || !(1.0..=10000.).contains(&size) {
                return Err("Invalid saved panel size".into());
            }
        }
        if [value.before_fraction, value.bottom_fraction]
            .into_iter()
            .flatten()
            .any(|fraction| !fraction.is_finite() || !(0.05..=0.95).contains(&fraction))
        {
            return Err("Invalid saved code split".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::LayoutState;
    #[test]
    fn invalid_saved_geometry_is_rejected() {
        for state in [
            LayoutState {
                history_width: Some(f32::NAN),
                ..Default::default()
            },
            LayoutState {
                bottom_height: Some(0.),
                ..Default::default()
            },
            LayoutState {
                before_fraction: Some(1.5),
                ..Default::default()
            },
            LayoutState {
                bottom_fraction: Some(f32::INFINITY),
                ..Default::default()
            },
        ] {
            assert!(state.validate().is_err());
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RepositoryLayouts {
    #[serde(default = "state_version")]
    version: u32,
    #[serde(default)]
    repositories: BTreeMap<String, LayoutState>,
}
fn state_version() -> u32 {
    1
}
static STATE_WRITES: Mutex<()> = Mutex::new(());

/// Read-modify-write under one process lock keeps independent windows from
/// overwriting other repositories. The single-instance server owns this file.
pub struct RepositoryLayoutStore {
    path: PathBuf,
    repository: String,
}
impl RepositoryLayoutStore {
    pub fn new(path: PathBuf, repository: &Path) -> Result<Self, String> {
        let root = std::fs::canonicalize(repository).map_err(|error| {
            format!(
                "Could not identify repository {}: {error}",
                repository.display()
            )
        })?;
        let repository = root
            .to_str()
            .ok_or("Repository path is not valid UTF-8")?
            .to_owned();
        Ok(Self { path, repository })
    }
    fn read_all(&self) -> Result<RepositoryLayouts, String> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RepositoryLayouts {
                    version: state_version(),
                    ..Default::default()
                });
            }
            Err(error) => return Err(format!("Could not read {}: {error}", self.path.display())),
        };
        let saved: RepositoryLayouts = toml::from_str(&text)
            .map_err(|error| format!("Invalid interface state {}: {error}", self.path.display()))?;
        if saved.version != state_version() {
            return Err("Unsupported interface state version".into());
        }
        Ok(saved)
    }
    pub fn read(&self) -> Result<Option<LayoutState>, String> {
        let saved = self.read_all()?.repositories.remove(&self.repository);
        if let Some(state) = &saved {
            state.validate()?;
        }
        Ok(saved)
    }
    pub fn save(&self, state: &LayoutState) -> Result<(), String> {
        state.validate()?;
        let _lock = STATE_WRITES
            .lock()
            .map_err(|_| "Interface state writer failed")?;
        let mut saved = self.read_all()?;
        saved
            .repositories
            .insert(self.repository.clone(), state.clone());
        let text = toml::to_string_pretty(&saved).map_err(|error| error.to_string())?;
        let parent = self
            .path
            .parent()
            .ok_or("Interface state path has no parent")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temp = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temp, text)
            .and_then(|()| std::fs::rename(&temp, &self.path))
            .map_err(|error| format!("Could not save {}: {error}", self.path.display()))
    }
}

#[cfg(test)]
mod repository_tests {
    use super::*;
    #[test]
    fn saves_are_isolated_by_canonical_root_and_preserve_other_entries() {
        let root =
            std::env::temp_dir().join(format!("jw-layout-repositories-{}", std::process::id()));
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        let path = root.join("interface.toml");
        let one = RepositoryLayoutStore::new(path.clone(), &root.join("one")).unwrap();
        let alias = RepositoryLayoutStore::new(path.clone(), &root.join("one/.")).unwrap();
        let two = RepositoryLayoutStore::new(path.clone(), &root.join("two")).unwrap();
        assert!(one.read().unwrap().is_none());
        one.save(&LayoutState {
            history_width: Some(333.),
            ..Default::default()
        })
        .unwrap();
        two.save(&LayoutState {
            files_hidden: true,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(alias.read().unwrap().unwrap().history_width, Some(333.));
        assert!(two.read().unwrap().unwrap().files_hidden);
        one.save(&LayoutState::default()).unwrap();
        assert!(two.read().unwrap().unwrap().files_hidden);
        std::fs::write(&path, "broken = true").unwrap();
        assert!(one.read().is_err());
        assert!(one.save(&LayoutState::default()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
