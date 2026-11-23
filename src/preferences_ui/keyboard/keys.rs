//! Native keycaps and searchable aliases share GPUI's parsed modifier semantics.
use gpui::{Keystroke, OperatingSystem};
use std::collections::BTreeSet;

pub(super) fn parts(stroke: &Keystroke) -> Vec<Keystroke> {
    let mut keys = Vec::new();
    for (enabled, key) in [
        (stroke.modifiers.function, "fn"),
        (stroke.modifiers.control, "ctrl"),
        (stroke.modifiers.alt, "alt"),
        (stroke.modifiers.shift, "shift"),
        (stroke.modifiers.platform, "cmd"),
    ] {
        if enabled {
            keys.push(Keystroke {
                key: key.into(),
                ..Default::default()
            });
        }
    }
    keys.push(Keystroke {
        key: stroke.key.clone(),
        ..Default::default()
    });
    keys
}
fn canonical(term: &str) -> &str {
    match term {
        "cmd" | "command" | "super" | "meta" | "win" | "windows" | "⌘" => "platform",
        "alt" | "opt" | "option" | "⌥" => "alt",
        "ctrl" | "ctl" | "control" | "⌃" => "control",
        "shift" | "⇧" => "shift",
        "fn" | "function" | "globe" => "function",
        "secondary" | "cmdorctrl" => "secondary",
        "return" | "enter" | "⏎" | "↵" => "enter",
        "esc" | "escape" | "⎋" => "escape",
        "spacebar" | "space" => "space",
        "backspace" | "⌫" => "backspace",
        "del" | "delete" | "⌦" => "delete",
        "tab" | "⇥" => "tab",
        "pgup" | "pageup" => "pageup",
        "pgdn" | "pgdown" | "pagedown" => "pagedown",
        "arrowleft" | "leftarrow" | "left" | "←" => "left",
        "arrowright" | "rightarrow" | "right" | "→" => "right",
        "arrowup" | "uparrow" | "up" | "↑" => "up",
        "arrowdown" | "downarrow" | "down" | "↓" => "down",
        _ => term,
    }
}
fn terms(query: &str) -> Vec<String> {
    let mut text = String::new();
    for c in query.chars() {
        if "⌘⌥⌃⇧⏎↵⎋⌫⌦⇥←→↑↓".contains(c) {
            text.push(' ');
            text.push(c);
            text.push(' ');
        } else {
            text.push(c);
        }
    }
    text.split_whitespace()
        .flat_map(|term| {
            // Literal +/- keys remain searchable; separators in Cmd+K/Cmd-K split.
            if matches!(term, "+" | "-") {
                vec![term]
            } else {
                term.split(['+', '-'])
                    .filter(|part| !part.is_empty())
                    .collect()
            }
        })
        .map(|term| canonical(term).to_owned())
        .collect()
}
pub(super) fn matches(
    label: &str,
    key: &str,
    bindings: &[String],
    query: &str,
    os: OperatingSystem,
) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() || format!("{label} {key}").to_lowercase().contains(&query) {
        return true;
    }
    let wanted = terms(&query);
    bindings.iter().any(|binding| {
        if !matches!(query.as_str(), "+" | "-") && binding.to_lowercase().contains(&query) {
            return true;
        }
        if wanted.is_empty() {
            return false;
        }
        let mut names = BTreeSet::new();
        for raw in binding.split_whitespace() {
            let Ok(stroke) = Keystroke::parse_with_os(raw, os) else {
                return false;
            };
            for (enabled, name) in [
                (stroke.modifiers.function, "function"),
                (stroke.modifiers.control, "control"),
                (stroke.modifiers.alt, "alt"),
                (stroke.modifiers.shift, "shift"),
                (stroke.modifiers.platform, "platform"),
                (
                    if os == OperatingSystem::Mac {
                        stroke.modifiers.platform
                    } else {
                        stroke.modifiers.control
                    },
                    "secondary",
                ),
            ] {
                if enabled {
                    names.insert(name.to_owned());
                }
            }
            names.insert(canonical(&stroke.key).to_owned());
        }
        wanted.iter().all(|term| names.contains(term))
    })
}

#[cfg(test)]
mod tests {
    use super::{matches, parts};
    use gpui::{Keystroke, OperatingSystem};
    fn found(binding: &str, query: &str, os: OperatingSystem) -> bool {
        matches("Example", "example", &[binding.into()], query, os)
    }
    #[test]
    fn modifier_aliases_and_native_symbols_match_the_parsed_keys() {
        let mac = OperatingSystem::Mac;
        for query in [
            "cmd",
            "command",
            "secondary",
            "super",
            "win",
            "meta",
            "⌘",
            "command k",
            "cmd+k",
            "cmd-k",
        ] {
            assert!(found("secondary-k", query, mac), "{query}");
        }
        for query in [
            "alt", "opt", "option", "⌥", "control", "ctrl", "ctl", "⌃", "shift", "⇧",
        ] {
            assert!(found("ctrl-alt-shift-k", query, mac), "{query}");
        }
        assert!(found("cmd-k cmd-c", "command k command c", mac));
        assert!(found("cmd-shift-k", "⌘⇧K", mac));
        assert!(!found("secondary-k", "control", mac));
        assert!(!found("ctrl-k", "secondary", mac));
        assert!(!found("cmd-k", "cmd+p", mac));
    }
    #[test]
    fn secondary_tracks_the_native_platform_and_alternatives_stay_separate() {
        for os in [OperatingSystem::Linux, OperatingSystem::Windows] {
            assert!(found("secondary-k", "control", os));
            assert!(found("ctrl-k", "secondary", os));
            assert!(!found("secondary-k", "command", os));
        }
        assert!(!matches(
            "Example",
            "example",
            &["cmd-k".into(), "ctrl-j".into()],
            "cmd j",
            OperatingSystem::Mac
        ));
        assert!(matches(
            "Open repository",
            "open_repository",
            &[],
            "repository",
            OperatingSystem::Mac
        ));
    }
    #[test]
    fn named_keys_chords_and_literal_punctuation_are_searchable() {
        for (binding, query) in [
            ("alt-enter", "option return"),
            ("ctrl-escape", "control esc"),
            ("alt-left", "option leftarrow"),
            ("cmd-pageup", "command pgup"),
            ("fn-k", "function"),
            ("cmd-+", "+"),
            ("cmd--", "-"),
        ] {
            assert!(
                found(binding, query, OperatingSystem::Mac),
                "{binding}: {query}"
            );
        }
        assert!(!found("cmd-k", "+", OperatingSystem::Mac));
        assert!(!found("cmd-k", "-", OperatingSystem::Mac));
        let stroke =
            Keystroke::parse_with_os("ctrl-alt-shift-cmd-left", OperatingSystem::Mac).unwrap();
        let pieces = parts(&stroke);
        assert_eq!(
            pieces
                .iter()
                .map(|key| key.key.as_str())
                .collect::<Vec<_>>(),
            ["ctrl", "alt", "shift", "cmd", "left"]
        );
        assert!(pieces.iter().all(|key| key.modifiers == Default::default()));
        if cfg!(target_os = "macos") {
            assert_eq!(
                pieces
                    .iter()
                    .map(gpui::component::kbd::Kbd::format)
                    .collect::<Vec<_>>(),
                ["⌃", "⌥", "⇧", "⌘", "←"]
            );
        }
    }
}
