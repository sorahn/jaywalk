extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::component::{Theme, ThemeMode};
use gpui::*;
use jaywalk::settings_reload::SettingsSource;

mod cli;
mod instance;
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
    let receiver = server.start();
    let initial = (!arguments.instance_server).then(|| arguments.clone());
    gpui::application()
        .with_assets(gpui::assets::AllAssets)
        .run(move |cx| {
            gpui::init(cx);
            windows::init(cx);
            viewer::init(&arguments, &settings, cx);
            Theme::change(ThemeMode::Dark, None, cx);
            configure_code_theme(cx);
            Theme::update(cx, |theme| {
                if let Some(family) = &settings.ui.font_family {
                    theme.font_family = family.clone().into();
                }
                theme.font_size = px(settings.ui.font_size);
                theme.mono_font_size = px(settings.editor.font_size);
                if let Some(family) = &settings.editor.font_family {
                    theme.mono_font_family = family.clone().into();
                }
            });
            windows::listen(receiver, cx);
            if let Some(arguments) = initial
                && let Err(error) = windows::open(arguments, settings, None, cx)
            {
                eprintln!("{error}");
                cx.quit();
            }
        });
    Ok(())
}

// Keep File > Open Repository usable after the last native window closes.
fn choose_repository(mut arguments: Arguments, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open repository".into()),
    });
    cx.spawn(async move |cx| match receiver.await {
        Ok(Ok(Some(paths))) => {
            if let Some(path) = paths.into_iter().next() {
                arguments.directory = path;
                arguments.diff = None;
                arguments.base = None;
                arguments.tip = "@".into();
                arguments.file = None;
                let config = arguments.config.clone();
                let settings = cx
                    .background_spawn(async move {
                        arguments.normalize()?;
                        let settings = SettingsSource::resolve(config.as_deref())?.read()?;
                        Ok::<_, String>((arguments, settings))
                    })
                    .await;
                match settings {
                    Ok((arguments, settings)) => {
                        cx.update(|cx| {
                            if let Err(error) = windows::open(arguments, settings, None, cx) {
                                eprintln!("Could not open repository: {error}");
                            }
                        });
                    }
                    Err(error) => eprintln!("Could not open repository: {error}"),
                }
            }
        }
        Ok(Ok(None)) => {}
        result => eprintln!("Could not select repository: {result:?}"),
    })
    .detach();
}

fn configure_code_theme(cx: &mut App) {
    use gpui::component::highlighter::{HighlightTheme, ThemeStyle};
    let mut highlight = (*HighlightTheme::default_dark()).clone();
    highlight.name = "VS Code default style".into();
    highlight.style.editor_background = Some(rgb(0x1f1f1f).into());
    highlight.style.editor_foreground = Some(rgb(0xcccccc).into());
    highlight.style.editor_line_number = Some(rgb(0x6e7681).into());
    let token = |hex: &str| -> Option<ThemeStyle> {
        Some(serde_json::from_value(serde_json::json!({"color": hex})).expect("valid theme color"))
    };
    highlight.style.syntax.comment = token("#6a9955");
    highlight.style.syntax.comment_doc = token("#6a9955");
    highlight.style.syntax.keyword = token("#569cd6");
    highlight.style.syntax.string = token("#ce9178");
    highlight.style.syntax.number = token("#b5cea8");
    highlight.style.syntax.function = token("#dcdcaa");
    highlight.style.syntax.type_ = token("#4ec9b0");
    Theme::global_mut(cx).highlight_theme = std::sync::Arc::new(highlight);
}
