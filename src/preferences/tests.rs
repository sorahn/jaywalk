use super::*;
use std::path::Path;
fn source(path: &Path) -> SettingsSource {
    SettingsSource::resolve(Some(path)).unwrap()
}
fn fixture(text: &str) -> (tempfile::TempDir, SettingsSource) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    let source = source(&path);
    source.initialize().unwrap();
    (root, source)
}
#[test]
fn catalogue_covers_defaults_and_searches_key_title_and_description() {
    fn visit(value: &toml::Value, prefix: String) {
        if let Some(table) = value.as_table()
            && !table.is_empty()
            && !(table.len() == 2 && table.contains_key("light") && table.contains_key("dark"))
        {
            for (key, value) in table {
                visit(
                    value,
                    if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    },
                );
            }
        } else {
            assert!(catalogue::setting(&prefix).is_some(), "missing {prefix}");
        }
    }
    visit(&DEFAULTS, String::new());
    assert!(
        catalogue::setting("ui.appearance")
            .unwrap()
            .rank("always")
            .is_some(),
        "summary-only words must remain searchable"
    );
    assert!(
        catalogue::setting("ui.accent_color")
            .unwrap()
            .rank("appkit")
            .is_some(),
        "full descriptions must remain searchable after adding a summary"
    );
    let mut keys = std::collections::HashSet::new();
    for setting in catalogue::SETTINGS.iter() {
        assert!(keys.insert(&setting.key));
        assert!(!setting.description.is_empty());
        assert!(setting.rank(&setting.key).is_some());
    }
    let (_root, source) = fixture("");
    let snapshot = source.read_snapshot().unwrap();
    assert_eq!(
        snapshot.matching("diff.before.word_wrap", None, false)[0],
        catalogue::SETTINGS
            .iter()
            .position(|s| s.key == "diff.before.word_wrap")
            .unwrap()
    );
    assert!(
        snapshot
            .matching("word wrap", None, false)
            .iter()
            .any(|index| catalogue::SETTINGS[*index].key == "diff.after.word_wrap")
    );

    for query in ["", "accounts", "credential"] {
        assert!(
            snapshot
                .matching(query, None, false)
                .iter()
                .all(|index| catalogue::SETTINGS[*index].key != "accounts")
        );
    }
}
#[test]
fn snapshots_distinguish_inherited_explicit_default_and_modified_without_runtime_state() {
    let (_root, source) = fixture(
        "[ui]\nappearance='system'\n[editor]\nfont_size=13.0\n[diff]\nword_highlight=false\n",
    );
    let snapshot = source.read_snapshot().unwrap();
    assert!(snapshot.explicit("ui.appearance").is_some());
    assert!(!snapshot.modified(catalogue::setting("ui.appearance").unwrap()));
    assert!(!snapshot.modified(catalogue::setting("editor.font_size").unwrap()));
    assert!(snapshot.modified(catalogue::setting("diff.word_highlight").unwrap()));
    assert!(snapshot.explicit("diff.algorithm").is_none());
}
#[test]
fn edits_merge_latest_fields_reject_stale_same_field_and_preserve_comments_and_symlinks() {
    let (_root, source) = fixture(
        "# keep\n[ui]\nappearance='system' # appearance note\n[diff]\nword_highlight=true\n",
    );
    let old = source.read_snapshot().unwrap();
    patch(&source, Patch::set("ui.appearance", "dark".into())).unwrap();
    patch(&source, Patch::set("diff.word_highlight", false.into())).unwrap();
    let stale = Patch {
        key: "ui.appearance".into(),
        value: Some("light".into()),
        expected: Expect::Explicit(old.explicit("ui.appearance").cloned()),
    };
    assert!(
        patch(&source, stale)
            .unwrap_err()
            .contains("changed elsewhere")
    );
    let current = source.read_snapshot().unwrap();
    assert_eq!(
        current.value("ui.appearance").unwrap().as_str(),
        Some("dark")
    );
    assert_eq!(
        current.value("diff.word_highlight").unwrap().as_bool(),
        Some(false)
    );
    assert!(current.text.contains("# appearance note"));
    #[cfg(unix)]
    {
        let alias = source.path().with_file_name("alias.toml");
        std::os::unix::fs::symlink(source.path(), &alias).unwrap();
        patch(
            &self::source(&alias),
            Patch::set("ui.appearance", "light".into()),
        )
        .unwrap();
        assert!(alias.is_symlink());
    }
}
#[test]
fn reset_inherits_future_defaults_keeps_siblings_and_comments_and_refuses_invalid_files() {
    let (_root, source) = fixture(
        "ui = { appearance = 'dark', font_size = 18 }\n[diff]\nword_highlight=false # keep this\n",
    );
    patch(
        &source,
        Patch {
            key: "ui.appearance".into(),
            value: None,
            expected: Expect::Any,
        },
    )
    .unwrap();
    let current = patch(
        &source,
        Patch {
            key: "diff.word_highlight".into(),
            value: None,
            expected: Expect::Any,
        },
    )
    .unwrap();
    assert_eq!(
        current.settings.ui.appearance,
        crate::settings::Appearance::System
    );
    assert_eq!(current.settings.ui.font_size, 18.);
    assert!(current.explicit("ui.appearance").is_none());
    assert!(current.text.contains("# keep this"));
    assert!(current.explicit("diff.word_highlight").is_none());
    std::fs::write(source.path(), "invalid=[").unwrap();
    assert!(patch(&source, Patch::set("ui.appearance", "dark".into())).is_err());
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), "invalid=[");
}
#[test]
fn update_prerequisites_are_atomic_and_invalid_numeric_drafts_never_reach_disk() {
    let (_root, source) = fixture(
        "[updates]\nautomatic_checks=true\nautomatic_downloads=true\nautomatic_installation=true\n",
    );
    patch(
        &source,
        Patch::set("updates.automatic_checks", false.into()),
    )
    .unwrap();
    patch(
        &source,
        Patch::set("updates.automatic_downloads", false.into()),
    )
    .unwrap();
    let current = source.read_snapshot().unwrap();
    assert!(!current.settings.updates.automatic_checks);
    assert!(!current.settings.updates.automatic_downloads);
    assert!(!current.settings.updates.automatic_installation);
    assert!(
        patch(
            &source,
            Patch::set("updates.automatic_downloads", true.into())
        )
        .is_err()
    );
    assert!(patch(&source, Patch::set("ui.font_size", (-100).into())).is_err());
    assert_eq!(source.read_snapshot().unwrap().text, current.text);
}

