extern crate gpui_kit as gpui;

use clap::Parser;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::input::{Editor, EditorState, RangeDecoration, RangeDecorationStyle};
use gpui::component::resizable::{h_resizable, resizable_panel};
use gpui::component::{StyledExt, Theme, ThemeMode};
use gpui::*;
use jaywalk::diff_model::FileDiff;
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
    #[arg(long)]
    file: Option<String>,
}

struct Viewer {
    arguments: Arguments,
    comparison: Result<Comparison, String>,
    selected: Option<usize>,
    code: Option<(Entity<EditorState>, Entity<EditorState>, FileDiff)>,
    file_error: Option<String>,
}

impl Viewer {
    fn new(arguments: Arguments, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let comparison = Comparison::load(
            &arguments.directory,
            arguments.base.as_deref(),
            &arguments.tip,
        );
        let mut viewer = Self {
            arguments,
            comparison,
            selected: None,
            code: None,
            file_error: None,
        };
        let selected_file = viewer.arguments.file.as_ref().and_then(|path| {
            viewer
                .comparison
                .as_ref()
                .ok()?
                .files
                .iter()
                .position(|file| &file.after_path == path)
        });
        if let Some(index) = selected_file {
            viewer.select_file(index, window, cx);
        }
        viewer
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(index);
        self.code = None;
        self.file_error = None;
        if let Ok(comparison) = &self.comparison {
            let file = &comparison.files[index];
            match comparison.contents(file) {
                Ok((before, after)) => {
                    let model = FileDiff::new(&before, &after);
                    let extension = std::path::Path::new(&file.after_path)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("");
                    let language = match extension {
                        "ts" => "typescript",
                        "tsx" => "tsx",
                        "js" | "jsx" => "javascript",
                        _ => "plain",
                    };
                    let make_editor =
                        |text: String,
                         before: bool,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
                            cx.new(|cx| {
                                let mut editor = EditorState::new(window, cx)
                                    .language(language)
                                    .soft_wrap(false)
                                    .folding(false)
                                    .default_value(text);
                                let decorations = model
                                    .blocks
                                    .iter()
                                    .filter(|block| block.changed)
                                    .map(|block| {
                                        let color = if before {
                                            rgba(0xff000028)
                                        } else {
                                            rgba(0x00ff0028)
                                        };
                                        RangeDecoration::new(model.byte_range(block, before))
                                            .with_style(RangeDecorationStyle::Fill)
                                            .with_color(color.into())
                                    })
                                    .collect();
                                editor.create_range_decorations_collection(decorations, cx);
                                editor
                            })
                        };
                    let left = make_editor(before, true, window, cx);
                    let right = make_editor(after, false, window, cx);
                    self.code = Some((left, right, model));
                }
                Err(error) => self.file_error = Some(error),
            }
        }
        cx.notify();
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.code = None;
        if let Some(index) = self.selected {
            self.select_file(index, window, cx);
        }
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
        if let Some(error) = &self.file_error {
            return div().p_6().child(error.clone()).into_any_element();
        }
        if let Some((left, right, _)) = &self.code {
            return div()
                .v_flex()
                .size_full()
                .child(
                    div().px_3().py_2().bg(rgb(0x302b3d)).child(
                        self.comparison.as_ref().unwrap().files[self.selected.unwrap()]
                            .after_path
                            .clone(),
                    ),
                )
                .child(
                    div().flex_1().min_h_0().child(
                        h_resizable("code-panes")
                            .child(
                                resizable_panel().child(
                                    div()
                                        .v_flex()
                                        .size_full()
                                        .child(div().px_3().py_1().child("BEFORE"))
                                        .child(
                                            Editor::new(left)
                                                .readonly(true)
                                                .bordered(false)
                                                .h_full(),
                                        ),
                                ),
                            )
                            .child(
                                resizable_panel().child(
                                    div()
                                        .v_flex()
                                        .size_full()
                                        .child(div().px_3().py_1().child("AFTER"))
                                        .child(
                                            Editor::new(right)
                                                .readonly(true)
                                                .bordered(false)
                                                .h_full(),
                                        ),
                                ),
                            ),
                    ),
                )
                .into_any_element();
        }
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
                                    this.code = None;
                                    this.file_error = None;
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
        gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| Viewer::new(arguments, window, cx))
        })
        .expect("could not open viewer window");
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
