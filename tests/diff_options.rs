use jaywalk::{
    diff_model::{Algorithm, FileDiff},
    diff_options::{ComparisonOptions, Whitespace},
};
fn changes(old: &str, new: &str, options: ComparisonOptions) -> usize {
    FileDiff::with_options(old, new, Algorithm::Myers, options)
        .blocks
        .iter()
        .filter(|b| b.changed)
        .count()
}
#[test]
fn whitespace_modes_have_distinct_meanings_and_preserve_real_edits() {
    let options = |whitespace| ComparisonOptions {
        whitespace,
        ..Default::default()
    };
    assert!(changes("a b\n", "a  b\n", Default::default()) > 0);
    assert_eq!(
        changes("a b \t\n", "a b\n", options(Whitespace::IgnoreTrailing)),
        0
    );
    assert!(changes("a b\n", "a  b\n", options(Whitespace::IgnoreTrailing)) > 0);
    assert_eq!(
        changes("a b\n", "a\t\tb\n", options(Whitespace::IgnoreChanges)),
        0
    );
    assert!(changes("ab\n", "a b\n", options(Whitespace::IgnoreChanges)) > 0);
    assert_eq!(
        changes("ab\n", " a \t b\n", options(Whitespace::IgnoreAll)),
        0
    );
    for mode in [
        Whitespace::Exact,
        Whitespace::IgnoreTrailing,
        Whitespace::IgnoreChanges,
        Whitespace::IgnoreAll,
    ] {
        assert!(changes("value=old \n", "value=new\n", options(mode)) > 0);
        assert!(
            changes("a\u{a0}b\n", "ab\n", options(mode)) > 0,
            "Non-ASCII source content is never silently stripped"
        );
    }
}
#[test]
fn eol_and_blank_ignores_are_independent_and_do_not_hide_the_final_newline() {
    let eol = ComparisonOptions {
        ignore_line_endings: true,
        ..Default::default()
    };
    assert_eq!(changes("one\r\ntwo\r\n", "one\ntwo\n", eol), 0);
    assert!(changes("one\r\n", "one", eol) > 0);
    assert!(changes("one\rtwo", "onetwo", eol) > 0);
    let blank = ComparisonOptions {
        ignore_blank_lines: true,
        ..Default::default()
    };
    assert_eq!(changes("one\n\n\t\ntwo\n", "one\ntwo\n", blank), 0);
    assert!(changes("one\n\ntwo\n", "one\nchanged\ntwo\n", blank) > 0);
    assert!(changes("one\r\n", "one\n", blank) > 0);
}
#[test]
fn every_policy_covers_each_original_row_once_and_preserves_byte_ranges() {
    let cases = [
        ("one\n\nold \t\n終\r\n", "one\nnew\n \n終\n"),
        ("\n\t\n", ""),
        ("", "\n"),
        ("\nold\n\n", "new\n\n\n"),
        ("a\rb\nlast\r", "a\rc\nlast\r\n"),
    ];
    for algorithm in [Algorithm::Myers, Algorithm::Patience] {
        for whitespace in [
            Whitespace::Exact,
            Whitespace::IgnoreTrailing,
            Whitespace::IgnoreChanges,
            Whitespace::IgnoreAll,
        ] {
            for ignore_blank_lines in [false, true] {
                for ignore_line_endings in [false, true] {
                    let options = ComparisonOptions {
                        whitespace,
                        ignore_blank_lines,
                        ignore_line_endings,
                    };
                    for (old, new) in cases {
                        let rows = options.alignment(old, new, algorithm);
                        assert_eq!(
                            rows.iter().filter_map(|r| r.before).collect::<Vec<_>>(),
                            (0..old.split_inclusive('\n').count()).collect::<Vec<_>>()
                        );
                        assert_eq!(
                            rows.iter().filter_map(|r| r.after).collect::<Vec<_>>(),
                            (0..new.split_inclusive('\n').count()).collect::<Vec<_>>()
                        );
                        let model = FileDiff::with_options(old, new, algorithm, options);
                        assert_eq!(
                            model
                                .blocks
                                .iter()
                                .map(|b| &old[model.byte_range(b, true)])
                                .collect::<String>(),
                            old
                        );
                        assert_eq!(
                            model
                                .blocks
                                .iter()
                                .map(|b| &new[model.byte_range(b, false)])
                                .collect::<String>(),
                            new
                        );
                        assert_eq!(model.line_alignment(), rows);
                    }
                }
            }
        }
    }
}
#[test]
fn diff_defaults_and_live_config_values_are_typed_and_color_settings_are_adaptive() {
    use jaywalk::settings::Settings;
    let defaults = Settings::default();
    let p = defaults.diff.preferences();
    assert_eq!(p.comparison, ComparisonOptions::default());
    assert!(p.word_highlight);
    assert!(!p.show_whitespace && !p.show_line_endings && !p.highlight_whitespace_errors);
    let s=Settings::parse("[diff]\nwhitespace='ignore_changes'\nignore_line_endings=true\nignore_blank_lines=true\nword_highlight=false\nshow_whitespace=true\nshow_line_endings=true\nhighlight_whitespace_errors=true\n[diff.colors]\nwhitespace_error={light='#123456',dark='#abcdef'}").unwrap();
    assert_eq!(s.diff.whitespace, Whitespace::IgnoreChanges);
    assert!(s.diff.show_line_endings);
    assert_eq!(
        s.diff.colors.whitespace_error.resolve(false).value(),
        0x123456
    );
    assert_eq!(
        s.diff.colors.whitespace_error.resolve(true).value(),
        0xabcdef
    );
    assert!(Settings::parse("[diff]\nwhitespace='ignore_some'").is_err());
    assert_eq!(
        changes(
            "a \t\n",
            "a\n",
            ComparisonOptions {
                whitespace: Whitespace::IgnoreChanges,
                ..Default::default()
            }
        ),
        0
    );
}
#[test]
fn ignored_unpaired_blank_rows_have_a_bounded_corresponding_position() {
    let options = ComparisonOptions {
        ignore_blank_lines: true,
        ..Default::default()
    };
    let model = FileDiff::with_options("", "\n\n\n", Algorithm::Myers, options);
    for line in 0..3 {
        assert_eq!(model.corresponding_line(line, false), 0);
    }
}
#[test]
fn display_alignment_rejects_partial_duplicate_and_missing_addresses() {
    use gpui_kit::component::diff::{DiffFile, DiffLineAlignment};
    let patch =
        jaywalk::diff_model::full_patch("a\nb\n", "a \nb\n", Algorithm::Myers, "a/x", "b/x");
    let doc = DiffFile::parse(&patch).unwrap().remove(0);
    assert!(doc.clone().with_alignment(vec![]).is_err());
    assert!(
        doc.clone()
            .with_alignment(vec![DiffLineAlignment {
                original: None,
                modified: None,
                changed: false
            }])
            .is_err()
    );
    assert!(
        doc.clone()
            .with_alignment(vec![DiffLineAlignment {
                original: Some(1),
                modified: Some(0),
                changed: false
            }])
            .is_err()
    );
    let rows = (0..2)
        .map(|i| DiffLineAlignment {
            original: Some(i),
            modified: Some(i),
            changed: false,
        })
        .collect();
    let ignored = doc.with_alignment(rows).unwrap();
    assert_eq!((ignored.additions(), ignored.deletions()), (0, 0));
    let partial = DiffFile::parse("--- a/x\n+++ b/x\n@@ -10 +10 @@\n-a\n+b\n")
        .unwrap()
        .remove(0);
    assert!(
        partial
            .with_alignment(vec![DiffLineAlignment {
                original: Some(0),
                modified: Some(0),
                changed: false
            }])
            .is_err()
    );
}
#[test]
fn whitespace_error_spans_are_bounded_and_keep_original_unicode_offsets() {
    use gpui_kit::component::diff::whitespace_error_ranges;
    let text = "\t  \t漢字  \r\nclean\n";
    let spans = whitespace_error_ranges(text);
    assert!(spans.iter().any(|r| &text[r.clone()] == "  "));
    assert!(spans.iter().any(|r| &text[r.clone()] == "\t  \t"));
    assert!(
        spans
            .iter()
            .all(|r| text.is_char_boundary(r.start) && text.is_char_boundary(r.end))
    );
    let pathological = " \t".repeat(10000) + "code";
    assert_eq!(whitespace_error_ranges(&pathological).len(), 1);
}
