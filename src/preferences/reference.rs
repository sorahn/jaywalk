//! Generated references share descriptions/examples with Settings and values with
//! the typed default document. Generated files are shipped, never rewritten in a
//! user's configuration directory.
use super::{Snapshot, catalogue, literal};
use catalogue::{Category, NumberBound, Setting};
use std::fmt::Write as _;

const INTRO: &str = "# Jaywalk settings. Saved edits apply automatically.\n\
# Generated from config/default.toml and config/settings-ui.toml.\n\
# Uncomment only values you want to override; section headers stay active.\n\
# Omitted values inherit current defaults, including after updates.\n\
# Dimensions are logical pixels unless noted. Colors accept a shared \"#RRGGBB\"\n\
# or { light = \"#RRGGBB\", dark = \"#RRGGBB\" }. Keybindings have their own file.\n\
# This file's existence skips automatic Setup; Set up Jaywalk remains in the\n\
# command palette. Creating it authorizes no network requests.\n\
# Account metadata is managed as [[accounts]]; credentials stay in the OS store.\n\n";

fn notes(setting: &Setting) -> Vec<String> {
    let mut notes = vec![setting.description.clone()];
    if !setting.choices.is_empty() {
        notes.push(format!("Choices: {}.", setting.choices.join(", ")));
    }
    let bound = |value: &NumberBound| match value {
        NumberBound::Value(value) => value.to_string(),
        NumberBound::Setting(key) => key.clone(),
    };
    match (&setting.minimum, &setting.maximum) {
        (Some(min), Some(max)) => notes.push(format!("Range: {} to {}.", bound(min), bound(max))),
        (Some(min), None) => notes.push(format!("Minimum: {}.", bound(min))),
        (None, Some(max)) => notes.push(format!("Maximum: {}.", bound(max))),
        _ => {}
    }
    notes
}

fn comments(text: &str, output: &mut String) {
    let mut width = 0;
    for word in text.split_whitespace() {
        let length = word.chars().count();
        if width == 0 {
            output.push_str("# ");
        } else if width + 1 + length > 78 {
            output.push_str("\n# ");
            width = 0;
        } else {
            output.push(' ');
            width += 1;
        }
        output.push_str(word);
        width += length;
    }
    output.push('\n');
}

fn describe(key: &str, output: &mut String) {
    if let Some(setting) = catalogue::setting(key) {
        for note in notes(setting) {
            comments(&note, output);
        }
    } else {
        // A paired seed is stored as one value but has separate UI controls.
        for setting in catalogue::SETTINGS.iter().filter(|setting| {
            setting
                .key
                .strip_prefix(key)
                .is_some_and(|tail| tail.starts_with('.'))
        }) {
            comments(
                &format!("{}: {}", setting.title, setting.description),
                output,
            );
        }
    }
}

fn table(path: &str, source: &toml_edit::Table, output: &mut String) {
    if !path.is_empty() && !source.is_implicit() {
        writeln!(output, "[{path}]").unwrap();
        if catalogue::setting(path).is_some() {
            describe(path, output);
        }
    }
    for (name, item) in source.iter() {
        if let Some(value) = item.as_value() {
            let key = if path.is_empty() {
                name.into()
            } else {
                format!("{path}.{name}")
            };
            describe(&key, output);
            let mut value = value.clone();
            value.decor_mut().clear();
            writeln!(output, "# {name} = {value}\n").unwrap();
        }
    }
    // Optional settings are inherited, not extra built-in defaults. Their
    // examples live in the same metadata used by the Settings controls.
    for setting in catalogue::SETTINGS.iter().filter(|setting| {
        setting
            .key
            .rsplit_once('.')
            .is_some_and(|(parent, name)| parent == path && !source.contains_key(name))
    }) {
        if !setting.examples.is_empty() {
            describe(&setting.key, output);
            comments("Optional example; omitted by default.", output);
            let name = setting.key.rsplit_once('.').unwrap().1;
            for value in &setting.examples {
                writeln!(output, "# {name} = {value}").unwrap();
            }
            output.push('\n');
        }
    }
    for (name, item) in source.iter() {
        if let Some(child) = item.as_table() {
            let key = if path.is_empty() {
                name.into()
            } else {
                format!("{path}.{name}")
            };
            table(&key, child, output);
        }
    }
}

/// Build the checked-in first-open artifact; no preference or credential access.
pub fn user_config() -> String {
    let source: toml_edit::DocumentMut = crate::settings::DEFAULT_CONFIG
        .parse()
        .expect("shipped defaults are validated in tests");
    let mut output = INTRO.to_owned();
    table("", source.as_table(), &mut output);
    format!("{}\n", output.trim_end())
}

