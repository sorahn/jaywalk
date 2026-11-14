//! Route all launch/menu requests through the same window registry.
use crate::instance::{Request, Response};
use crate::{Arguments, Viewer};
use gpui::*;
use jaywalk::settings::Settings;
use jaywalk::settings_reload::SettingsSource;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenDisposition {
    Configured,
    NewWindow,
}
fn reuse_priority(
    disposition: OpenDisposition,
    single_window_mode: bool,
    origin: bool,
    active: bool,
) -> Option<u8> {
    if disposition == OpenDisposition::NewWindow || !single_window_mode {
        None
    } else {
        Some(if origin {
            0
        } else if active {
            1
        } else {
            2
        })
    }
}

fn release_waiters(waiting: &mut Vec<Sender<Response>>) {
    for client in waiting.drain(..) {
        let _ = client.send(Response::Closed);
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
    _inputs: Option<jaywalk::difftool::OwnedInputs>,
    key: Key,
    window: AnyWindowHandle,
    viewer: Entity<Viewer>,
    waiting: Vec<Sender<Response>>,
    last_used: std::time::Instant,
}
#[derive(Default)]
struct Windows(HashMap<WindowId, Record>);
impl Global for Windows {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Windows::default());
    cx.on_window_closed(|cx, id| {
        if let Some(record) = cx.global_mut::<Windows>().0.remove(&id) {
            let mut record = record;
            release_waiters(&mut record.waiting);
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
) -> Result<WindowId, String> {
    open_with_disposition(prepared, reply, OpenDisposition::Configured, None, cx)
}
pub(crate) fn open_with_disposition(
    prepared: PreparedWindow,
    reply: Option<Sender<Response>>,
    disposition: OpenDisposition,
    origin: Option<&WeakEntity<Viewer>>,
    cx: &mut App,
) -> Result<WindowId, String> {
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
    let active = cx.active_window().map(|window| window.window_id());
    let batch_key = arguments.difftool.as_ref().map(|capture| capture.key());
    let existing = cx
        .global::<Windows>()
        .0
        .values()
        .filter_map(|record| {
            let same_batch = batch_key.is_some() && record.key == key;
            let priority = if same_batch {
                Some(0)
            } else {
                reuse_priority(
                    disposition,
                    settings.application.single_window_mode,
                    origin.is_some_and(|origin| origin.entity_id() == record.viewer.entity_id()),
                    active == Some(record.window.window_id()),
                )
            }?;
            Some((
                priority,
                std::cmp::Reverse(record.last_used),
                record.window,
                record.viewer.clone(),
                record.key != key,
            ))
        })
        .min_by_key(|(priority, opened, ..)| (*priority, *opened))
        .map(|(_, _, window, viewer, different)| (window, viewer, different));
    let inputs = arguments
        .difftool
        .as_ref()
        .map(|c| c.own(&crate::instance::directory()?))
        .transpose()?;
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
                } else if arguments.difftool.is_some() {
                    viewer.update(cx, |viewer, cx| {
                        viewer.update_difftool(arguments.clone(), window, cx)
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
        if different_subject {
            release_waiters(&mut record.waiting);
        }
        record._inputs = inputs;
        record.key = key;
        record.last_used = std::time::Instant::now();
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
            let viewer = cx.new(|cx| Viewer::new(arguments.clone(), settings, layout, window, cx));
            // Register once per native window, rather than per replaced repository.
            viewer.update(cx, |_, cx| {
                cx.observe_window_activation(window, |_, window, cx| {
                    if window.is_window_active()
                        && let Some(record) = cx
                            .global_mut::<Windows>()
                            .0
                            .get_mut(&window.window_handle().window_id())
                    {
                        record.last_used = std::time::Instant::now();
                    }
                })
                .detach();
            });
            viewer
        })
        .map_err(|error| format!("Could not open the viewer window: {error}"))?;
        #[cfg(feature = "profiling")]
        crate::frame_profile::start(window, cx);
        cx.global_mut::<Windows>().0.insert(
            window.window_id(),
            Record {
                _inputs: inputs,
                key,
                window,
                viewer,
                waiting: Vec::new(),
                last_used: std::time::Instant::now(),
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
    Ok(id)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{Key, OpenDisposition, reuse_priority};
    use crate::Arguments;
    use clap::Parser;
    // TestAppContext uses GPUI's test platform: no native desktop window opens.
    #[gpui_test::test]
    fn configured_opens_replace_the_origin_and_new_opens_keep_distinct_windows(
        cx: &mut gpui_test::TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!("jaywalk-routing-{}", std::process::id()));
        std::fs::create_dir_all(root.join("one/.git")).unwrap();
        std::fs::create_dir_all(root.join("two/.git")).unwrap();
        let config = root.join("config.toml");
        std::fs::write(&config, "").unwrap();
        let mut one = Arguments::parse_from(["jaywalk", root.join("one").to_str().unwrap()]);
        one.config = Some(config.clone());
        one.normalize().unwrap();
        let two = one.for_repository(root.join("two"));
        let mut settings = jaywalk::settings::Settings::default();
        // Exercise routing without starting the production layout writer on
        // an OS thread outside GPUI's deterministic test scheduler.
        settings.layout.remember_per_repository = false;
        settings.application.single_window_mode = true;
        cx.update(|app| {
            gpui_kit::init(app);
            super::init(app);
            crate::settings_service::init(app);
            crate::recents::init_without_storage(app);
            crate::windows::apply_application_settings(&settings.application, app);
            crate::viewer::init(&one, &settings, app).unwrap();
            crate::appearance::init(&settings, app);
            let (client, response) = std::sync::mpsc::channel();
            let mut waiting = one.clone();
            waiting.wait = true;
            let first = super::open(
                super::PreparedWindow::load(waiting, settings.clone()),
                Some(client),
                app,
            )
            .unwrap();
            assert!(matches!(
                response.recv().unwrap(),
                crate::instance::Response::Opened
            ));
            let origin = app.global::<super::Windows>().0[&first].viewer.downgrade();
            let same = super::open(
                super::PreparedWindow::load(one.clone(), settings.clone()),
                None,
                app,
            )
            .unwrap();
            assert_eq!(same, first);
            assert!(response.try_recv().is_err());
            // The requested repository differs: single-window mode still replaces it.
            let replaced = super::open_with_disposition(
                super::PreparedWindow::load(two.clone(), settings.clone()),
                None,
                OpenDisposition::Configured,
                Some(&origin),
                app,
            )
            .unwrap();
            assert_eq!(first, replaced);
            assert!(matches!(
                response.recv().unwrap(),
                crate::instance::Response::Closed
            ));
            assert_eq!(app.global::<super::Windows>().0.len(), 1);
            assert!(app.global::<super::Windows>().0[&first].key == Key::new(&two));
            let explicit = super::open_with_disposition(
                super::PreparedWindow::load(two.clone(), settings.clone()),
                None,
                OpenDisposition::NewWindow,
                Some(&origin),
                app,
            )
            .unwrap();
            assert_ne!(first, explicit);
            // Recent-picker origin wins over the other, more recently opened window.
            let current = super::open_with_disposition(
                super::PreparedWindow::load(one.clone(), settings.clone()),
                None,
                OpenDisposition::Configured,
                Some(&origin),
                app,
            )
            .unwrap();
            assert_eq!(current, first);
            // A CLI-style request has no origin and reuses the active/recent viewer.
            let cli = super::open(
                super::PreparedWindow::load(two.clone(), settings.clone()),
                None,
                app,
            )
            .unwrap();
            assert_eq!(cli, first);
            let mut multiple = settings.clone();
            multiple.application.single_window_mode = false;
            let third = super::open(
                super::PreparedWindow::load(two.clone(), multiple),
                None,
                app,
            )
            .unwrap();
            assert_ne!(third, first);
            assert_ne!(third, explicit);
            assert_eq!(app.global::<super::Windows>().0.len(), 3);
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configured_mode_reuses_the_origin_then_active_viewer_without_matching_repositories() {
        assert_eq!(
            reuse_priority(OpenDisposition::Configured, true, true, false),
            Some(0)
        );
        assert_eq!(
            reuse_priority(OpenDisposition::Configured, true, false, true),
            Some(1)
        );
        assert_eq!(
            reuse_priority(OpenDisposition::Configured, true, false, false),
            Some(2)
        );
        for origin in [false, true] {
            for active in [false, true] {
                assert_eq!(
                    reuse_priority(OpenDisposition::Configured, false, origin, active),
                    None
                );
                for single in [false, true] {
                    assert_eq!(
                        reuse_priority(OpenDisposition::NewWindow, single, origin, active),
                        None
                    );
                }
            }
        }
    }

    #[test]
    fn replacing_a_comparison_releases_all_wait_clients_once() {
        let (one, first) = std::sync::mpsc::channel();
        let (two, second) = std::sync::mpsc::channel();
        let mut clients = vec![one, two];
        super::release_waiters(&mut clients);
        assert!(matches!(
            first.recv().unwrap(),
            crate::instance::Response::Closed
        ));
        assert!(matches!(
            second.recv().unwrap(),
            crate::instance::Response::Closed
        ));
        assert!(clients.is_empty());
        super::release_waiters(&mut clients);
        assert!(first.try_recv().is_err());
    }

    #[test]
    fn window_keys_preserve_aliases_and_distinguish_comparison_ranges() {
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
    if let Some(capture) = &arguments.difftool {
        return format!("{} · Git difftool — Jaywalk", capture.repository.display());
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
pub(crate) fn report_welcome_error(id: WindowId, error: String, cx: &mut App) {
    let viewer = cx
        .global::<Windows>()
        .0
        .get(&id)
        .map(|record| record.viewer.clone());
    if let Some(viewer) = viewer {
        viewer.update(cx, |viewer, cx| viewer.report_open_error(error, cx));
    }
}

pub(crate) fn dispatch_command(
    id: WindowId,
    command: jaywalk::commands::CommandId,
    cx: &mut App,
) -> Result<(), String> {
    let (window, viewer) = cx
        .global::<Windows>()
        .0
        .get(&id)
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