#[test]
fn table_and_multiline_resets_keep_following_sections_and_read_only_files_are_unchanged() {
    let (_root, source) = fixture(
        "[history.remotes.overrides]\norigin=true # keep origin note\nupstream=false\n[ui]\nappearance='light'\n",
    );
    let reset = patch(
        &source,
        Patch {
            key: "history.remotes.overrides".into(),
            value: None,
            expected: Expect::Any,
        },
    )
    .unwrap();
    assert!(reset.explicit("history.remotes.overrides").is_none());
    assert!(reset.text.contains("# keep origin note"));
    assert_eq!(
        reset.settings.ui.appearance,
        crate::settings::Appearance::Light
    );
    let before = std::fs::read_to_string(source.path()).unwrap();
    let permissions = std::fs::metadata(source.path()).unwrap().permissions();
    let mut read_only = permissions.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(source.path(), read_only).unwrap();
    let result = patch(&source, Patch::set("ui.appearance", "dark".into()));
    std::fs::set_permissions(source.path(), permissions).unwrap();
    assert!(result.unwrap_err().contains("read-only"));
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), before);
}

#[test]
fn numeric_controls_keep_typed_ranges_and_dynamic_limits() {
    let (_root, source) =
        fixture("[panels.history]\nmin_width=100\nmax_width=200\ndefault_width=150\n");
    let snapshot = source.read_snapshot().unwrap();
    for setting in catalogue::SETTINGS
        .iter()
        .filter(|setting| setting.kind == Kind::Number)
    {
        assert!(
            setting
                .step
                .is_some_and(|step| step.is_finite() && step > 0.),
            "{}",
            setting.key
        );
    }
    // Every shipped numeric control must produce a schema-valid first step
    // from the canonical defaults; UI hints never replace schema validation.
    let (_defaults_root, defaults_source) = fixture("");
    let defaults = defaults_source.read_snapshot().unwrap();
    for setting in catalogue::SETTINGS
        .iter()
        .filter(|setting| setting.kind == Kind::Number)
    {
        let initial = defaults
            .value(&setting.key)
            .map(|value| display(Some(value)))
            .unwrap_or_default();
        let stepped = setting.stepped(&initial, true, &defaults).unwrap();
        let mut doc: toml_edit::DocumentMut = DEFAULT_CONFIG.parse().unwrap();
        set(&mut doc, &setting.key, &stepped).unwrap();
        assert!(
            Config::parse(&doc.to_string()).is_ok(),
            "invalid first numeric step for {}",
            setting.key
        );
    }
    let font = catalogue::setting("ui.font_size").unwrap();
    assert_eq!(
        font.stepped("8", false, &snapshot).unwrap(),
        toml::Value::Integer(8)
    );
    assert_eq!(
        font.stepped("48", true, &snapshot).unwrap(),
        toml::Value::Integer(48)
    );
    assert_eq!(
        font.stepped("16", true, &snapshot).unwrap(),
        toml::Value::Float(16.5)
    );
    assert!(font.stepped("nan", true, &snapshot).is_err());
    let width = catalogue::setting("panels.history.default_width").unwrap();
    assert_eq!(
        width.stepped("200", true, &snapshot).unwrap(),
        toml::Value::Integer(200)
    );
    assert_eq!(
        width.stepped("100", false, &snapshot).unwrap(),
        toml::Value::Integer(100)
    );
    let opacity = catalogue::setting("panels.diff.shared_scrollbar_opacity").unwrap();
    assert_eq!(
        opacity.stepped("0.99", true, &snapshot).unwrap(),
        toml::Value::Integer(1)
    );
}

