//! A small shared About window, available even with no repository open.
use crate::viewer::commands::ExecuteCommand;
use gpui::component::StyledExt;
use gpui::component::button::Button;
use gpui::*;
use jaywalk::commands::CommandId;

#[derive(Default)]
struct AboutWindow(Option<AnyWindowHandle>);
impl Global for AboutWindow {}
struct AboutView {
    focus: FocusHandle,
}

pub(crate) fn show(cx: &mut App) -> Result<()> {
    if let Some(window) = cx.try_global::<AboutWindow>().and_then(|state| state.0)
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(());
    }
    let options = WindowOptions {
        app_id: Some("io.github.sorahn.jaywalk".into()),
        titlebar: Some(TitlebarOptions {
            title: Some("About Jaywalk".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(520.), px(280.)),
            cx,
        ))),
        is_resizable: false,
        ..Default::default()
    };
    let (window, _) = gpui::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            AboutView { focus }
        })
    })?;
    cx.set_global(AboutWindow(Some(window)));
    cx.activate(true);
    Ok(())
}

impl Render for AboutView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("about-jaywalk")
            .key_context("Jaywalk")
            .track_focus(&self.focus)
            .on_action(|action: &ExecuteCommand, window, cx| {
                if action.command == CommandId::CloseWindow {
                    window.remove_window();
                } else {
                    cx.propagate();
                }
            })
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .p_6()
            .bg(rgb(0x202124))
            .text_color(rgb(0xdddddd))
            .child(div().text_xl().child("Jaywalk"))
            .child(
                div()
                    .text_sm()
                    .child(format!("Version {}", jaywalk::version::DISPLAY)),
            )
            .child(
                div()
                    .text_sm()
                    .child(format!("Build {}", jaywalk::version::REVISION)),
            )
            .child(div().text_sm().child("Native Git and Jujutsu diff viewer"))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(Button::new("copy-version").label("Copy version").on_click(
                        |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(format!(
                                "Jaywalk {}\nBuild {}",
                                jaywalk::version::DISPLAY,
                                jaywalk::version::REVISION
                            )));
                        },
                    ))
                    .child(
                        Button::new("close-about")
                            .label("Close")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
