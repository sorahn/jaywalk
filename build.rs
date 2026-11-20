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
    println!("cargo::rerun-if-env-changed=JAYWALK_SPARKLE_DIR");
    if env::var_os("CARGO_FEATURE_MACOS_UPDATER").is_some()
        && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
    {
        let sdk = PathBuf::from(
            env::var_os("JAYWALK_SPARKLE_DIR")
                .expect("macos-updater requires JAYWALK_SPARKLE_DIR; run scripts/fetch-sparkle.py"),
        );
        let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
        let object = out.join("updater.o");
        let status = Command::new("xcrun")
            .args([
                "clang",
                "-fobjc-arc",
                "-fmodules",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-unused-parameter",
                "-mmacosx-version-min=15.0",
                "-c",
                "packaging/macos/updater.m",
                "-F",
            ])
            .arg(&sdk)
            .arg("-o")
            .arg(&object)
            .status()
            .expect("compile updater bridge");
        assert!(status.success(), "could not compile Sparkle bridge");
        assert!(
            Command::new("ar")
                .arg("crs")
                .arg(out.join("libjaywalk_updater.a"))
                .arg(object)
                .status()
                .expect("archive updater bridge")
                .success()
        );
        println!("cargo::rustc-link-search=native={}", out.display());
        println!("cargo::rustc-link-lib=static=jaywalk_updater");
        println!("cargo::rustc-link-search=framework={}", sdk.display());
        println!("cargo::rustc-link-lib=framework=Sparkle");
        println!("cargo::rustc-link-lib=framework=AppKit");
        println!("cargo::rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        // /bin/sh and system Git can strip DYLD_* during integration tests.
        // Keep a build-machine SDK fallback only in development executables;
        // distributable release binaries resolve their embedded framework only.
        if env::var("PROFILE").as_deref() != Ok("release") {
            println!("cargo::rustc-link-arg=-Wl,-rpath,{}", sdk.display());
        }
    }

    for path in [
        "build.rs",
        "Cargo.toml",
        "Cargo.lock",
        "src",
        "config",
        "packaging",
        "scripts",
        "vendor",
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
