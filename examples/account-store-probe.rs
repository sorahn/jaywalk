//! Synthetic macOS signing/Keychain check. Never reads app accounts or tokens.
//! Build twice with different JAYWALK_SIGNING_PROBE_VERSION values, signing each
//! build with the same identity/identifier, to verify access across rebuilds.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entry = keyring::Entry::new("jaywalk.signing-probe.v1", "synthetic")?;
    match std::env::args().nth(1).as_deref() {
        Some("write") => entry.set_password("synthetic signing probe credential")?,
        Some("read") => {
            if entry.get_password()? != "synthetic signing probe credential" {
                return Err("unexpected synthetic credential".into());
            }
        }
        Some("delete") => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(error) => return Err(error.into()),
        },
        _ => return Err("use write, read or delete".into()),
    }
    println!(
        "Synthetic store check passed (build {}).",
        option_env!("JAYWALK_SIGNING_PROBE_VERSION").unwrap_or("unspecified")
    );
    Ok(())
}