#[test]
fn shell_color_modes_preserve_shared_other_mode_and_reset_to_inheritance() {
    let (_root, source) = fixture("[theme.shell_auto]\nseed='#223344' # keep seed note\n");
    let before = source.read_snapshot().unwrap();
    assert_eq!(
        before
            .value("theme.shell_auto.seed.light")
            .unwrap()
            .as_str(),
        Some("#223344")
    );
    let changed = patch(
        &source,
        Patch::set("theme.shell_auto.seed.light", "#445566".into()),
    )
    .unwrap();
    assert_eq!(
        changed
            .value("theme.shell_auto.seed.dark")
            .unwrap()
            .as_str(),
        Some("#223344")
    );
    assert_eq!(
        changed
            .value("theme.shell_auto.seed.light")
            .unwrap()
            .as_str(),
        Some("#445566")
    );
    assert!(changed.text.contains("# keep seed note"));
    let reset = patch(
        &source,
        Patch {
            key: "theme.shell_auto.seed.light".into(),
            value: None,
            expected: Expect::Any,
        },
    )
    .unwrap();
    assert!(reset.explicit("theme.shell_auto.seed.light").is_none());
    assert_eq!(
        reset.value("theme.shell_auto.seed.light"),
        Snapshot::default_value("theme.shell_auto.seed.light")
    );
    assert_eq!(
        reset.value("theme.shell_auto.seed.dark").unwrap().as_str(),
        Some("#223344")
    );
}

#[test]
fn primary_colors_are_visible_and_advanced_diff_colors_have_one_entry() {
    let (_root, source) =
        fixture("[diff.colors]\nword_added='#112233'\nadded_background='#223344'\n");
    let snapshot = source.read_snapshot().unwrap();
    let keys = snapshot
        .matching("", None, false)
        .into_iter()
        .map(|index| catalogue::SETTINGS[index].key.as_str())
        .collect::<Vec<_>>();
    assert!(keys.contains(&"theme.shell_auto.seed.light"));
    assert!(keys.contains(&"theme.shell_auto.seed.dark"));
    assert!(keys.contains(&"ui.accent_color"));
    assert!(keys.contains(&"diff.colors.added_background"));
    assert!(keys.contains(&"diff.colors"));
    assert!(!keys.contains(&"diff.colors.word_added"));
    assert!(!keys.contains(&"ui.accent_color_fallback"));
    assert!(snapshot.modified(catalogue::setting("diff.colors").unwrap()));
    assert_eq!(
        catalogue::SETTINGS[snapshot.matching("diff.colors.word_added", None, false)[0]].key,
        "diff.colors"
    );
    assert_eq!(
        catalogue::SETTINGS[snapshot.matching("ui.accent_color_fallback", None, false)[0]].key,
        "theme.overrides"
    );
    let fallback = patch(
        &source,
        Patch::set("ui.accent_color_fallback", "#445566".into()),
    )
    .unwrap();
    assert!(fallback.modified(catalogue::setting("theme.overrides").unwrap()));
}

