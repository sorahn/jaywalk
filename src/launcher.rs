//! Start the GUI without retaining the invoking terminal or its pipe handles.
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn log_path(config: Option<&Path>) -> Result<PathBuf, String> {
    config
        .map(|path| Ok(path.with_extension("log")))
        .unwrap_or_else(|| {
            jaywalk::settings::default_path().map(|path| path.with_file_name("jaywalk.log"))
        })
}

pub(crate) fn launch(config: Option<&Path>) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate the viewer executable: {error}"))?;
    let path = log_path(config)?;
    let parent = path.parent().filter(|path| !path.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let log = options
        .open(&path)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let errors = log
        .try_clone()
        .map_err(|error| format!("Could not attach viewer diagnostics: {error}"))?;
    let mut command = Command::new(executable);
    // The server starts without an initial window. The invoking CLI sends
    // its normalized request after startup, including its own wait lifetime.
    command.arg("--foreground").arg("--instance-server");
    if let Some(config) = config {
        command.arg("--config").arg(config);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    // Dropping Child does not terminate it. The GUI owns its lifetime, while
    // the shell receives this launcher's exit status as soon as spawning succeeds.
    command.spawn().map_err(|error| {
        format!("Could not launch the viewer: {error}. Use --foreground for diagnostics.")
    })?;
    Ok(())
}
