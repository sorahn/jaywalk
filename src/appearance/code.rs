//! Normalize code colors independently, then project them after shell/accent.
use super::*;
use gpui::component::highlighter::ThemeStyle;

pub(super) fn project_code(
    theme: &mut Theme,
    source: &Arc<HighlightTheme>,
    shell: &jaywalk::theme::Shell,
) {
    let mut highlight = (**source).clone();
    for (color, part) in [
        (&mut highlight.style.editor_background, Part::Background),
        (&mut highlight.style.editor_foreground, Part::Foreground),
        (
            &mut highlight.style.editor_line_number,
            Part::MutedForeground,
        ),
        (&mut highlight.style.editor_border, Part::Border),
    ] {
        let resolved = shell.get(Role::Surface(Surface::Viewer, part));
        // Canonical explicit role overrides retain final authority. Generated
        // shell surfaces fill gaps, never replace a supplied code-theme color.
        if resolved.origin == jaywalk::theme::Origin::Override || color.is_none() {
            *color = Some(gpui::rgba(resolved.color.0).into());
        }
    }
    if *theme.highlight_theme != highlight {
        theme.highlight_theme = Arc::new(highlight);
    }
}

pub(super) fn syntax_theme(dark: bool) -> std::sync::Arc<HighlightTheme> {
    let mut highlight = if dark {
        (*HighlightTheme::default_dark()).clone()
    } else {
        (*HighlightTheme::default_light()).clone()
    };
    // The built-in code palette inherits the generated viewer surface. Imported
    // sources will retain their supplied colors instead of this fallback.
    highlight.style.editor_background = None;
    highlight.style.editor_foreground = None;
    highlight.style.editor_line_number = None;
    highlight.name = if dark {
        "Jaywalk VS Code dark"
    } else {
        "Jaywalk VS Code light"
    }
    .into();
    let token = |hex: &str| -> Option<ThemeStyle> {
        Some(
            serde_json::from_value(serde_json::json!({"color":hex}))
                .expect("built-in syntax color is valid"),
        )
    };
    highlight.style.syntax.comment = token(if dark { "#6a9955" } else { "#008000" });
    highlight.style.syntax.comment_doc = highlight.style.syntax.comment;
    highlight.style.syntax.keyword = token(if dark { "#569cd6" } else { "#0000ff" });
    highlight.style.syntax.string = token(if dark { "#ce9178" } else { "#a31515" });
    highlight.style.syntax.number = token(if dark { "#b5cea8" } else { "#098658" });
    highlight.style.syntax.function = token(if dark { "#dcdcaa" } else { "#795e26" });
    highlight.style.syntax.type_ = token(if dark { "#4ec9b0" } else { "#267f99" });
    std::sync::Arc::new(highlight)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn supplied(dark: bool, background: u32) -> Arc<HighlightTheme> {
        let mut source = (*syntax_theme(dark)).clone();
        source.style.editor_background = Some(rgb(background).into());
        source.style.editor_foreground = Some(rgb(0xcceeff).into());
        source.style.editor_line_number = Some(rgb(0x6789ab).into());
        source.style.editor_selection = Some(gpui::rgba(0x55667788).into());
        source.style.editor_caret = Some(rgb(0xabcdef).into());
        source.style.editor_gutter_background = Some(rgb(0x111a22).into());
        Arc::new(source)
    }
    #[test]
    fn shell_and_accent_fill_code_gaps_without_replacing_supplied_colors() {
        for dark in [false, true] {
            let source = supplied(dark, 0x102030);
            let original = (*source).clone();
            let settings = Settings::parse("[theme.shell_auto]\nseed='#903050'").unwrap();
            let mut theme = Theme::default();
            project_shell(&mut theme, settings.resolved_theme().shell(dark));
            apply_accent(
                &mut theme,
                RgbColor::try_from("#ff8800".to_owned()).unwrap(),
            );
            let shell_selection = theme.selection;
            project_code(&mut theme, &source, settings.resolved_theme().shell(dark));
            let code = theme.editor_colors();
            assert_eq!(code.background, Hsla::from(rgb(0x102030)));
            assert_eq!(code.foreground, Hsla::from(rgb(0xcceeff)));
            assert_eq!(code.line_number, Hsla::from(rgb(0x6789ab)));
            assert_eq!(code.muted_foreground, Hsla::from(rgb(0x6789ab)));
            assert_eq!(code.selection, Hsla::from(gpui::rgba(0x55667788)));
            assert_eq!(code.caret, Hsla::from(rgb(0xabcdef)));
            assert_ne!(code.selection, shell_selection);
            assert_ne!(code.foreground, theme.foreground);
            assert_eq!(*source, original);
            let retained = theme.highlight_theme.clone();
            project_code(&mut theme, &source, settings.resolved_theme().shell(dark));
            assert!(Arc::ptr_eq(&retained, &theme.highlight_theme));
        }
    }
    #[test]
    fn canonical_editor_overrides_have_final_authority_and_do_not_recolor_shell() {
        let settings = Settings::parse(
            r##"[theme.overrides]
"editor.background"="#112233"
"editor.foreground"="#c0ffee"
"editor.border"="#445566"
"##,
        )
        .unwrap();
        let source = supplied(true, 0x999999);
        let mut theme = Theme::default();
        project_shell(&mut theme, settings.resolved_theme().shell(true));
        let shell_background = theme.background;
        project_code(&mut theme, &source, settings.resolved_theme().shell(true));
        assert_eq!(theme.editor_colors().background, Hsla::from(rgb(0x112233)));
        assert_eq!(theme.editor_colors().foreground, Hsla::from(rgb(0xc0ffee)));
        assert_eq!(theme.editor_colors().border, Hsla::from(rgb(0x445566)));
        assert_eq!(theme.background, shell_background);
        assert_ne!(theme.background, theme.editor_colors().background);
    }
    #[gpui_test::test]
    fn same_mode_code_updates_and_mode_transitions_keep_fonts_and_source_identity(
        cx: &mut gpui_test::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let mut settings=Settings::parse("[ui]\nappearance='dark'\nfont_size=16\n[editor]\nfont_size=18\nfont_family='Menlo'").unwrap();
            init(&settings,cx);
            let fonts=(Theme::global(cx).font_family.clone(),Theme::global(cx).mono_font_family.clone());
            for background in [0x123456,0x345678] {
                cx.global_mut::<AppAppearance>().code[1]=supplied(true,background);
                apply(&settings,cx);
                assert_eq!(Theme::global(cx).editor_colors().background,Hsla::from(rgb(background)));
                assert_eq!(Theme::global(cx).font_size,px(16.));
                assert_eq!(Theme::global(cx).mono_font_size,px(18.));
                assert_eq!((Theme::global(cx).font_family.clone(),Theme::global(cx).mono_font_family.clone()), fonts);
            }
            cx.global_mut::<AppAppearance>().code[1]=syntax_theme(true);
            apply(&settings,cx);
            assert_eq!(Theme::global(cx).editor_colors().background,Hsla::from(rgb(0x121212)));
            assert_eq!(Theme::global(cx).editor_colors().selection,Theme::global(cx).selection);
            cx.global_mut::<AppAppearance>().code[1]=supplied(true,0x345678);
            settings.ui.appearance=Appearance::System;
            apply(&settings,cx);
            refresh(Some(WindowAppearance::Light),false,cx);
            assert_eq!(Theme::global(cx).mode,ThemeMode::Light);
            refresh(Some(WindowAppearance::Dark),false,cx);
            assert_eq!(Theme::global(cx).editor_colors().background,Hsla::from(rgb(0x345678)));
            assert_eq!(Theme::global(cx).mono_font_size,px(18.));
        });
    }
}

