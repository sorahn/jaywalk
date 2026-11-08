extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::component::{Theme, ThemeMode};
use gpui::*;
use jaywalk::repository::Comparison;
use jaywalk::settings_reload::SettingsSource;
use std::path::PathBuf;

mod launcher;
mod platform;
mod viewer;
use viewer::Viewer;

#[derive(Clone, Parser)]
#[command(about = "Jaywalk: a native Jujutsu diff viewer")]
struct Arguments {
    #[arg(default_value = ".")]
    directory: PathBuf,
    #[arg(long, conflicts_with = "diff")]
    base: Option<String>,
    #[arg(long, default_value = "@", conflicts_with = "diff")]
    tip: String,
    /// Compare two files or directory trees, without requiring a repository.
    #[arg(long, num_args = 2, value_names = ["LEFT", "RIGHT"])]
    diff: Option<Vec<PathBuf>>,
    /// Wait until the launched window closes (required for temporary diff inputs).
    #[arg(long)]
    wait: bool,
    #[arg(long)]
    inspect: bool,
    /// Keep the GUI attached to this terminal (useful for debugging).
    #[arg(long)]
    foreground: bool,
    // A bundled executable reached through jw still needs normal CLI detachment.
    #[arg(long, hide = true)]
    terminal_launch: bool,
    #[arg(long)]
    file: Option<String>,
    /// Read settings from this TOML file instead of the default config path.
    #[arg(long)]
    config: Option<PathBuf>,
}

impl Arguments {
    fn comparison(&self) -> std::result::Result<Comparison, String> {
        match &self.diff {
            Some(paths) => jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]),
            None => Comparison::load(&self.directory, self.base.as_deref(), &self.tip),
        }
    }
}

fn main() {
    let arguments = Arguments::parse();
    if arguments.inspect {
        match arguments.comparison() {
            Ok(comparison) => println!(
                "{} changed files\nbase {}\ntip {}\noperation {}",
                comparison.files.len(),
                comparison.base,
                comparison.tip,
                comparison.operation
            ),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    let settings = match SettingsSource::resolve(arguments.config.as_deref())
        .and_then(|source| source.read())
    {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    if !arguments.foreground
        && !arguments.wait
        && (arguments.terminal_launch || !platform::is_app_bundle())
    {
        if let Err(error) = launcher::launch(arguments.config.as_deref()) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    gpui::application()
        .with_assets(gpui::assets::AllAssets)
        .run(move |cx| {
            gpui::init(cx);
            if arguments.wait {
                cx.set_quit_mode(QuitMode::LastWindowClosed);
            }
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
            if let Err(error) = open_viewer(arguments, settings, cx) {
                eprintln!("Could not open the viewer window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
}

fn open_viewer(
    arguments: Arguments,
    settings: jaywalk::settings::Settings,
    cx: &mut App,
) -> Result<()> {
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("Jaywalk".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(settings.window.width), px(settings.window.height)),
            cx,
        ))),
        ..Default::default()
    };
    gpui::open_window(options, cx, |window, cx| {
        cx.new(|cx| Viewer::new(arguments, settings, window, cx))
    })?;
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
                    .background_spawn(
                        async move { SettingsSource::resolve(config.as_deref())?.read() },
                    )
                    .await;
                match settings {
                    Ok(settings) => {
                        cx.update(|cx| {
                            if let Err(error) = open_viewer(arguments, settings, cx) {
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
