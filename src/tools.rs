//! Repository subprocesses must work when Finder starts the single app instance.
use std::process::Command;

pub(crate) fn command(program: &str) -> Result<Command, String> {
    let mut command = Command::new(program);
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = command;
        let path = repository_path(std::env::var_os("PATH"), std::env::var_os("HOME"))?;
        // Set the child's search path, never the multithreaded app's environment.
        // jj's own Git subprocesses inherit the same tool discovery behavior.
        command.env("PATH", path);
        Ok(command)
    }
    #[cfg(not(target_os = "macos"))]
    Ok(command)
}

#[cfg(target_os = "macos")]
fn repository_path(
    path: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<std::ffi::OsString, String> {
    use std::path::PathBuf;
    let mut directories = path
        .as_ref()
        .map(|path| std::env::split_paths(path).collect::<Vec<_>>())
        .unwrap_or_default();
    // Preserve explicit terminal PATH precedence. Finder normally supplies only
    // system directories, omitting Homebrew and Cargo installations.
    let mut fallbacks = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home {
        fallbacks.push(PathBuf::from(home).join(".cargo/bin"));
    }
    fallbacks.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
    for directory in fallbacks {
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    std::env::join_paths(directories)
        .map_err(|error| format!("Could not prepare repository tool PATH: {error}"))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn finder_path_gains_installation_directories_without_changing_terminal_precedence() {
        let path =
            repository_path(Some("/usr/bin:/bin".into()), Some("/Users/fixture".into())).unwrap();
        let dirs = std::env::split_paths(&path).collect::<Vec<_>>();
        assert_eq!(dirs[0], std::path::Path::new("/usr/bin"));
        assert!(dirs.contains(&"/opt/homebrew/bin".into()));
        assert!(dirs.contains(&"/usr/local/bin".into()));
        assert!(dirs.contains(&"/Users/fixture/.cargo/bin".into()));
        let terminal = repository_path(
            Some("/custom/tools:/opt/homebrew/bin:/usr/bin".into()),
            None,
        )
        .unwrap();
        let dirs = std::env::split_paths(&terminal).collect::<Vec<_>>();
        assert_eq!(dirs[0], std::path::Path::new("/custom/tools"));
        assert_eq!(
            dirs.iter()
                .filter(|path| **path == std::path::Path::new("/opt/homebrew/bin"))
                .count(),
            1
        );
    }
}
