//! Route all launch/menu requests through the same window registry.
use crate::instance::{Request, Response};
use crate::{Arguments, Viewer};
use gpui::*;
use jaywalk::settings::Settings;
use jaywalk::settings_reload::SettingsSource;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenDisposition {
    Reuse,
    NewWindow,
}
fn reuse_priority(
    disposition: OpenDisposition,
    existing: &Key,
    requested: &Key,
    welcome: bool,
) -> Option<u8> {
    if disposition == OpenDisposition::NewWindow {
        None
    } else if existing == requested {
        Some(0)
    } else if welcome && existing.config == requested.config {
        Some(1)
    } else {
        None
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Subject {
    Welcome,
    Repository {
        directory: PathBuf,
        base: Option<String>,
        tip: String,
    },
    Diff(Vec<PathBuf>),
}
#[derive(Clone, PartialEq, Eq)]
struct Key {
    subject: Subject,
    config: Option<PathBuf>,
}
impl Key {
    fn new(arguments: &Arguments) -> Self {
        Self {
            subject: if arguments.welcome {
                Subject::Welcome
            } else {
                match &arguments.diff {
                    Some(paths) => Subject::Diff(paths.clone()),
                    None => Subject::Repository {
                        directory: arguments.directory().to_owned(),
                        base: arguments.base.clone(),
                        tip: arguments.tip.clone(),
                    },
                }
            },
            config: arguments.config.clone(),
        }
    }
}
struct Record {
    key: Key,
    window: AnyWindowHandle,
    viewer: Entity<Viewer>,
    waiting: Vec<Sender<Response>>,
    last_opened: std::time::Instant,
}
#[derive(Default)]
struct Windows(HashMap<WindowId, Record>);
impl Global for Windows {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Windows::default());
    cx.on_window_closed(|cx, id| {
        if let Some(record) = cx.global_mut::<Windows>().0.remove(&id) {
            for client in record.waiting {
                let _ = client.send(Response::Closed);
            }
        }
    })
    .detach();
}

/// Use the native policy so all actual windows count, including About, and
/// close observers release --wait clients before GPUI shuts down the app.
pub(crate) fn apply_application_settings(
    settings: &jaywalk::settings::ApplicationSettings,
    cx: &mut App,
) {
    cx.set_quit_mode(if settings.quit_on_last_window {
        QuitMode::LastWindowClosed
    } else {
        QuitMode::Explicit
    });
}

pub(crate) fn listen(receiver: async_channel::Receiver<Request>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok(Request {
            mut arguments,
            reply,
        }) = receiver.recv().await
        {
            let prepared = cx
                .background_spawn(async move {
                    arguments.normalize()?;
                    let settings = SettingsSource::resolve(arguments.config.as_deref())?.read()?;
                    Ok::<_, String>(PreparedWindow::load(arguments, settings))
                })
                .await;
            let result = match prepared {
                Ok(prepared) => cx.update(|cx| open(prepared, Some(reply.clone()), cx)),
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                let _ = reply.send(Response::Error(error));
            }
        }
    })
    .detach();
}

