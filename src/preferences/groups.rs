//! Presentation groups and their independent interface-state file.
use super::catalogue::Category;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    ThemeGeneration,
    DetailedColors,
    LayoutGeometry,
    LoadingLimits,
    HistoryGeometry,
    AcquisitionLimits,
    ProfileDiscovery,
    ReloadTuning,
    FileGeometry,
    EvolutionGeometry,
    RefreshTiming,
    CheckoutLimits,
    NavigationLimits,
}
impl Group {
    pub fn key(self) -> &'static str {
        match self {
            Self::ThemeGeneration => "theme_generation",
            Self::DetailedColors => "detailed_colors",
            Self::LayoutGeometry => "layout_geometry",
            Self::LoadingLimits => "loading_limits",
            Self::HistoryGeometry => "history_geometry",
            Self::AcquisitionLimits => "acquisition_limits",
            Self::ProfileDiscovery => "profile_discovery",
            Self::ReloadTuning => "reload_tuning",
            Self::FileGeometry => "file_geometry",
            Self::EvolutionGeometry => "evolution_geometry",
            Self::RefreshTiming => "refresh_timing",
            Self::CheckoutLimits => "checkout_limits",
            Self::NavigationLimits => "navigation_limits",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ThemeGeneration => "Theme generation",
            Self::DetailedColors => "Detailed colors",
            Self::LayoutGeometry => "Fine layout controls",
            Self::LoadingLimits => "History loading and limits",
            Self::HistoryGeometry => "Graph appearance",
            Self::AcquisitionLimits => "Acquisition and refresh limits",
            Self::ProfileDiscovery => "Profiles and navigation limits",
            Self::ReloadTuning => "Settings reload",
            Self::FileGeometry => "List and tree appearance",
            Self::EvolutionGeometry => "Evolution layout and limits",
            Self::RefreshTiming => "Refresh timing",
            Self::CheckoutLimits => "Checkout limits",
            Self::NavigationLimits => "Navigation history limits",
        }
    }
    pub fn category(self) -> Category {
        match self {
            Self::ThemeGeneration | Self::DetailedColors => Category::Appearance,
            Self::LayoutGeometry => Category::Layout,
            Self::LoadingLimits
            | Self::HistoryGeometry
            | Self::FileGeometry
            | Self::EvolutionGeometry => Category::History,
            Self::AcquisitionLimits | Self::RefreshTiming | Self::CheckoutLimits => {
                Category::Repositories
            }
            Self::ProfileDiscovery | Self::NavigationLimits => Category::Keyboard,
            Self::ReloadTuning => Category::General,
        }
    }
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Saved {
    expanded: BTreeSet<Group>,
}
pub fn state_path(config: &Path) -> PathBuf {
    config.with_extension("view-state.toml")
}
pub fn read(config: &Path) -> Result<BTreeSet<Group>, String> {
    let path = state_path(config);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => {
            return Err(format!(
                "Could not read settings view state {}: {e}",
                path.display()
            ));
        }
    };
    toml::from_str::<Saved>(&text)
        .map(|saved| saved.expanded)
        .map_err(|e| format!("Invalid settings view state {}: {e}", path.display()))
}
pub fn save(config: &Path, expanded: &BTreeSet<Group>) -> Result<(), String> {
    let path = state_path(config);
    let parent = path.parent().ok_or("Settings view state has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let text = toml::to_string_pretty(&Saved {
        expanded: expanded.clone(),
    })
    .map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, &path))
        .map_err(|e| format!("Could not save settings view state {}: {e}", path.display()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expansion_roundtrips_separately_from_preferences() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        std::fs::write(&config, "[updates]\nautomatic_checks=false\n").unwrap();
        let before = std::fs::read(&config).unwrap();
        assert!(read(&config).unwrap().is_empty());
        let expanded = BTreeSet::from([Group::ThemeGeneration]);
        save(&config, &expanded).unwrap();
        assert_eq!(read(&config).unwrap(), expanded);
        assert_eq!(std::fs::read(&config).unwrap(), before);
        assert!(read(&root.path().join("other.toml")).unwrap().is_empty());
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;
    use crate::preferences::catalogue::{SETTINGS, setting};
    #[test]
    fn groups_stay_in_their_category_and_keep_safety_choices_visible() {
        let mut sections = std::collections::BTreeMap::new();
        for entry in SETTINGS.iter() {
            assert!(
                !entry.section.is_empty(),
                "{} needs a contextual subsection",
                entry.key
            );
            if let Some(group) = entry.group {
                if let Some(section) = sections.insert(group, entry.section.as_str()) {
                    assert_eq!(
                        section,
                        entry.section,
                        "{} spans unrelated subsections",
                        group.key()
                    );
                }
                assert_eq!(entry.category, group.category(), "{}", entry.key);
            }
        }
        for key in [
            "updates.automatic_checks",
            "updates.automatic_downloads",
            "updates.automatic_installation",
            "local_review.mode",
            "local_review.checkout_branch",
            "local_review.cleanup_created_branch",
            "local_review.fetch_missing",
            "live.enabled",
        ] {
            assert!(
                setting(key).unwrap().group.is_none(),
                "{key} should stay visible"
            );
        }
        for key in [
            "theme.shell_auto.seed.light",
            "theme.shell_auto.seed.dark",
            "ui.accent_color",
            "history.page_size",
        ] {
            assert!(setting(key).unwrap().group.is_none(), "{key}");
        }
        assert!(
            setting("theme.shell_auto.layer_strength")
                .unwrap()
                .rank("theme generation")
                .is_some()
        );
        assert_eq!(
            setting("theme.shell_auto.layer_strength").unwrap().group,
            Some(Group::ThemeGeneration)
        );
    }
}
