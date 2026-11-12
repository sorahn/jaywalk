//! Optional desktop preferences are sampled at app startup, never during paint.
use gpui::{App, Global};
use jaywalk::settings::ScrollbarClick;

struct SystemScrollbarClick(Option<ScrollbarClick>);
impl Global for SystemScrollbarClick {}

pub(crate) fn install_scrollbar_preference(preference: Option<ScrollbarClick>, cx: &mut App) {
    cx.set_global(SystemScrollbarClick(preference));
}
pub(crate) fn system_scrollbar_click(cx: &App) -> Option<ScrollbarClick> {
    cx.try_global::<SystemScrollbarClick>()
        .and_then(|preference| preference.0)
}

pub(crate) fn read_scrollbar_preference() -> Option<ScrollbarClick> {
    #[cfg(target_os = "macos")]
    {
        use core_foundation::{base::TCFType, string::CFString};
        use core_foundation_sys::preferences::{
            CFPreferencesGetAppBooleanValue, kCFPreferencesCurrentApplication,
        };
        // WebKit uses this same AppKit preference:
        // https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/mac/ScrollbarThemeMac.mm
        let key = CFString::new("AppleScrollerPagingBehavior");
        let mut valid = 0;
        // SAFETY: key remains alive for the call; the application constant is
        // supplied by CoreFoundation and valid is a writable Boolean out-param.
        let jump = unsafe {
            CFPreferencesGetAppBooleanValue(
                key.as_concrete_TypeRef(),
                kCFPreferencesCurrentApplication,
                &mut valid,
            )
        };
        (valid != 0).then_some(if jump != 0 {
            ScrollbarClick::Jump
        } else {
            ScrollbarClick::Page
        })
    }
    #[cfg(target_os = "linux")]
    {
        let mut roots = Vec::new();
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
            roots.push(std::path::PathBuf::from(config));
        } else if let Some(home) = std::env::var_os("HOME") {
            roots.push(std::path::PathBuf::from(home).join(".config"));
        }
        roots.extend(std::env::split_paths(
            &std::env::var_os("XDG_CONFIG_DIRS")
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| "/etc/xdg".into()),
        ));
        // GTK also loads system defaults from /etc/gtk-{3,4}.0/settings.ini.
        roots.push(std::path::PathBuf::from("/etc"));
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| std::env::var("XDG_SESSION_DESKTOP"))
            .unwrap_or_default();
        linux_preference(&roots, &desktop)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
fn ini_bool(contents: &str, section: &str, key: &str) -> Option<bool> {
    let mut active = false;
    let mut value = None;
    for line in contents.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(group) = line.strip_prefix('[').and_then(|s| s.split_once(']')) {
            active = group.0 == section;
        } else if active
            && let Some((name, text)) = line.split_once('=')
            && name.split('[').next().unwrap_or_default().trim() == key
        {
            value = match text.trim() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            };
        }
    }
    value
}

#[cfg(any(target_os = "linux", test))]
fn linux_preference(roots: &[std::path::PathBuf], desktop: &str) -> Option<ScrollbarClick> {
    use std::io::Read;
    let read = |path: std::path::PathBuf, section, key| {
        // These are optional capabilities. Unreadable, absent or malformed
        // preferences deliberately yield None, which the caller maps to jump.
        let mut contents = String::new();
        std::fs::File::open(path)
            .ok()?
            .take(256 * 1024)
            .read_to_string(&mut contents)
            .ok()?;
        ini_bool(&contents, section, key)
    };
    if desktop
        .to_ascii_uppercase()
        .split(':')
        .any(|name| name == "KDE")
    {
        // https://github.com/KDE/plasma-desktop/blob/master/kcms/workspaceoptions/workspaceoptions_kdeglobalssettings.kcfg
        for root in roots {
            if let Some(page) = read(
                root.join("kdeglobals"),
                "KDE",
                "ScrollbarLeftClickNavigatesByPage",
            ) {
                return Some(if page {
                    ScrollbarClick::Page
                } else {
                    ScrollbarClick::Jump
                });
            }
        }
        return None;
    }
    // https://docs.gtk.org/gtk4/property.Settings.gtk-primary-button-warps-slider.html
    for root in roots {
        for gtk in ["gtk-4.0", "gtk-3.0"] {
            if let Some(jump) = read(
                root.join(gtk).join("settings.ini"),
                "Settings",
                "gtk-primary-button-warps-slider",
            ) {
                return Some(if jump {
                    ScrollbarClick::Jump
                } else {
                    ScrollbarClick::Page
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{ini_bool, linux_preference};
    use jaywalk::settings::{ScrollbarClick, Settings};
    #[test]
    fn overrides_and_system_fallback_are_explicit() {
        assert_eq!(
            Settings::default().panels.diff.shared_scrollbar_click,
            ScrollbarClick::System
        );
        assert_eq!(ScrollbarClick::System.resolve(None), ScrollbarClick::Jump);
        assert_eq!(
            ScrollbarClick::System.resolve(Some(ScrollbarClick::Page)),
            ScrollbarClick::Page
        );
        for (value, mode) in [
            ("jump", ScrollbarClick::Jump),
            ("page", ScrollbarClick::Page),
        ] {
            let settings =
                Settings::parse(&format!("[panels.diff]\nshared_scrollbar_click='{value}'"))
                    .unwrap();
            assert_eq!(settings.panels.diff.shared_scrollbar_click, mode);
            assert_eq!(mode.resolve(Some(ScrollbarClick::System)), mode);
        }
    }
    #[test]
    fn desktop_ini_values_are_scoped_and_missing_is_unknown() {
        assert_eq!(ini_bool("[Other]\nkey=true\n", "Settings", "key"), None);
        assert_eq!(
            ini_bool("[Settings]\nkey=false\nkey=1\n", "Settings", "key"),
            Some(true)
        );
        assert_eq!(
            ini_bool("[KDE][$i]\nkey[$i]=0\n", "KDE", "key"),
            Some(false)
        );
        assert_eq!(
            ini_bool("[Settings]\nkey=unknown\n", "Settings", "key"),
            None
        );
    }
    #[test]
    fn active_desktop_and_user_files_take_precedence() {
        let root = std::env::temp_dir().join(format!(
            "jaywalk-scrollbar-preferences-{}",
            std::process::id()
        ));
        let user = root.join("user");
        let system = root.join("system");
        for path in [&user, &system] {
            std::fs::create_dir_all(path.join("gtk-4.0")).unwrap();
        }
        std::fs::write(
            user.join("kdeglobals"),
            "[KDE]\nScrollbarLeftClickNavigatesByPage=true\n",
        )
        .unwrap();
        std::fs::write(
            user.join("gtk-4.0/settings.ini"),
            "[Settings]\ngtk-primary-button-warps-slider=true\n",
        )
        .unwrap();
        std::fs::write(
            system.join("gtk-4.0/settings.ini"),
            "[Settings]\ngtk-primary-button-warps-slider=false\n",
        )
        .unwrap();
        let roots = [user, system];
        assert_eq!(linux_preference(&roots, "KDE"), Some(ScrollbarClick::Page));
        assert_eq!(
            linux_preference(&roots, "GNOME"),
            Some(ScrollbarClick::Jump)
        );
        assert_eq!(linux_preference(&[], "GNOME"), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
