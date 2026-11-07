extern crate gpui_kit as gpui;

use gpui::component::{StyledExt, Theme, ThemeMode};
use gpui::*;

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

fn main() {
    let repository = std::env::args().nth(1).unwrap_or_else(|| ".".into());
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
