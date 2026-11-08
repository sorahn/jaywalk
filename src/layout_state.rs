//! Local UI state is separate from hand-edited startup settings.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
}
impl LayoutState {
    pub fn path() -> Result<PathBuf, String> {
        Ok(crate::settings::default_path()?.with_file_name("layout.toml"))
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
        };
        let value: Self = toml::from_str(&text)
            .map_err(|error| format!("Invalid {}: {error}", path.display()))?;
        for size in [
            value.history_width,
            value.files_width,
            value.change_height,
            value.file_height,
        ]
        .into_iter()
        .flatten()
        {
            if !size.is_finite() || !(1.0..=10000.).contains(&size) {
                return Err("Invalid saved panel size".into());
            }
        }
        if value
            .before_fraction
            .is_some_and(|fraction| !fraction.is_finite() || !(0.05..=0.95).contains(&fraction))
        {
            return Err("Invalid saved code split".into());
        }
        Ok(value)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        let parent = path.parent().ok_or("Layout path has no parent")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, path))
            .map_err(|error| format!("Could not save {}: {error}", path.display()))
    }
}
#[cfg(test)]
mod tests {
    use super::LayoutState;
    #[test]
    fn layout_round_trip_and_invalid_geometry() {
        let path = std::env::temp_dir().join(format!("viewer-layout-{}.toml", std::process::id()));
        let state = LayoutState {
            history_hidden: true,
            history_width: Some(333.),
            ..Default::default()
        };
        state.save(&path).unwrap();
        let loaded = LayoutState::read(&path).unwrap();
        assert!(loaded.history_hidden);
        assert_eq!(loaded.history_width, Some(333.));
        std::fs::write(&path, "before_fraction=2.0").unwrap();
        assert!(LayoutState::read(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
