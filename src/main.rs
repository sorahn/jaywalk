extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::component::{Theme, ThemeMode};
use gpui::*;
use jaywalk::repository::Comparison;
use std::path::PathBuf;

mod viewer;
use viewer::Viewer;

#[derive(Clone, Parser)]
#[command(about = "Jaywalk: a native Jujutsu diff viewer")]
struct Arguments {
    #[arg(default_value = ".")]
    directory: PathBuf,
    #[arg(long)]
    base: Option<String>,
    #[arg(long, default_value = "@")]
    tip: String,
    #[arg(long)]
    inspect: bool,
    #[arg(long)]
    file: Option<String>,
}

fn main() {
    let arguments = Arguments::parse();
    if arguments.inspect {
        match Comparison::load(
            &arguments.directory,
            arguments.base.as_deref(),
            &arguments.tip,
        ) {
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

    gpui::application()
        .with_assets(gpui::assets::AllAssets)
        .run(move |cx| {
            gpui::init(cx);
            viewer::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            configure_code_theme(cx);
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Jaywalk".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1500.), px(950.)),
                    cx,
                ))),
                ..Default::default()
            };
            if let Err(error) = gpui::open_window(options, cx, |window, cx| {
                cx.new(|cx| Viewer::new(arguments, window, cx))
            }) {
                eprintln!("Could not open the viewer window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
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
