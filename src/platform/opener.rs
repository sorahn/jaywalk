//! Open the settings file with a native app or a literal argv command.
use jaywalk::settings::SettingsEditor;
use std::path::Path;
use std::process::Command;

fn text_file_command(path: &Path, editor: Option<&SettingsEditor>) -> Result<Command, String> {
    let mut command = match editor {
        Some(SettingsEditor::Command(arguments)) => {
            let (program, arguments) = arguments
                .split_first()
                .ok_or("settings.editor requires an executable")?;
            let mut command = Command::new(program);
            command.args(arguments);
            command
        }
        Some(SettingsEditor::App(app)) => {
            #[cfg(target_os = "macos")]
            {
                let mut command = Command::new("open");
                command.arg("-a").arg(app).arg("--");
                command
            }
            #[cfg(not(target_os = "macos"))]
            {
                Command::new(app)
            }
        }
        None => {
            #[cfg(target_os = "macos")]
            {
                let mut command = Command::new("open");
                command.args(["-t", "--"]);
                command
            }
            #[cfg(target_os = "linux")]
            {
                Command::new("xdg-open")
            }
            #[cfg(target_os = "windows")]
            {
                Command::new("notepad.exe")
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
            return Err("Opening a text editor is not supported on this platform yet".into());
        }
    };
    command.arg(path);
    Ok(command)
}

pub(crate) fn open_text_file(path: &Path, editor: Option<&SettingsEditor>) -> Result<(), String> {
    let mut command = text_file_command(path, editor)?;
    let program = command.get_program().to_string_lossy().into_owned();
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "Could not open {} using {program}: {status}",
            path.display()
        )),
        Err(error) => Err(format!(
            "Could not open {} using {program}: {error}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::text_file_command;
    use jaywalk::settings::SettingsEditor;
    use std::ffi::OsStr;
    use std::path::Path;
    #[test]
    fn custom_command_preserves_spaces_and_shell_characters_as_literal_arguments() {
        let editor = SettingsEditor::Command(vec![
            "/Applications/Example Editor/bin/editor".into(),
            "--reuse-window".into(),
            "$(literal)".into(),
        ]);
        let path = Path::new("/tmp/config with spaces;literal.toml");
        let command = text_file_command(path, Some(&editor)).unwrap();
        assert_eq!(
            command.get_program(),
            OsStr::new("/Applications/Example Editor/bin/editor")
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                OsStr::new("--reuse-window"),
                OsStr::new("$(literal)"),
                path.as_os_str()
            ]
        );
        assert!(text_file_command(path, Some(&SettingsEditor::Command(vec![]))).is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_app_selection_and_default_editor_use_distinct_open_modes() {
        let path = Path::new("/tmp/config.toml");
        let editor = SettingsEditor::App("Example Editor".into());
        let custom = text_file_command(path, Some(&editor)).unwrap();
        assert_eq!(
            custom.get_args().collect::<Vec<_>>(),
            ["-a", "Example Editor", "--", "/tmp/config.toml"].map(OsStr::new)
        );
        let default = text_file_command(path, None).unwrap();
        assert_eq!(
            default.get_args().collect::<Vec<_>>(),
            ["-t", "--", "/tmp/config.toml"].map(OsStr::new)
        );
    }
}
