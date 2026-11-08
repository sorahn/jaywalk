//! Optional source provenance; Cargo.toml owns the release version.
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn main() {
    for path in [
        "build.rs",
        "Cargo.toml",
        "Cargo.lock",
        "src",
        "config",
        "packaging",
        "scripts",
        "vendor",
        "CHANGELOG.md",
    ] {
        println!("cargo::rerun-if-changed={path}");
    }
    // Work in source archives as well as Git-backed jj checkouts. Missing Git
    // metadata is explicit in the displayed version; it never invents a hash.
    let revision = git(&["rev-parse", "--short=12", "HEAD"]);
    let dirty = git(&["status", "--porcelain", "--untracked-files=normal"]);
    if let Some(directory) = git(&["rev-parse", "--absolute-git-dir"]) {
        let directory = PathBuf::from(directory);
        for name in ["HEAD", "index", "packed-refs"] {
            let path = directory.join(name);
            if path.exists() {
                println!("cargo::rerun-if-changed={}", path.display());
            }
        }
        if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
            && let Some(path) = git(&["rev-parse", "--git-path", &reference])
        {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo supplies the package version");
    let development = env::var("PROFILE").expect("Cargo supplies the build profile") != "release";
    let provenance = match (revision, dirty) {
        (Some(revision), Some(status)) => format!(
            "{revision}{}",
            if status.is_empty() { "" } else { "-dirty" }
        ),
        _ => {
            println!("cargo::warning=Source revision unavailable; build provenance is unknown");
            "unknown".into()
        }
    };
    let display = if development {
        format!("{version}-dev ({provenance})")
    } else {
        version
    };
    println!("cargo::rustc-env=JAYWALK_DISPLAY_VERSION={display}");
    println!("cargo::rustc-env=JAYWALK_BUILD_REVISION={provenance}");
}
