//! Small terminal entry point; the GUI remains in the Jaywalk executable.
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn executable() -> Result<PathBuf, String> {
    let current =
        std::env::current_exe().map_err(|error| format!("Could not locate jw: {error}"))?;
    let current = std::fs::canonicalize(&current)
        .map_err(|error| format!("Could not resolve {}: {error}", current.display()))?;
    #[cfg(target_os = "macos")]
    if let Some(parent) = current.parent() {
        let bundle = parent.join("Jaywalk.app/Contents/MacOS/jaywalk");
        if bundle.is_file() {
            return Ok(bundle);
        }
    }
    let sibling = current.with_file_name(if cfg!(windows) {
        "jaywalk.exe"
    } else {
        "jaywalk"
    });
    if sibling.is_file() {
        return Ok(sibling);
    }
    Err(format!(
        "Jaywalk executable is missing beside {}. Reinstall the CLI with scripts/install-cli.sh.",
        current.display()
    ))
}

fn main() -> ExitCode {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // Accept --wait before or after the diff subcommand, like editor launchers.
    if let Some(index) = args.iter().position(|arg| arg != "--wait")
        && args[index] == "diff"
    {
        args[index] = "--diff".into();
    }
    let result = executable().and_then(|executable| {
        Command::new(executable)
            .arg("--terminal-launch")
            .args(args)
            .status()
            .map_err(|error| format!("Could not launch Jaywalk: {error}"))
    });
    match result {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