#[test]
fn draft_batch_merges_unrelated_edits_and_rejects_conflicts_without_partial_writes() {
    let (_root, source) = fixture("# keep\n[ui]\nfont_size=16\n");
    let before = source.read_snapshot().unwrap();
    let patches = [
        Patch {
            key: "diff.word_highlight".into(),
            value: Some(false.into()),
            expected: Expect::Explicit(None),
        },
        Patch {
            key: "ui.font_size".into(),
            value: Some(18.into()),
            expected: Expect::Explicit(Some(16.into())),
        },
    ];
    let draft = before.preview(&patches).unwrap();
    assert!(!draft.settings.diff.word_highlight);
    assert_eq!(draft.settings.ui.font_size, 18.);
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), before.text);
    patch(&source, Patch::set("files.default_view", "tree".into())).unwrap();
    let saved = patch_many(&source, &patches).unwrap();
    assert_eq!(
        saved.value("files.default_view").unwrap().as_str(),
        Some("tree")
    );
    assert!(saved.text.contains("# keep"));
    assert!(!saved.settings.diff.word_highlight);
    let expected = saved.text.clone();
    assert!(
        patch_many(&source, &patches)
            .unwrap_err()
            .contains("changed elsewhere")
    );
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), expected);
    let invalid = [
        Patch::set("diff.word_highlight", true.into()),
        Patch::set("ui.font_size", (-1).into()),
    ];
    assert!(patch_many(&source, &invalid).is_err());
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), expected);
}

#[test]
fn draft_update_consent_changes_have_no_effect_until_the_atomic_save() {
    let (_root, source) = fixture("");
    let before = source.read_snapshot().unwrap();
    let patches = [
        Patch::set("updates.automatic_checks", true.into()),
        Patch::set("updates.automatic_downloads", true.into()),
    ];
    let draft = before.preview(&patches).unwrap();
    assert!(draft.settings.updates.automatic_checks);
    assert!(!source.read().unwrap().updates.automatic_checks);
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), before.text);
    assert!(
        patch_many(&source, &patches)
            .unwrap()
            .settings
            .updates
            .automatic_downloads
    );
}

#[test]
fn keyboard_and_settings_save_validates_all_documents_before_any_write() {
    use crate::{commands::CommandId, keybinding_profiles::Draft};
    let (_dir, source) = fixture("[editor]\nfont_size=14\n");
    let before = source.read_snapshot().unwrap();
    let name = before.settings.keyboard.profile.clone();
    let draft = Draft::load(source.path(), &name).unwrap();
    let edited = draft
        .remap(CommandId::FindFile, Some(&["alt-shift-j".into()]))
        .unwrap();
    let patch = Patch::set("editor.font_size", 18.into());
    std::fs::write(&draft.path, format!("{}# external\n", draft.original)).unwrap();
    assert!(
        save(
            &source,
            std::slice::from_ref(&patch),
            std::slice::from_ref(&edited)
        )
        .unwrap_err()
        .contains("changed elsewhere")
    );
    assert_eq!(std::fs::read_to_string(source.path()).unwrap(), before.text);
    std::fs::write(&draft.path, &draft.original).unwrap();
    let saved = save(&source, &[patch], &[edited]).unwrap();
    assert_eq!(saved.settings.editor.font_size, 18.);
    assert_eq!(
        saved.settings.keybindings[&CommandId::FindFile],
        ["alt-shift-j"]
    );
    assert!(source.read_snapshot().unwrap().settings == saved.settings);
}

