//! Explicit GitHub CLI import; no token discovery at application startup.
use super::{Control, Error, Secret};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
/// Worker-only metadata probe. Finder PATH may omit common Homebrew locations.
pub fn discover() -> Option<PathBuf> {
    let mut directories: Vec<_> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    directories.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    directories
        .into_iter()
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join("gh"))
        .find(|path| {
            std::fs::metadata(path).is_ok_and(|metadata| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    metadata.is_file()
                }
            })
        })
}
fn gh_command(path: &Path) -> Command {
    let mut command = Command::new(path);
    command
        .args(["auth", "token", "--hostname", "github.com"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // This command imports gh's saved credential, not ambient shell/CI credentials.
    for name in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "GH_HOST",
    ] {
        command.env_remove(name);
    }
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1")
        .env("DO_NOT_TRACK", "1");
    command
}
pub fn parse(bytes: &[u8]) -> Result<Secret, Error> {
    if bytes.len() > 16_386 {
        return Err(Error::InvalidToken);
    }
    let value = std::str::from_utf8(bytes).map_err(|_| Error::InvalidToken)?;
    // Accept CLI line termination only; embedded whitespace is never a credential.
    let value = value.trim_end_matches(['\r', '\n']);
    if !value.is_ascii() || value.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err(Error::InvalidToken);
    }
    Secret::new(value.to_owned()).map_err(|_| Error::InvalidToken)
}
pub fn import_gh(path: &Path, control: &Control) -> Result<Secret, Error> {
    control.check()?;
    let mut child = gh_command(path).spawn().map_err(|_| Error::CliImport)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let result = loop {
        if let Err(error) = control.check() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => break Err("GitHub CLI token import timed out"),
            Err(_) => break Err("Could not wait for GitHub CLI"),
        }
    };
    let status = match result {
        Ok(status) => status,
        Err(_error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::CliImport);
        }
    };
    if !status.success() {
        return Err(Error::CliImport);
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or(Error::CliImport)?
        .take(16_387)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::CliImport)?;
    parse(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_is_explicit_bounded_and_redacted() {
        assert_eq!(
            parse(b"synthetic-secret\r\n").unwrap().expose(),
            "synthetic-secret"
        );
        for bytes in [
            &b""[..],
            &b"secret extra"[..],
            &b"secret\nextra"[..],
            &b"\xff"[..],
            &vec![b'x'; 16387][..],
        ] {
            assert_eq!(parse(bytes).unwrap_err(), Error::InvalidToken);
        }
        let command = gh_command(Path::new("/synthetic/gh"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["auth", "token", "--hostname", "github.com"]
        );
        for key in ["GH_TOKEN", "GITHUB_TOKEN", "GH_DEBUG"] {
            assert!(
                command
                    .get_envs()
                    .any(|(name, value)| name == key && value.is_none())
            );
        }
        let cancelled = Control::new(Duration::from_secs(1));
        cancelled.cancel();
        assert_eq!(
            import_gh(Path::new("/does-not-exist"), &cancelled).unwrap_err(),
            Error::Cancelled
        );
    }
    #[cfg(unix)]
    #[test]
    fn cli_failures_never_echo_captured_output() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("gh");
        std::fs::write(
            &path,
            "#!/bin/sh\nprintf synthetic-secret\nprintf private-error >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = import_gh(&path, &Control::new(Duration::from_secs(2))).unwrap_err();
        assert_eq!(error, Error::CliImport);
        assert!(!error.to_string().contains("synthetic-secret"));
        std::fs::write(&path, "#!/bin/sh\nprintf synthetic-secret\n").unwrap();
        assert_eq!(
            import_gh(&path, &Control::new(Duration::from_secs(2)))
                .unwrap()
                .expose(),
            "synthetic-secret"
        );
    }
}
