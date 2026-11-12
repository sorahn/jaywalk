//! Native install paths must agree with Cargo even with conflicting user settings.
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn installs_fresh_native_outputs_and_upgrades_only_its_own_legacy_link() {
    let root = std::env::temp_dir().join(format!("jaywalk native install {}", std::process::id()));
    fs::create_dir_all(root.join("scripts")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    for directory in ["bin", "target/debug", "target/release", "result", "blocked"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    for script in ["install-cli.sh", "build-native.sh"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join(script),
            root.join("scripts").join(script),
        )
        .unwrap();
    }
    executable(&root.join("bin/uname"), "echo Linux");
    executable(
        &root.join("bin/rustc"),
        "echo 'host: aarch64-unknown-linux-gnu'",
    );
    executable(
        &root.join("bin/cargo"),
        r#"
shift
target=${CARGO_BUILD_TARGET:-wrong}
directory=${CARGO_TARGET_DIR:-wrong}
profile=debug
while [ "$#" -gt 0 ]; do
 case "$1" in
  --target) target=$2; shift 2 ;;
  --target-dir) directory=$2; shift 2 ;;
  --release) profile=release; shift ;;
  *) shift ;;
 esac
done
mkdir -p "$directory/$target/$profile"
printf '#!/bin/sh\necho fresh-native\n' > "$directory/$target/$profile/jw"
printf '#!/bin/sh\necho fresh-app\n' > "$directory/$target/$profile/jaywalk"
chmod +x "$directory/$target/$profile/jw" "$directory/$target/$profile/jaywalk"
"#,
    );
    let run = |prefix: &Path, profile: &str| {
        Command::new("sh")
            .arg(root.join("scripts/install-cli.sh"))
            .arg(prefix)
            .arg(profile)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("CARGO_TARGET_DIR", root.join("custom cache"))
            .env("CARGO_BUILD_TARGET", "wrong-cross-target")
            .output()
            .unwrap()
    };
    for profile in ["debug", "release"] {
        let prefix = root.join("result").join(profile);
        fs::create_dir_all(&prefix).unwrap();
        let stale = root.join("target").join(profile).join("jw");
        executable(&stale, "echo stale");
        executable(&stale.with_file_name("jaywalk"), "echo stale");
        std::os::unix::fs::symlink(&stale, prefix.join("jw")).unwrap();
        let output = run(&prefix, profile);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let target = root
            .join("target/aarch64-unknown-linux-gnu")
            .join(profile)
            .join("jw");
        assert_eq!(fs::read_link(prefix.join("jw")).unwrap(), target);
        assert_eq!(
            Command::new(prefix.join("jw")).output().unwrap().stdout,
            b"fresh-native\n"
        );
        assert!(run(&prefix, profile).status.success()); // Idempotent reinstall.
    }
    assert!(!root.join("custom cache").exists());
    let prefix = root.join("blocked");
    fs::write(prefix.join("jw"), "user command").unwrap();
    assert!(!run(&prefix, "debug").status.success());
    assert_eq!(
        fs::read_to_string(prefix.join("jw")).unwrap(),
        "user command"
    );
    fs::remove_file(prefix.join("jw")).unwrap();
    std::os::unix::fs::symlink(root.join("unrelated"), prefix.join("jw")).unwrap();
    assert!(!run(&prefix, "debug").status.success());
    assert_eq!(
        fs::read_link(prefix.join("jw")).unwrap(),
        root.join("unrelated")
    );
    fs::remove_file(prefix.join("jw")).unwrap();
    assert!(run(&prefix, "debug").status.success()); // Fresh install.
    fs::remove_dir_all(root).unwrap();
}
