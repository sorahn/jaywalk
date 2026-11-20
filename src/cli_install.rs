//! Install only a link to the running bundle; never replace another command.
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Missing,
    Installed,
    Conflict,
}
pub fn status(destination: &Path, source: &Path) -> Result<Status, String> {
    match std::fs::symlink_metadata(destination) {
        Ok(_) => Ok(
            if std::fs::canonicalize(destination).ok().as_deref() == Some(source) {
                Status::Installed
            } else {
                Status::Conflict
            },
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Status::Missing),
        Err(error) => Err(format!(
            "Could not inspect {}: {error}",
            destination.display()
        )),
    }
}
pub fn bundled_launcher(executable: &Path) -> Result<PathBuf, String> {
    let executable = std::fs::canonicalize(executable)
        .map_err(|error| format!("Could not locate installed Jaywalk: {error}"))?;
    let directory = executable.parent().ok_or("Executable has no directory")?;
    if !directory.ends_with("Contents/MacOS") {
        return Err(
            "Install the Jaywalk app bundle first; development builds can use just install-cli."
                .into(),
        );
    }
    if executable.starts_with("/Volumes")
        || executable
            .components()
            .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Err("Move Jaywalk to Applications or another permanent folder, reopen it there, then install jw.".into());
    }
    let source = directory.join("jw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&source)
            .map_err(|error| format!("Bundled jw is unavailable: {error}"))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err("The bundled jw command is not executable. Reinstall Jaywalk.".into());
        }
    }
    Ok(source)
}
pub fn install(source: &Path, directory: &Path) -> Result<PathBuf, String> {
    if !directory.is_absolute() {
        return Err("Choose an absolute installation directory".into());
    }
    let source = std::fs::canonicalize(source)
        .map_err(|error| format!("Could not resolve bundled jw: {error}"))?;
    if !source.is_file() {
        return Err("The bundled jw must be a regular executable file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&source)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err("The bundled jw is not executable".into());
        }
    }
    let destination = directory.join("jw");
    match status(&destination, &source)? {
        Status::Installed => return Ok(destination),
        Status::Conflict => {
            return Err(format!(
                "{} already exists and points elsewhere. Choose another directory or remove that command yourself.",
                destination.display()
            ));
        }
        Status::Missing => {}
    }
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create {}: {error}. Choose a writable directory or use the manual instructions.", directory.display()))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, &destination).map_err(|error| {
        format!(
            "Could not install {}: {error}. Existing commands are never replaced.",
            destination.display()
        )
    })?;
    #[cfg(not(unix))]
    return Err("Automatic CLI installation is supported on Unix only".into());
    #[cfg(unix)]
    Ok(destination)
}
pub fn candidate_directories(home: &Path, path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let mut directories = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|p| p.is_absolute())
        .collect::<Vec<_>>();
    for directory in [
        home.join(".local/bin"),
        "/usr/local/bin".into(),
        "/opt/homebrew/bin".into(),
    ] {
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    directories
}

/// Literal POSIX shell examples, suitable for pasting into zsh/bash/fish.
pub fn manual_instructions(source: &Path) -> String {
    let literal = format!("'{}'", source.to_string_lossy().replace('\'', "'\\''"));
    format!("mkdir -p \"$HOME/.local/bin\"\nln -s {literal} \"$HOME/.local/bin/jw\"\n\nVerify: command -v jw; jw --version\nFor a custom location, change the destination directory.\nAdd ~/.local/bin to PATH:
fish: fish_add_path \"$HOME/.local/bin\"\nzsh/bash: add export PATH=\"$HOME/.local/bin:$PATH\" to ~/.zshrc or ~/.bashrc.\nRemove only the installed link to uninstall the command; keep the app and config.")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn copied_commands_preserve_literal_paths_and_never_execute_path_content() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("j'w $(touch escaped).tool");
        std::fs::write(&source, "fixture").unwrap();
        let commands = manual_instructions(&source)
            .lines()
            .take(2)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(commands)
                .env("HOME", fixture.path())
                .current_dir(fixture.path())
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            std::fs::read_link(fixture.path().join(".local/bin/jw")).unwrap(),
            source
        );
        assert!(!fixture.path().join("escaped").exists());
    }
    #[test]
    fn installs_idempotently_and_recognizes_chained_links_without_overwriting_conflicts() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("Jay walk ü.app/Contents/MacOS/jw");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "fixture").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
        let source = source.canonicalize().unwrap();
        let bin = fixture.path().join("custom/bin");
        let installed = install(&source, &bin).unwrap();
        assert_eq!(install(&source, &bin).unwrap(), installed);
        let another = fixture.path().join("another");
        symlink(&installed, &another).unwrap();
        assert_eq!(status(&another, &source).unwrap(), Status::Installed);
        std::fs::remove_file(&installed).unwrap();
        std::fs::write(&installed, "other command").unwrap();
        assert!(install(&source, &bin).is_err());
        assert_eq!(
            std::fs::read_to_string(&installed).unwrap(),
            "other command"
        );
        std::fs::remove_file(&installed).unwrap();
        symlink("missing-target", &installed).unwrap();
        assert!(install(&source, &bin).is_err());
        assert_eq!(
            std::fs::read_link(&installed).unwrap(),
            Path::new("missing-target")
        );
        assert!(install(&source, Path::new("relative")).is_err());
    }
    #[test]
    fn refuses_translocated_or_unbundled_installation_and_checks_execute_permission() {
        assert!(bundled_launcher(Path::new("/nonexistent/app")).is_err());
        let fixture = tempfile::tempdir().unwrap();
        let directory = fixture
            .path()
            .join("AppTranslocation/Example.app/Contents/MacOS");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("jaywalk"), "fixture").unwrap();
        assert!(
            bundled_launcher(&directory.join("jaywalk"))
                .unwrap_err()
                .contains("Move Jaywalk")
        );
    }
}
