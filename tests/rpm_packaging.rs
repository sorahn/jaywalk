//! Exercise the packaging boundary with disposable tool stand-ins, on macOS or Linux.
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
#[test]
fn packages_fresh_native_binaries_despite_custom_cargo_target_and_stale_outputs() {
    let root = std::env::temp_dir().join(format!("jaywalk rpm review {}", std::process::id()));
    for directory in [
        "scripts",
        "packaging/linux",
        "vendor/gpui-component",
        "bin",
        "target/release",
        "result",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in [
        "scripts/package-rpm.sh",
        "packaging/linux/jaywalk.spec",
        "packaging/linux/io.github.sorahn.jaywalk.desktop",
        "packaging/linux/io.github.sorahn.jaywalk.svg",
        "vendor/gpui-component/LICENSE-APACHE",
        "vendor/README.md",
        "LICENSE",
        "README.md",
    ] {
        fs::copy(source.join(file), root.join(file)).unwrap();
    }
    executable(&root.join("target/release/jaywalk"), "echo 0.0.0");
    executable(&root.join("target/release/jw"), "echo stale");
    for (name, body) in [
        ("uname", "echo Linux"),
        ("rustc", "echo 'host: aarch64-unknown-linux-gnu'"),
        ("rpm", "echo aarch64"),
        ("desktop-file-validate", ":"),
    ] {
        executable(&root.join("bin").join(name), body);
    }
    executable(
        &root.join("bin/cargo"),
        r#"
if [ "$1" = tree ]; then echo 'fixture 1.0 MIT'; exit; fi
shift
target=${CARGO_BUILD_TARGET:-native}
directory=${CARGO_TARGET_DIR:-target}
while [ "$#" -gt 0 ]; do
 case "$1" in --target) target=$2; shift 2 ;; --target-dir) directory=$2; shift 2 ;; *) shift ;; esac
done
mkdir -p "$directory/$target/release"
printf '#!/bin/sh\necho 0.1.1\n' > "$directory/$target/release/jaywalk"
printf '#!/bin/sh\necho fresh-native\n' > "$directory/$target/release/jw"
chmod +x "$directory/$target/release/jaywalk" "$directory/$target/release/jw"
"#,
    );
    executable(
        &root.join("bin/rpmbuild"),
        r#"tar -xzf target/rpm/SOURCES/jaywalk-0.1.1-linux-aarch64.tar.gz -C result"#,
    );
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let output = Command::new("sh")
        .arg(root.join("scripts/package-rpm.sh"))
        .env("PATH", path)
        .env("CARGO_TARGET_DIR", root.join("custom cache"))
        .env("CARGO_BUILD_TARGET", "wrong-cross-target")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(root.join("result/jaywalk-0.1.1/jw"))
            .unwrap()
            .contains("fresh-native")
    );
    assert!(!root.join("custom cache").exists());
    fs::remove_dir_all(root).unwrap();
}
