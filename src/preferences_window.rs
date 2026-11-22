//! Settings has its own window and never takes over a repository viewer.
use crate::{
    Arguments,
    preferences_ui::{Event, SettingsPage},
    viewer::commands::ExecuteCommand,
};
use gpui::component::Root;
use gpui::*;
use jaywalk::{commands::CommandId, settings::Settings, settings_reload::SettingsSource};
use std::collections::HashMap;

#[derive(Default)]
struct SettingsWindows(HashMap<SettingsSource, (AnyWindowHandle, WeakEntity<SettingsPage>)>);
impl Global for SettingsWindows {}

pub(crate) fn show(
    arguments: Arguments,
    settings: Settings,
    defaults: bool,
    cx: &mut App,
) -> std::result::Result<(AnyWindowHandle, Entity<SettingsPage>), String> {
    let source = SettingsSource::resolve(arguments.config.as_deref())?;
    if !cx.has_global::<SettingsWindows>() {
        cx.set_global(SettingsWindows::default());
        cx.on_window_closed(|cx, id| {
            cx.global_mut::<SettingsWindows>()
                .0
                .retain(|_, (handle, _)| handle.window_id() != id)
        })
        .detach();
    }
    if let Some((handle, page)) = cx.global::<SettingsWindows>().0.get(&source).cloned()
        && let Some(page) = page.upgrade()
        && handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.show_defaults(defaults, cx);
                    page.focus(window, cx);
                });
                window.activate_window();
            })
            .is_ok()
    {
        cx.activate(true);
        return Ok((handle, page));
    }
    let options = WindowOptions {
        app_id: Some("io.github.sorahn.jaywalk".into()),
        titlebar: Some(TitlebarOptions {
            title: Some("Jaywalk Settings".into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1050.), px(740.)),
            cx,
        ))),
        ..Default::default()
    };
    let mut page = None;
    let key = source.clone();
    let (handle, _) = gpui::open_window(options, cx, |window, cx| {
        crate::appearance::watch_window(window, cx);
        let content = cx.new(|cx| {
            let settings_page =
                cx.new(|cx| SettingsPage::new(source.clone(), &settings, window, cx));
            settings_page.update(cx, |page, cx| {
                page.show_defaults(defaults, cx);
                page.focus(window, cx);
            });
            page = Some(settings_page.clone());
            let subscription = cx.subscribe_in(
                &settings_page,
                window,
                |view: &mut SettingsWindow, _, event, window, cx| match event {
                    Event::Leave => window.remove_window(),
                    Event::EditToml => view.edit_toml(window, cx),
                    Event::Accounts | Event::Keybindings => {
                        let command = if matches!(event, Event::Accounts) {
                            CommandId::Accounts
                        } else {
                            CommandId::SelectKeybindings
                        };
                        let arguments = view.arguments.clone();
                        cx.defer(move |cx| crate::show_application_page(arguments, command, cx));
                    }
                },
            );
            SettingsWindow {
                page: settings_page,
                arguments,
                source,
                _subscription: subscription,
            }
        });
        cx.new(|cx| Root::new(content, window, cx))
    })
    .map_err(|error| error.to_string())?;
    let page = page.expect("window builder creates settings content");
    cx.global_mut::<SettingsWindows>()
        .0
        .insert(key, (handle, page.downgrade()));
    cx.activate(true);
    Ok((handle, page))
}
struct SettingsWindow {
    page: Entity<SettingsPage>,
    arguments: Arguments,
    source: SettingsSource,
    _subscription: Subscription,
}
impl SettingsWindow {
    fn edit_toml(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.source.clone();
        cx.spawn_in(window, async move |view, cx| {
            let result = cx
                .background_spawn(async move {
                    source.initialize()?;
                    let snapshot = source.read_snapshot()?;
                    crate::platform::open_text_file(
                        source.path(),
                        snapshot.settings.settings.editor.as_ref(),
                    )
                })
                .await;
            if let Err(error) = result {
                let _ = view.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        "Could not open settings",
                        Some(&error),
                        &["OK"],
                        cx,
                    )
                });
            }
        })
        .detach();
    }
}
impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .key_context("Jaywalk")
            .bg(gpui::component::Theme::global(cx).background)
            .text_color(gpui::component::Theme::global(cx).foreground)
            .on_action(cx.listener(
                |view, action: &ExecuteCommand, window, cx| match action.command {
                    CommandId::CloseWindow => window.remove_window(),
                    CommandId::OpenSettings => view.edit_toml(window, cx),
                    CommandId::ShowSettings | CommandId::ShowDefaultSettings => {
                        view.page.update(cx, |page, cx| {
                            page.show_defaults(action.command == CommandId::ShowDefaultSettings, cx)
                        })
                    }
                    _ => cx.propagate(),
                },
            ))
            .child(self.page.clone())
    }
}
