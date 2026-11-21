//! Dark/colored reference palettes come from shell-lab checkpoint
//! 9a4fe57eacddbb44f280fb94ccb857919fa40efb. The soft light default uses the
//! same Layered 1.4 offsets with the native-reviewed paper seed.
use super::*;
use crate::settings::Settings;

#[test]
fn prototype_palettes_and_soft_light_default_match_fixtures() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!("fixtures.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let settings = match fixture["seed"].as_str() {
            Some(seed) => Settings::parse(&format!("[theme.shell_auto]\nseed='{seed}'")).unwrap(),
            None => Settings::default(),
        };
        let shell = settings.resolved_theme().shell(fixture["mode"] == "dark");
        for (key, surface) in [
            ("window", Surface::Window),
            ("history", Surface::History),
            ("files", Surface::Files),
            ("header", Surface::Header),
            ("footer", Surface::Footer),
            ("historyHeader", Surface::HistoryHeader),
            ("filesHeader", Surface::FilesHeader),
            ("historyFooter", Surface::HistoryFooter),
            ("filesFooter", Surface::FilesFooter),
            ("viewerHeader", Surface::ViewerHeader),
            ("floating", Surface::Floating),
        ] {
            for (name, part) in [("bg", Part::Background), ("fg", Part::Foreground)] {
                let expected = u32::from_str_radix(
                    fixture["roles"][key][name]
                        .as_str()
                        .unwrap()
                        .trim_start_matches('#'),
                    16,
                )
                .unwrap();
                assert_eq!(
                    shell.surface(surface, part).rgb_value(),
                    expected,
                    "{:?} {key} {name}",
                    fixture["seed"]
                );
            }
        }
    }
}

#[test]
fn exact_overrides_win_and_report_low_contrast() {
    let settings = Settings::parse(
        r##"
[theme.overrides]
"shell.sidebar.history.background" = "#ffffff"
"shell.sidebar.history.foreground" = {light="#ffffff", dark="#000000"}
"graph.lanes.0" = "#abcdef"
"editor.background" = "#123456"
"##,
    )
    .unwrap();
    let themes = settings.resolved_theme();
    for shell in [&themes.light, &themes.dark] {
        let color = shell.get(Role::Surface(Surface::History, Part::Background));
        assert_eq!(color.color, Color::rgb(0xffffff));
        assert_eq!(color.origin, Origin::Override);
        assert_eq!(shell.get(Role::GraphLane(0)).color, Color::rgb(0xabcdef));
    }
    assert!(
        themes
            .light
            .warnings
            .iter()
            .any(|w| w.contains("history.foreground"))
    );
    assert!(themes.dark.warnings.is_empty());
}

#[test]
fn saturated_seeds_stay_readable_and_do_not_change_status_colors() {
    let original = Settings::default();
    for seed in [
        "#ff0000", "#00ff00", "#0000ff", "#ffffff", "#000000", "#f500ff",
    ] {
        let settings = Settings::parse(&format!(
            r##"
[theme.shell_auto]
seed = "{seed}"
chroma_scale = 3.0
hue_shift = {{light=-180.0, dark=180.0}}
"##
        ))
        .unwrap();
        for dark in [false, true] {
            let shell = settings.resolved_theme().shell(dark);
            assert!(shell.warnings.is_empty(), "{seed}: {:?}", shell.warnings);
            for role in [Role::Success, Role::Error, Role::GraphLane(0)] {
                assert_eq!(
                    shell.get(role).color,
                    original.resolved_theme().shell(dark).get(role).color
                );
            }
        }
    }
}

#[test]
fn mode_knobs_and_panel_adjustments_are_independent() {
    let original = Settings::default();
    let changed = Settings::parse(
        r##"
[theme.shell_auto]
layer_strength = {light=0.0, dark=1.4}
[theme.shell_auto.roles."shell.sidebar.files.background"]
lightness_shift = {light=0.0, dark=0.05}
"##,
    )
    .unwrap();
    assert_eq!(
        changed
            .resolved_theme()
            .light
            .surface(Surface::Files, Part::Background),
        changed
            .resolved_theme()
            .light
            .surface(Surface::Viewer, Part::Background)
    );
    assert_eq!(
        changed
            .resolved_theme()
            .dark
            .surface(Surface::History, Part::Background),
        original
            .resolved_theme()
            .dark
            .surface(Surface::History, Part::Background)
    );
    assert_ne!(
        changed
            .resolved_theme()
            .dark
            .surface(Surface::Files, Part::Background),
        original
            .resolved_theme()
            .dark
            .surface(Surface::Files, Part::Background)
    );
}

#[test]
fn disabled_generation_keeps_builtins_and_exact_overrides() {
    let settings = Settings::parse(
        r##"
[theme.shell_auto]
enabled=false
seed="#ff00ff"
[theme.overrides]
"shell.sidebar.files.background"="#123456"
"##,
    )
    .unwrap();
    assert_eq!(
        settings
            .resolved_theme()
            .dark
            .surface(Surface::History, Part::Background),
        Color::rgb(0x1e1e1e)
    );
    assert_eq!(
        settings
            .resolved_theme()
            .dark
            .surface(Surface::Files, Part::Background),
        Color::rgb(0x123456)
    );
}

#[test]
fn invalid_theme_config_is_rejected() {
    for input in [
        "[theme.shell_auto]\nalgorithm='tonal-v2'",
        "[theme.shell_auto]\nlayer_strength=2.6",
        "[theme.shell_auto]\nchroma_scale=nan",
        "[theme.shell_auto]\nseed='oops'",
        "[theme.overrides]\nunknown='#ffffff'",
        "[theme.shell_auto.roles.\"shell.foreground\"]\nlightness_shift=0.1",
    ] {
        assert!(Settings::parse(input).is_err(), "{input}");
    }
}

#[test]
fn soft_light_default_reverses_dark_layering_with_readable_surface_text() {
    let settings = Settings::default();
    assert_eq!(settings.theme.shell_auto.layer_strength.resolve(false), 1.4);
    assert_eq!(settings.theme.shell_auto.layer_strength.resolve(true), 1.4);
    let resolved = settings.resolved_theme();
    assert_eq!(
        resolved.light.surface(Surface::Viewer, Part::Background),
        Color::rgb(0xf7f7f6)
    );
    for surface in [
        Surface::History,
        Surface::Files,
        Surface::Header,
        Surface::HistoryHeader,
        Surface::FilesHeader,
    ] {
        let light_offset = resolved
            .light
            .surface(Surface::Viewer, Part::Background)
            .lch()[0]
            - resolved.light.surface(surface, Part::Background).lch()[0];
        let dark_offset = resolved.dark.surface(surface, Part::Background).lch()[0]
            - resolved
                .dark
                .surface(Surface::Viewer, Part::Background)
                .lch()[0];
        assert!(
            (light_offset - dark_offset).abs() < 0.004,
            "{surface:?}: light={light_offset}, dark={dark_offset}"
        );
    }
    for shell in [&resolved.light, &resolved.dark] {
        for surface in Surface::ALL {
            let background = shell.surface(surface, Part::Background);
            for part in [Part::Foreground, Part::MutedForeground] {
                assert!(
                    background.contrast(shell.surface(surface, part)) >= 4.5,
                    "{surface:?} {part:?}"
                );
            }
        }
    }
}
