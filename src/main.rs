extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::component::{StyledExt, Theme, ThemeMode};
use gpui::*;
use jaywalk::repository::Comparison;
use std::path::PathBuf;

struct Viewer {
    repository: String,
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .bg(rgb(0x1f1f1f))
            .text_color(rgb(0xcccccc))
            .child(
                div()
                    .p_4()
                    .bg(rgb(0x263849))
                    .child("Jaywalk / native prototype"),
            )
            .child(div().p_4().child(self.repository.clone()))
            .child(
                div()
                    .p_4()
                    .child("Choose a file to inspect its comparison."),
            )
    }
}

#[derive(Parser)]
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
    let repository = arguments.directory.display().to_string();
    gpui::application().run(move |cx| {
        gpui::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Jaywalk".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1400.), px(900.)),
                cx,
            ))),
            ..Default::default()
        };
        gpui::open_window(options, cx, |_, cx| cx.new(|_| Viewer { repository }))
            .expect("could not open viewer window");
        cx.activate(true);
    });
}