pub(crate) struct PreparedWindow {
    pub(crate) arguments: Arguments,
    settings: Settings,
    layout: crate::viewer::SessionLayout,
}
impl PreparedWindow {
    /// Read persistence before entering the window/UI update.
    pub(crate) fn load(arguments: Arguments, settings: Settings) -> Self {
        let layout = crate::viewer::SessionLayout::load(&arguments, &settings);
        Self {
            arguments,
            settings,
            layout,
        }
    }
}
pub(crate) fn open(
    prepared: PreparedWindow,
    reply: Option<Sender<Response>>,
    cx: &mut App,
) -> Result<(), String> {
    open_with_disposition(prepared, reply, OpenDisposition::Reuse, cx)
}
pub(crate) fn open_with_disposition(
    prepared: PreparedWindow,
    reply: Option<Sender<Response>>,
    disposition: OpenDisposition,
    cx: &mut App,
) -> Result<(), String> {
    let PreparedWindow {
        arguments,
        settings,
        mut layout,
    } = prepared;
    if !arguments.welcome && arguments.diff.is_none() {
        layout.use_latest(&settings, cx);
    }
    let source = SettingsSource::resolve(arguments.config.as_deref())?;
    let state = crate::settings_service::source(source.clone(), &settings, cx)?;
    crate::settings_service::publish(source, settings.clone(), cx)?;
    let settings = (*state.read(cx).current).clone();
    let key = Key::new(&arguments);
    let existing = cx
        .global::<Windows>()
        .0
        .values()
        .filter_map(|record| {
            let priority = reuse_priority(
                disposition,
                &record.key,
                &key,
                record.viewer.read(cx).is_welcome(),
            )?;
            Some((
                priority,
                std::cmp::Reverse(record.last_opened),
                record.window,
                record.viewer.clone(),
                record.key != key,
            ))
        })
        .min_by_key(|(priority, opened, ..)| (*priority, *opened))
        .map(|(_, _, window, viewer, different)| (window, viewer, different));
    let id = if let Some((window, viewer, different_subject)) = existing {
        let root = window
            .update(cx, |_, window, cx| {
                window.activate_window();
                let replace =
                    different_subject || (viewer.read(cx).is_welcome() && !arguments.welcome);
                if replace {
                    window.set_window_title(&window_title(&arguments));
                    viewer.update(cx, |viewer, cx| {
                        *viewer = Viewer::new(arguments.clone(), settings, layout, window, cx)
                    });
                } else if let Some(path) = &arguments.file {
                    viewer.update(cx, |viewer, cx| viewer.reveal_file(path, window, cx));
                }
                viewer.read(cx).repository_directory()
            })
            .map_err(|error| format!("Could not focus the repository window: {error}"))?;
        if let Some(root) = root {
            crate::recents::record(&root, cx);
        }
        let record = cx
            .global_mut::<Windows>()
            .0
            .get_mut(&window.window_id())
            .expect("the reused window remains registered");
        record.key = key;
        record.last_opened = std::time::Instant::now();
        window.window_id()
    } else {
        let options = WindowOptions {
            app_id: Some("io.github.sorahn.jaywalk".into()),
            titlebar: Some(TitlebarOptions {
                title: Some(window_title(&arguments).into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(settings.window.width), px(settings.window.height)),
                cx,
            ))),
            ..Default::default()
        };
        let (window, viewer) = gpui::open_window(options, cx, |window, cx| {
            crate::appearance::watch_window(window, cx);
            cx.new(|cx| Viewer::new(arguments.clone(), settings, layout, window, cx))
        })
        .map_err(|error| format!("Could not open the viewer window: {error}"))?;
        cx.global_mut::<Windows>().0.insert(
            window.window_id(),
            Record {
                key,
                window,
                viewer,
                waiting: Vec::new(),
                last_opened: std::time::Instant::now(),
            },
        );
        window.window_id()
    };
    if let Some(reply) = reply {
        let _ = reply.send(Response::Opened);
        if arguments.wait {
            cx.global_mut::<Windows>()
                .0
                .get_mut(&id)
                .expect("window was opened or found above")
                .waiting
                .push(reply);
        }
    }
    cx.activate(true);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::{Key, OpenDisposition, Subject, reuse_priority};
    use crate::Arguments;
    use clap::Parser;
    #[test]
    fn explicit_new_window_bypasses_matching_and_welcome_windows() {
        let repo = Key {
            subject: Subject::Repository {
                directory: "/repo".into(),
                base: None,
                tip: "@".into(),
            },
            config: None,
        };
        let welcome = Key {
            subject: Subject::Welcome,
            config: None,
        };
        assert_eq!(
            reuse_priority(OpenDisposition::Reuse, &repo, &repo, false),
            Some(0)
        );
        assert_eq!(
            reuse_priority(OpenDisposition::Reuse, &welcome, &repo, true),
            Some(1)
        );
        assert_eq!(
            reuse_priority(OpenDisposition::NewWindow, &repo, &repo, false),
            None
        );
        assert_eq!(
            reuse_priority(OpenDisposition::NewWindow, &welcome, &repo, true),
            None
        );
        assert_eq!(
            reuse_priority(OpenDisposition::Reuse, &welcome, &repo, false),
            None
        );
        let other_profile = Key {
            config: Some("/other.toml".into()),
            ..welcome
        };
        assert_eq!(
            reuse_priority(OpenDisposition::Reuse, &other_profile, &repo, true),
            None
        );
    }

    #[test]
    fn aliases_and_subdirectories_reuse_a_window_but_other_comparisons_do_not() {
        let root = std::env::temp_dir().join(format!("jw-window-key-{}", std::process::id()));
        std::fs::create_dir_all(root.join("repo/.git")).unwrap();
        std::fs::create_dir_all(root.join("repo/src")).unwrap();
        std::os::unix::fs::symlink(root.join("repo"), root.join("alias")).unwrap();
        let normalize = |path: &std::path::Path| {
            let mut arguments =
                Arguments::parse_from([std::ffi::OsStr::new("jaywalk"), path.as_os_str()]);
            arguments.normalize().unwrap();
            arguments
        };
        let repository = normalize(&root.join("repo"));
        let mut alias = normalize(&root.join("alias"));
        alias.wait = true;
        alias.file = Some("src/main.rs".into());
        assert!(Key::new(&repository) == Key::new(&alias));
        assert!(Key::new(&repository) == Key::new(&normalize(&root.join("repo/src"))));
        alias.base = Some("main".into());
        assert!(Key::new(&repository) != Key::new(&alias));
        assert!(Key::new(&repository.for_welcome()) == Key::new(&normalize(&root).for_welcome()));
        assert!(Key::new(&repository.for_welcome()) != Key::new(&repository));
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn window_title(arguments: &Arguments) -> String {
    if arguments.welcome {
        return "Welcome — Jaywalk".into();
    }
    match &arguments.diff {
        Some(paths) => format!("{} ↔ {} — Jaywalk", paths[0].display(), paths[1].display()),
        None => format!("{} — Jaywalk", arguments.directory().display()),
    }
}

pub(crate) fn has_viewer_windows(cx: &App) -> bool {
    cx.try_global::<Windows>()
        .is_some_and(|windows| !windows.0.is_empty())
}
pub(crate) fn report_welcome_error(arguments: &Arguments, error: String, cx: &mut App) {
    let key = Key::new(arguments);
    let viewer = cx
        .global::<Windows>()
        .0
        .values()
        .find(|record| record.key == key)
        .map(|record| record.viewer.clone());
    if let Some(viewer) = viewer {
        viewer.update(cx, |viewer, cx| viewer.report_open_error(error, cx));
    }
}

pub(crate) fn dispatch_command(
    arguments: &Arguments,
    command: jaywalk::commands::CommandId,
    cx: &mut App,
) -> Result<(), String> {
    let key = Key::new(arguments);
    let (window, viewer) = cx
        .global::<Windows>()
        .0
        .values()
        .find(|record| record.key == key)
        .map(|record| (record.window, record.viewer.clone()))
        .ok_or("The requested viewer window is no longer open")?;
    window
        .update(cx, |_, window, cx| {
            viewer.update(cx, |viewer, cx| viewer.execute_command(command, window, cx))
        })
        .map_err(|error| format!("Could not dispatch the window command: {error}"))
}

pub(crate) fn layout_saved(
    key: &crate::interface_state::StoreKey,
    error: Option<String>,
    cx: &mut App,
) {
    let viewers: Vec<_> = cx
        .global::<Windows>()
        .0
        .values()
        .map(|record| record.viewer.clone())
        .collect();
    for viewer in viewers {
        viewer.update(cx, |viewer, cx| viewer.layout_saved(key, error.clone(), cx));
    }
}
