use std::process::Command;

#[test]
fn cli_and_bundle_version_outputs_share_the_compiled_app_identity() {
    let expected = option_env!("JAYWALK_RELEASE_VERSION")
        .filter(|version| !version.is_empty())
        .unwrap_or("0.0.0");
    assert_eq!(jaywalk::version::PACKAGE, expected);
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
        jaywalk::version::PACKAGE
    );
}