#[test]
fn unreadable_or_read_only_later_document_cannot_partly_save_profiles() {
    use crate::{commands::CommandId, keybinding_profiles::Draft};
    let (_dir, source) = fixture("");
    let saved = source.read_snapshot().unwrap();
    let draft = Draft::load(source.path(), &saved.settings.keyboard.profile).unwrap();
    let edit = draft
        .remap(CommandId::FindFile, Some(&["alt-shift-j".into()]))
        .unwrap();
    let mut permissions = std::fs::metadata(source.path()).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(source.path(), permissions).unwrap();
    assert!(
        save(
            &source,
            &[Patch::set("editor.font_size", 18.into())],
            &[edit]
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(&draft.path).unwrap(),
        draft.original
    );
}

#[test]
fn toml_only_tuning_stays_searchable_and_modified_without_cluttering_browsing() {
    let (_root, source) = fixture("[ui]\nloading_delay_ms=450\n[window]\nwidth=1600\n");
    let snapshot = source.read_snapshot().unwrap();
    let ordinary = snapshot.matching("", None, false);
    let changed = snapshot.matching("", None, true);
    for (index, setting) in catalogue::SETTINGS
        .iter()
        .enumerate()
        .filter(|(_, setting)| setting.toml_only)
    {
        assert!(
            !ordinary.contains(&index),
            "{} appears in ordinary browsing",
            setting.key
        );
        assert!(
            snapshot
                .matching(&setting.key, None, false)
                .contains(&index),
            "{} must remain discoverable",
            setting.key
        );
        assert_eq!(
            changed.contains(&index),
            snapshot.modified(setting),
            "{} changed filter",
            setting.key
        );
    }
    for key in ["ui.loading_delay_ms", "window.width"] {
        let index = catalogue::SETTINGS
            .iter()
            .position(|setting| setting.key == key)
            .unwrap();
        assert!(changed.contains(&index));
    }
    for key in [
        "ui.font_size",
        "ui.row_height",
        "ui.spacing.unit",
        "diff.context_lines",
        "panels.diff.gutter_width",
        "history.page_size",
        "hosting.max_content_mb",
        "local_review.max_repository_mb",
        "theme.shell_auto.layer_strength",
    ] {
        let index = catalogue::SETTINGS
            .iter()
            .position(|setting| setting.key == key)
            .unwrap();
        assert!(ordinary.contains(&index), "keep a normal control for {key}");
    }
    assert_eq!(
        std::fs::read_to_string(source.path()).unwrap(),
        "[ui]\nloading_delay_ms=450\n[window]\nwidth=1600\n"
    );
}

#[test]
fn focused_editors_preserve_literal_arguments_order_dotted_remote_names_and_comments() {
    let (_root, source) = fixture(
        "# config note\n[keyboard]\nadditional_directories = [\n  'first folder', # first note\n  'second folder', # second note\n]\n[settings]\neditor = ['code', '--wait'] # editor note\n[history.remotes.overrides]\n'backup.eu' = true # remote note\norigin = false\n",
    );
    let folders = toml::Value::Array(vec!["second folder".into(), "first folder".into()]);
    let editor = toml::Value::Array(vec![
        "editor with spaces".into(),
        "--reuse-window".into(),
        "$(literal); no shell".into(),
        "".into(),
    ]);
    let remotes = toml::Value::Table(toml::Table::from_iter([
        ("backup.eu".into(), true.into()),
        ("origin".into(), true.into()),
    ]));
    let saved = save(
        &source,
        &[
            Patch::set("keyboard.additional_directories", folders),
            Patch::set("settings.editor", editor),
            Patch::set("history.remotes.overrides", remotes),
        ],
        &[],
    )
    .unwrap();
    for note in [
        "# config note",
        "# first note",
        "# second note",
        "# editor note",
        "# remote note",
    ] {
        assert!(saved.text.contains(note), "lost {note}");
    }
    assert_eq!(
        saved.settings.keyboard.additional_directories,
        [
            std::path::PathBuf::from("second folder"),
            std::path::PathBuf::from("first folder")
        ]
    );
    assert_eq!(
        saved.settings.settings.editor,
        Some(crate::settings::SettingsEditor::Command(vec![
            "editor with spaces".into(),
            "--reuse-window".into(),
            "$(literal); no shell".into(),
            "".into()
        ]))
    );
    assert!(saved.settings.history.remotes.overrides["backup.eu"]);
    assert!(saved.settings.history.remotes.overrides["origin"]);
    let latest = source.read_snapshot().unwrap();
    std::fs::write(
        source.path(),
        saved.text.replace("origin = true", "origin = false"),
    )
    .unwrap();
    let stale = Patch {
        key: "history.remotes.overrides".into(),
        value: Some(toml::Value::Table(toml::Table::new())),
        expected: Expect::Explicit(latest.explicit("history.remotes.overrides").cloned()),
    };
    assert!(
        save(&source, &[stale], &[]).is_err(),
        "a stale map must never overwrite newer rules"
    );
}

#[test]
fn selecting_editor_in_first_open_template_activates_only_one_alternative_example() {
    let (_root, source) = fixture(&crate::keybinding_profiles::user_config());
    let selected = patch(
        &source,
        Patch::set(
            "settings.editor",
            toml::Value::Array(vec!["code".into(), "--reuse-window".into()]),
        ),
    )
    .unwrap();
    assert_eq!(
        selected.settings.settings.editor,
        Some(crate::settings::SettingsEditor::Command(vec![
            "code".into(),
            "--reuse-window".into()
        ]))
    );
    assert_eq!(
        selected
            .text
            .lines()
            .filter(|line| line.starts_with("editor ="))
            .count(),
        1
    );
    assert!(!selected.settings.updates.automatic_checks);
}
