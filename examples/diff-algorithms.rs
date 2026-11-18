//! Synthetic, opt-in alignment trials. Never reads a checkout. Git timings include
//! process startup and are not directly comparable with in-process `similar`.
use similar::{Algorithm, TextDiff};
use std::{process::Command, time::Instant};
fn record(
    case: &str,
    backend: &str,
    algorithm: &str,
    indent: bool,
    started: Instant,
    patch: &str,
    show: bool,
) {
    let lines = patch.lines();
    let hunks = lines.clone().filter(|l| l.starts_with("@@")).count();
    let edits = lines
        .filter(|l| {
            (l.starts_with('+') && !l.starts_with("+++"))
                || (l.starts_with('-') && !l.starts_with("---"))
        })
        .count();
    println!(
        "{}",
        serde_json::json!({"case":case,"backend":backend,"algorithm":algorithm,"indent_heuristic":indent,"elapsed_us":started.elapsed().as_micros(),"hunks":hunks,"edit_lines":edits})
    );
    if show {
        println!("{patch}");
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let show = std::env::args().any(|arg| arg == "--show");
    let repeated = "{\n\n    same();\n}\n\n".repeat(400);
    let cases = [
        (
            "signature",
            "fn old_name() {\n    work();\n}\n\nfn other() {\n    keep();\n}\n".into(),
            "fn new_name(arg: i32) {\n    work(arg);\n}\n\nfn other() {\n    keep();\n}\n".into(),
        ),
        (
            "reordered",
            "fn alpha() {\n    first();\n}\n\nfn beta() {\n    second();\n}\n".into(),
            "fn beta() {\n    second();\n}\n\nfn alpha() {\n    changed();\n}\n".into(),
        ),
        (
            "indent",
            "fn run() {\n    a();\n    b();\n}\n".into(),
            "fn run() {\n    if ready() {\n        a();\n        b();\n    }\n}\n".into(),
        ),
        (
            "repeated",
            format!("fn before() {{\n{repeated}}}\n"),
            format!("fn after() {{\n{repeated}    added();\n}}\n"),
        ),
    ];
    let directory = tempfile::tempdir()?;
    let old = directory.path().join("before");
    let new = directory.path().join("after");
    for (case, before, after) in cases {
        for (name, algorithm) in [
            ("myers", Algorithm::Myers),
            ("patience", Algorithm::Patience),
            ("lcs", Algorithm::Lcs),
        ] {
            let start = Instant::now();
            let patch = TextDiff::configure()
                .algorithm(algorithm)
                .diff_lines(&before, &after)
                .unified_diff()
                .context_radius(0)
                .header("a/file", "b/file")
                .to_string();
            record(
                case,
                "similar 2.7",
                name,
                false,
                start,
                &patch,
                show && case != "repeated",
            );
        }
        std::fs::write(&old, &before)?;
        std::fs::write(&new, &after)?;
        for algorithm in ["myers", "patience", "histogram", "minimal"] {
            for indent in [false, true] {
                let start = Instant::now();
                let output = Command::new("git")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .args([
                        "diff",
                        "--no-index",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        "--unified=0",
                        &format!("--diff-algorithm={algorithm}"),
                        if indent {
                            "--indent-heuristic"
                        } else {
                            "--no-indent-heuristic"
                        },
                        "--",
                    ])
                    .arg(&old)
                    .arg(&new)
                    .output()?;
                if output.status.code() != Some(1) {
                    return Err("Synthetic Git diff failed".into());
                }
                record(
                    case,
                    "git",
                    algorithm,
                    indent,
                    start,
                    &String::from_utf8(output.stdout)?,
                    show && case != "repeated",
                );
            }
        }
    }
    Ok(())
}
