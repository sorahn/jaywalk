use std::process::Command;

#[test]
fn cli_and_bundle_version_outputs_share_the_compiled_cargo_identity() {
    let executable = env!("CARGO_BIN_EXE_jaywalk");
    let version = Command::new(executable).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("jaywalk {}", jaywalk::version::DISPLAY)
    );
    let package = Command::new(executable)
        .arg("--package-version")
        .output()
        .unwrap();
    assert!(package.status.success());
    assert_eq!(
        String::from_utf8(package.stdout).unwrap().trim(),
        env!("CARGO_PKG_VERSION")
    );
}