fn default_text(setting: &Setting) -> String {
    match Snapshot::default_value(&setting.key) {
        Some(toml::Value::Table(table)) if table.len() > 2 => "See child settings".into(),
        Some(value) => format!("`{}`", literal(value)),
        None => "Automatic / inherited (unset)".into(),
    }
}

/// Build the complete key reference, keeping the prose usage guide hand-written.
pub fn markdown() -> String {
    let mut output = String::from(
        "# Generated settings reference\n\n\
<!-- Generated by `just settings-reference`; do not edit by hand. -->\n\n\
Values come from `config/default.toml`; descriptions, choices, ranges and optional\n\
examples come from `config/settings-ui.toml`, shared with Settings. For configuration\n\
sources, live previews, scope and detailed examples, see [the settings guide](settings.md).\n\n\
Keys below are fully qualified. In TOML, put the short key under its section,\n\
for example `[editor]` followed by `font_size = 14`. Optional examples are not defaults.\n\n",
    );
    for category in Category::ALL {
        writeln!(output, "## {}\n", category.label()).unwrap();
        let mut rows: Vec<_> = catalogue::SETTINGS
            .iter()
            .filter(|row| row.category == category)
            .collect();
        rows.sort_by(|a, b| (a.section_order, &a.key).cmp(&(b.section_order, &b.key)));
        for setting in rows {
            writeln!(
                output,
                "### `{}`\n\n**{}** · {}\n\nDefault: {}\n",
                setting.key,
                setting.title,
                setting.section,
                default_text(setting)
            )
            .unwrap();
            for note in notes(setting) {
                writeln!(output, "{note}\n").unwrap();
            }
            if setting.toml_only {
                output.push_str("Editing: TOML only. Search Settings for this key to see its description and Edit TOML action.\n\n");
            }
            if !setting.examples.is_empty() {
                let name = setting
                    .key
                    .rsplit_once('.')
                    .map_or(setting.key.as_str(), |(_, name)| name);
                output.push_str("Optional examples (choose one):\n\n```toml\n");
                if let Some((section, _)) = setting.key.rsplit_once('.') {
                    writeln!(output, "[{section}]").unwrap();
                }
                for example in &setting.examples {
                    writeln!(output, "# {name} = {example}").unwrap();
                }
                output.push_str("```\n\n");
            }
        }
    }
    format!("{}\n", output.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_in_artifacts_match_shared_metadata() {
        assert_eq!(
            user_config(),
            include_str!("../../config/user.toml"),
            "run just settings-reference"
        );
        assert_eq!(
            markdown(),
            include_str!("../../docs/settings-reference.md"),
            "run just settings-reference"
        );
    }
    #[test]
    fn uncommenting_default_values_reproduces_the_complete_default_document() {
        let mut section = String::new();
        let mut uncommented = String::new();
        for line in user_config().lines() {
            if line.starts_with('[') {
                section = line.trim_matches(['[', ']']).to_owned();
                writeln!(uncommented, "{line}").unwrap();
            } else if let Some(example) = line.strip_prefix("# ")
                && let Ok(values) = toml::from_str::<toml::Table>(example)
                && let Some(name) = values.keys().next()
            {
                let key = if section.is_empty() {
                    name.clone()
                } else {
                    format!("{section}.{name}")
                };
                if Snapshot::default_value(&key).is_some() {
                    writeln!(uncommented, "{example}").unwrap();
                }
            }
        }
        let exported: toml::Value = toml::from_str(&uncommented).unwrap();
        let defaults: toml::Value = toml::from_str(crate::settings::DEFAULT_CONFIG).unwrap();
        assert_eq!(
            exported, defaults,
            "the generated file must document every built-in value exactly"
        );
    }

    #[test]
    fn template_is_inherited_and_optional_examples_validate() {
        let template = user_config();
        assert_eq!(
            crate::settings::Config::parse(&template).unwrap(),
            crate::settings::Config::default()
        );
        assert!(
            template
                .lines()
                .all(|line| line.is_empty() || line.starts_with('#') || line.starts_with('['))
        );
        for setting in catalogue::SETTINGS.iter() {
            for example in &setting.examples {
                let doc = format!("{} = {example}", setting.key);
                crate::settings::Config::parse(&doc)
                    .unwrap_or_else(|error| panic!("{}: {error}", setting.key));
                assert!(
                    Snapshot::default_value(&setting.key).is_none(),
                    "examples should not mask defaults: {}",
                    setting.key
                );
                assert!(template.contains(&format!(
                    "# {} = {example}",
                    setting.key.rsplit_once('.').unwrap().1
                )));
            }
        }
    }
}
