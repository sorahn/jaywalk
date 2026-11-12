extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::*;
use jaywalk::settings_reload::SettingsSource;

mod about;
mod appearance;
mod cli;
mod instance;
mod interface_state;
mod recents;
mod settings_service;
mod windows;
use cli::Arguments;
mod launcher;
mod platform;
mod viewer;
use viewer::Viewer;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> std::result::Result<(), String> {
    let mut arguments = Arguments::parse();
    if arguments.package_version {
        println!("{}", jaywalk::version::PACKAGE);
        return Ok(());
    }
    if arguments.inspect {
        let comparison = arguments.comparison()?;
        println!(
            "{} changed files\nbase {}\ntip {}\noperation {}",
            comparison.files.len(),
            comparison.base,
            comparison.tip,
            comparison.operation
        );
        return Ok(());
    }
    arguments.normalize()?;
    let settings = SettingsSource::resolve(arguments.config.as_deref())?.read()?;
    let directory = instance::directory()?;
    if !arguments.instance_server {
        if instance::forward(&directory, &arguments)? {
            return Ok(());
        }
        if !arguments.foreground
            && (arguments.wait || arguments.terminal_launch || !platform::is_app_bundle())
        {
            launcher::launch(arguments.config.as_deref())?;
            return instance::wait_for_server(&directory, &arguments);
        }
    }
    let Some(server) = instance::Server::claim(&directory)? else {
        // Concurrent CLI launchers can race to spawn a server. Only the lock
        // winner owns a GUI; every original launcher forwards its own request.
        return if arguments.instance_server {
            Ok(())
        } else {
            instance::wait_for_server(&directory, &arguments)
        };
    };
    let recent_path = jaywalk::recent::RecentStore::default_path()?;
    let recent_store = jaywalk::recent::RecentStore::load(recent_path.clone())
        .unwrap_or_else(|error| jaywalk::recent::RecentStore::preserve_failed(recent_path, error));
    let receiver = server.start();
    let initial = (!arguments.instance_server)
        .then(|| windows::PreparedWindow::load(arguments.clone(), settings.clone()));
    let reopen_arguments = arguments.for_welcome();
    let application = gpui::application().with_assets(gpui::assets::AllAssets);
    application.on_reopen(move |cx| {
        if !windows::has_viewer_windows(cx) {
            open_arguments(reopen_arguments.clone(), None, false, cx);
        }
    });
    application.run(move |cx| {
        gpui::init(cx);
        windows::init(cx);
        interface_state::init(cx);
        settings_service::init(cx);
        recents::init(recent_store, cx);
        windows::apply_application_settings(&settings.application, cx);
        if let Err(error) = viewer::init(&arguments, &settings, cx) {
            eprintln!("Could not apply keymap: {error}");
            cx.quit();
            return;
        }
        appearance::init(&settings, cx);
        windows::listen(receiver, cx);
        if let Some(prepared) = initial
            && let Err(error) = windows::open(prepared, None, cx)
        {
            eprintln!("{error}");
            cx.quit();
        }
    });
    Ok(())
}

