use gpui_kit::component::diff::DiffFile;
use jaywalk::{
    diff_model::{Algorithm, FileDiff},
    overview::overview_patch_with_algorithm,
    repository::ChangedFile,
    settings::Settings,
};
#[test]
fn each_algorithm_preserves_full_content_and_agrees_across_fluid_and_standard_diffs() {
    let cases = [
        ("", "added\n"),
        ("removed", ""),
        ("last\n", "last"),
        (
            "fn alpha() {\n    old();\n}\n\nfn beta() {\n    keep();\n}\n",
            "fn alpha(arg: i32) {\n    new();\n}\n\nfn beta() {\n    keep();\n}\n",
        ),
        (
            "fn alpha() {\n    first();\n}\n\nfn beta() {\n    second();\n}\n",
            "fn beta() {\n    second();\n}\n\nfn alpha() {\n    changed();\n}\n",
        ),
        (
            "fn 漢字() {\n\n    old();\n}\n",
            "fn 漢字() {\n    if ready {\n        new();\n    }\n}",
        ),
    ];
    for algorithm in [Algorithm::Myers, Algorithm::Patience] {
        for (before, after) in cases {
            let model = FileDiff::with_algorithm(before, after, algorithm);
            let mut old_end = 0;
            let mut new_end = 0;
            let mut additions = 0;
            let mut deletions = 0;
            for block in &model.blocks {
                assert_eq!(block.before.start, old_end);
                assert_eq!(block.after.start, new_end);
                old_end = block.before.end;
                new_end = block.after.end;
                if block.changed {
                    additions += block.after.len();
                    deletions += block.before.len();
                } else {
                    assert_eq!(
                        &before[model.byte_range(block, true)],
                        &after[model.byte_range(block, false)]
                    );
                }
            }
            assert_eq!(model.before_offsets[old_end], before.len());
            assert_eq!(model.after_offsets[new_end], after.len());
            let file = ChangedFile::new("modified".into(), "file.rs".into(), "file.rs".into());
            let patch = overview_patch_with_algorithm(&file, Some((before, after)), algorithm);
            let docs = DiffFile::parse(&patch).unwrap();
            assert_eq!(docs.len(), 1);
            assert_eq!(
                (docs[0].additions(), docs[0].deletions()),
                (additions, deletions),
                "{algorithm:?}: {patch}"
            );
        }
    }
}
#[test]
fn algorithm_settings_keep_myers_default_and_reject_unsupported_backends() {
    assert_eq!(Settings::default().diff.algorithm, Algorithm::Myers);
    for (name, algorithm) in [
        ("myers", Algorithm::Myers),
        ("patience", Algorithm::Patience),
    ] {
        assert_eq!(
            Settings::parse(&format!("[diff]\nalgorithm='{name}'"))
                .unwrap()
                .diff
                .algorithm,
            algorithm
        );
    }
    for name in ["histogram", "minimal", "lcs", "typo"] {
        assert!(Settings::parse(&format!("[diff]\nalgorithm='{name}'")).is_err());
    }
}