/// Compile the normalized format on the asset worker, never during paint.
pub(crate) fn code_asset(asset: &jaywalk::theme::assets::Asset, dark: bool) -> Arc<HighlightTheme> {
    let mut syntax = serde_json::Map::new();
    for (capture, style) in &asset.syntax {
        syntax.insert(
            capture.clone(),
            serde_json::json!({
                "color": style.color.map(|color| format!("#{color:08x}")),
                "font_style": if style.italic { "italic" } else { "normal" },
                "font_weight": if style.bold { 700 } else { 400 },
            }),
        );
    }
    let mut style = serde_json::Map::new();
    for (key, color) in &asset.editor {
        style.insert(key.clone(), serde_json::json!(format!("#{color:08x}")));
    }
    style.insert("syntax".into(), serde_json::Value::Object(syntax));
    Arc::new(
        serde_json::from_value(serde_json::json!({
            "name": asset.name,
            "appearance": if dark { "dark" } else { "light" },
            "style": style,
        }))
        .expect("normalized asset uses the toolkit's declared theme schema"),
    )
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use gpui::component::highlighter::PreparedInputHighlighter;
    #[test]
    fn imported_theme_styles_real_bundled_language_captures_and_normal_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.json");
        std::fs::write(&path,r##"{"type":"dark","colors":{"editor.background":"#123456","editor.foreground":"#aabbcc"},"tokenColors":[
            {"scope":"string","settings":{"foreground":"#ff3344","fontStyle":"bold italic"}},
            {"scope":"markup.heading","settings":{"foreground":"#33ccff"}}
        ]}"##).unwrap();
        let asset = jaywalk::theme::assets::load(&path).unwrap();
        let theme = code_asset(&asset, true);
        for (language, text, needle, color) in [
            (
                "rust",
                "fn main() { let text = \"hello\"; }",
                "hello",
                0xff3344,
            ),
            ("javascript", "const text = \"hello\";", "hello", 0xff3344),
            (
                "typescript",
                "const text: string = \"hello\";",
                "hello",
                0xff3344,
            ),
            ("python", "text = \"hello\"\n", "hello", 0xff3344),
            ("json", "{\"text\": \"hello\"}", "hello", 0xff3344),
            ("markdown", "# hello\n", "hello", 0x33ccff),
        ] {
            let factory = PreparedInputHighlighter::new(language, text).into_factory();
            let highlighter = factory(language).unwrap();
            let start = text.find(needle).unwrap();
            let styles = highlighter.styles(&(0..text.len()), theme.as_ref());
            let expected = Some(Hsla::from(rgb(color)));
            assert!(
                styles
                    .iter()
                    .any(|(range, style)| range.contains(&start) && style.color == expected),
                "{language}: {styles:?}"
            );
            if language != "markdown" {
                let style = styles
                    .iter()
                    .find(|(range, style)| range.contains(&start) && style.color == expected)
                    .unwrap()
                    .1;
                assert_eq!(style.font_style, Some(gpui::FontStyle::Italic));
                assert_eq!(style.font_weight, Some(gpui::FontWeight::BOLD));
            }
        }
        assert_eq!(
            theme
                .style
                .syntax
                .style("variable.parameter")
                .unwrap()
                .color,
            Some(Hsla::from(rgb(0xaabbcc)))
        );
        assert_eq!(
            theme.style.editor_background,
            Some(Hsla::from(rgb(0x123456)))
        );
    }
}