fn open_arguments(
    arguments: Arguments,
    origin: Option<WeakEntity<Viewer>>,
    require_repository: bool,
    cx: &mut App,
) {
    open_arguments_after(
        arguments,
        origin,
        require_repository,
        None,
        windows::OpenDisposition::Reuse,
        cx,
    );
}
fn open_arguments_in_new_window(
    arguments: Arguments,
    origin: Option<WeakEntity<Viewer>>,
    require_repository: bool,
    cx: &mut App,
) {
    open_arguments_after(
        arguments,
        origin,
        require_repository,
        None,
        windows::OpenDisposition::NewWindow,
        cx,
    );
}
fn show_recent_repository_picker(arguments: Arguments, cx: &mut App) {
    open_arguments_after(
        arguments.for_welcome(),
        None,
        false,
        Some(jaywalk::commands::CommandId::OpenRecent),
        windows::OpenDisposition::Reuse,
        cx,
    );
}
fn open_arguments_after(
    mut arguments: Arguments,
    origin: Option<WeakEntity<Viewer>>,
    require_repository: bool,
    after_open: Option<jaywalk::commands::CommandId>,
    disposition: windows::OpenDisposition,
    cx: &mut App,
) {
    let error_arguments = arguments.for_welcome();
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move {
                arguments.normalize()?;
                if require_repository && arguments.welcome {
                    return Err("The selected folder is not a Git or Jujutsu repository.".into());
                }
                let settings = SettingsSource::resolve(arguments.config.as_deref())?.read()?;
                Ok::<_, String>(windows::PreparedWindow::load(arguments, settings))
            })
            .await;
        let result = match result {
            Ok(prepared) => cx.update(|cx| {
                let arguments = prepared.arguments.clone();
                windows::open_with_disposition(prepared, None, disposition, cx)?;
                if let Some(command) = after_open {
                    windows::dispatch_command(&arguments, command, cx)?;
                }
                Ok(())
            }),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            cx.update(|cx| report_open_error(error_arguments, origin, error, cx));
        }
    })
    .detach();
}
fn report_open_error(
    arguments: Arguments,
    origin: Option<WeakEntity<Viewer>>,
    error: String,
    cx: &mut App,
) {
    if let Some(origin) = origin {
        let _ = origin.update(cx, |viewer, cx| viewer.report_open_error(error, cx));
        return;
    }
    let arguments = arguments.for_welcome();
    cx.spawn(async move |cx| {
        let config = arguments.config.clone();
        let requested = arguments.clone();
        let settings = cx
            .background_spawn(async move {
                let settings = SettingsSource::resolve(config.as_deref())?.read()?;
                Ok::<_, String>(windows::PreparedWindow::load(requested, settings))
            })
            .await;
        match settings {
            Ok(prepared) => cx.update(|cx| match windows::open(prepared, None, cx) {
                Ok(()) => windows::report_welcome_error(&arguments, error, cx),
                Err(open_error) => {
                    eprintln!("{error}; could not show welcome: {open_error}")
                }
            }),
            Err(settings_error) => eprintln!("{error}; could not read settings: {settings_error}"),
        }
    })
    .detach();
}
// Keep File > Open Repository usable when the app stays open without windows.
fn choose_repository(arguments: Arguments, origin: Option<WeakEntity<Viewer>>, cx: &mut App) {
    choose_repository_with_disposition(arguments, origin, windows::OpenDisposition::Reuse, cx);
}
fn choose_repository_in_new_window(
    arguments: Arguments,
    origin: Option<WeakEntity<Viewer>>,
    cx: &mut App,
) {
    choose_repository_with_disposition(arguments, origin, windows::OpenDisposition::NewWindow, cx);
}
fn choose_repository_with_disposition(
    arguments: Arguments,
    origin: Option<WeakEntity<Viewer>>,
    disposition: windows::OpenDisposition,
    cx: &mut App,
) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open repository".into()),
    });
    cx.spawn(async move |cx| match receiver.await {
        Ok(Ok(Some(paths))) => {
            if let Some(path) = paths.into_iter().next() {
                cx.update(|cx| {
                    open_arguments_after(
                        arguments.for_repository(path),
                        origin,
                        true,
                        None,
                        disposition,
                        cx,
                    )
                });
            }
        }
        Ok(Ok(None)) => {}
        result => {
            let error = format!("Could not select repository: {result:?}");
            cx.update(|cx| report_open_error(arguments, origin, error, cx));
        }
    })
    .detach();
}
fn choose_comparison(arguments: Arguments, origin: Option<WeakEntity<Viewer>>, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: Some("Choose two files or two folders to compare".into()),
    });
    cx.spawn(async move |cx| match receiver.await {
        Ok(Ok(Some(paths))) if paths.len() == 2 => {
            let mut arguments = arguments.for_repository(arguments.directory().to_owned());
            arguments.diff = Some(paths);
            cx.update(|cx| open_arguments(arguments, origin, false, cx));
        }
        Ok(Ok(None)) => {}
        result => {
            let error = match result {
                Ok(Ok(Some(_))) => "Select exactly two files or two folders to compare.".into(),
                other => format!("Could not select comparison inputs: {other:?}"),
            };
            cx.update(|cx| report_open_error(arguments, origin, error, cx));
        }
    })
    .detach();
}
