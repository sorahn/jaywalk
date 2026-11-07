extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::resizable::{h_resizable, resizable_panel};
use gpui::component::{StyledExt, Theme, ThemeMode};
use gpui::*;
use jaywalk::repository::Comparison;
use std::path::PathBuf;

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
}

struct Viewer {
    arguments: Arguments,
    comparison: Result<Comparison, String>,
    selected: Option<usize>,
}

impl Viewer {
    fn new(arguments: Arguments) -> Self {
        let comparison = Comparison::load(
            &arguments.directory,
            arguments.base.as_deref(),
            &arguments.tip,
        );
        Self {
            arguments,
            comparison,
            selected: None,
        }
    }

    fn select_file(&mut self, index: usize, _: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(index);
        cx.notify();
    }

    fn refresh(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let selected_path = self
            .selected
            .and_then(|index| self.comparison.as_ref().ok()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.comparison = Comparison::load(
            &self.arguments.directory,
            self.arguments.base.as_deref(),
            &self.arguments.tip,
        );
        self.selected = selected_path.and_then(|path| {
            self.comparison
                .as_ref()
                .ok()?
                .files
                .iter()
                .position(|file| file.after_path == path)
        });
        cx.notify();
    }

    fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .v_flex()
            .size_full()
            .bg(rgb(0x253448))
            .child(div().p_3().text_sm().child("CHANGED FILES / debug blue"));

        match &self.comparison {
            Err(error) => panel = panel.child(div().p_3().child(error.clone())),
            Ok(comparison) => {
                let mut list = div()
                    .id("changed-files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll();
                for (index, file) in comparison.files.iter().enumerate() {
                    let background = if self.selected == Some(index) {
                        0x345577
                    } else {
                        0x253448
                    };
                    let marker = match file.status.as_str() {
                        "added" => "A",
                        "removed" => "D",
                        "renamed" => "R",
                        "copied" => "C",
                        _ => "M",
                    };
                    let label = format!("{marker}  {}", file.after_path);
                    list = list.child(
                        div()
                            .id(("file", index))
                            .px_3()
                            .py_2()
                            .bg(rgb(background))
                            .text_sm()
                            .cursor_pointer()
                            .child(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_file(index, window, cx)
                            })),
                    );
                }
                panel = panel.child(list);
            }
        }
        panel.into_any_element()
    }

    fn render_comparison(&self) -> AnyElement {
        match &self.comparison {
            Err(error) => div().p_6().child(error.clone()).into_any_element(),
            Ok(comparison) => {
                let heading = self
                    .selected
                    .map(|index| comparison.files[index].after_path.clone())
                    .unwrap_or_else(|| "Changeset overview".into());
                div().v_flex().size_full().bg(rgb(0x302b3d)).gap_4().p_6()
                    .child(div().text_xl().child(heading))
                    .child(format!("{} changed files", comparison.files.len()))
                    .child(format!("Base: {}", comparison.base))
                    .child(format!("Tip: {}", comparison.tip))
                    .child(format!("Operation: {}", &comparison.operation[..12]))
                    .child("Choose a file from the list. Each refresh reads one recorded jj operation.")
                    .into_any_element()
            }
        }
    }
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .bg(rgb(0x1f1f1f))
            .text_color(rgb(0xcccccc))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .bg(rgb(0x263d35))
                    .child(format!(
                        "Jaywalk  /  {}",
                        self.arguments.directory.display()
                    ))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(Button::new("overview").ghost().label("Overview").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.selected = None;
                                    cx.notify();
                                }),
                            ))
                            .child(Button::new("refresh").ghost().label("Refresh").on_click(
                                cx.listener(|this, _, window, cx| this.refresh(window, cx)),
                            )),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("viewer-layout")
                        .child(
                            resizable_panel()
                                .size(px(340.))
                                .size_range(px(200.)..px(700.))
                                .child(self.render_files(cx)),
                        )
                        .child(resizable_panel().child(self.render_comparison())),
                ),
            )
            .child(
                div().px_3().py_2().bg(rgb(0x3d3226)).text_sm().child(
                    "Read-only jj snapshot  |  Branch comparison  |  Prototype panel shading",
                ),
            )
    }
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
                size(px(1500.), px(950.)),
                cx,
            ))),
            ..Default::default()
        };
        gpui::open_window(options, cx, |_, cx| cx.new(|_| Viewer::new(arguments)))
            .expect("could not open viewer window");
        cx.activate(true);
    });
}
