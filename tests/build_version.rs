#[path = "../src/build_version.rs"]
mod build_version;

#[test]
fn local_builds_are_development_builds_even_in_release_profile() {
    for release_profile in [false, true] {
        let identity = build_version::resolve(None, release_profile, "abc123-dirty").unwrap();
        assert_eq!(identity.numeric, "0.0.0");
        assert_eq!(identity.display, "dev (abc123-dirty)");
    }
}

#[test]
fn injected_calendar_version_is_shared_by_app_and_bundle() {
    let identity = build_version::resolve(Some("2026.11.23"), true, "abc123").unwrap();
    assert_eq!(identity.numeric, "2026.11.23");
    assert_eq!(identity.display, "2026.11.23");
    let development = build_version::resolve(Some("2026.11.23"), false, "abc123").unwrap();
    assert_eq!(development.numeric, identity.numeric);
    assert_eq!(development.display, "2026.11.23-dev (abc123)");
}

#[test]
fn invalid_injected_versions_fail_instead_of_silently_building() {
    for value in [
        "",
        "0.0.0",
        "2026.00.1",
        "2026.13.1",
        "2026.11.0",
        "2026.01.1",
        "2026.11.1-dev",
        "2026.11.1+abc",
        "2026.11.1\n",
        "2026.11",
        "2026.11.999999999999",
        "2026.+1.1",
    ] {
        assert!(
            build_version::resolve(Some(value), true, "abc123").is_err(),
            "accepted {value:?}"
        );
    }
}
