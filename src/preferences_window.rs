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

pub(crate) fn themes(arguments: Arguments, settings: Settings, command: CommandId, cx: &mut App) {
    match show(arguments, settings, false, cx) {
        Ok((handle, page)) => {
            let _ = handle.update(cx, |_, window, cx| {
                page.update(cx, |page, cx| page.show_themes(command, window, cx))
            });
        }
        Err(error) => eprintln!("Could not open code theme settings: {error}"),
    }
}
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
                    page.set_repository_context(
                        arguments
                            .interface_repository()
                            .map(std::path::Path::to_owned),
                        cx,
                    );
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
                page.set_repository_context(
                    arguments
                        .interface_repository()
                        .map(std::path::Path::to_owned),
                    cx,
                );
                page.show_defaults(defaults, cx);
                page.focus(window, cx);
            });
            page = Some(settings_page.clone());
            let subscription = cx.subscribe_in(
                &settings_page,
                window,
                |view: &mut SettingsWindow, _, event, window, cx| match event {
                    Event::Leave => view
                        .page
                        .update(cx, |page, cx| page.request_close(false, window, cx)),
                    Event::Close { quit } => {
                        window.remove_window();
                        if *quit {
                            request_quit(cx);
                        }
                    }
                    Event::EditToml => view.edit_toml(window, cx),
                    Event::Accounts => view.page.update(cx, |page, cx| {
                        page.show_accounts(CommandId::Accounts, window, cx)
                    }),
                    Event::Keybindings => view
                        .page
                        .update(cx, |page, cx| page.show_keyboard(window, cx)),
                },
            );
            SettingsWindow {
                page: settings_page,
                source,
                _subscription: subscription,
            }
        });
        let page = page.as_ref().expect("settings page created").downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = page.update(cx, |page, cx| page.request_close(false, window, cx));
            false
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
        crate::appearance::window_content(cx)
            .size_full()
            .key_context("Jaywalk")
            .bg(gpui::component::Theme::global(cx).background)
            .text_color(gpui::component::Theme::global(cx).foreground)
            .on_action(cx.listener(|view, action: &ExecuteCommand, window, cx| {
                match action.command {
                    CommandId::CloseWindow => view
                        .page
                        .update(cx, |page, cx| page.request_close(false, window, cx)),
                    CommandId::SelectKeybindings => view
                        .page
                        .update(cx, |page, cx| page.show_keyboard(window, cx)),
                    CommandId::OpenSettings => view.edit_toml(window, cx),
                    CommandId::ShowSettings | CommandId::ShowDefaultSettings => {
                        view.page.update(cx, |page, cx| {
                            page.show_defaults(action.command == CommandId::ShowDefaultSettings, cx)
                        })
                    }
                    _ => cx.propagate(),
                }
            }))
            .child(self.page.clone())
    }
}

/// Menu actions may run during a window update; inspect saved handles after it ends.
pub(crate) fn request_quit(cx: &mut App) {
    cx.defer(|cx| {
        let pending = cx.try_global::<SettingsWindows>().and_then(|windows| {
            windows.0.values().find_map(|(handle, page)| {
                page.upgrade()
                    .filter(|page| page.read(cx).has_changes())
                    .map(|page| (*handle, page))
            })
        });
        if let Some((handle, page)) = pending {
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                page.update(cx, |page, cx| page.request_close(true, window, cx));
            });
        } else {
            cx.quit();
        }
    });
}

/// Accounts shares the existing Settings window and preserves its draft/query.
pub(crate) fn show_accounts(
    arguments: Arguments,
    settings: Settings,
    command: CommandId,
    cx: &mut App,
) -> std::result::Result<(AnyWindowHandle, Entity<SettingsPage>), String> {
    let (handle, page) = show(arguments, settings, false, cx)?;
    handle
        .update(cx, |_, window, cx| {
            page.update(cx, |page, cx| page.show_accounts(command, window, cx));
        })
        .map_err(|error| error.to_string())?;
    Ok((handle, page))
}

/// Direct palette entry uses the same retained Settings window and drafts.
pub(crate) fn show_keyboard(
    arguments: Arguments,
    settings: Settings,
    cx: &mut App,
) -> std::result::Result<(AnyWindowHandle, Entity<SettingsPage>), String> {
    let (handle, page) = show(arguments, settings, false, cx)?;
    handle
        .update(cx, |_, window, cx| {
            page.update(cx, |page, cx| page.show_keyboard(window, cx))
        })
        .map_err(|e| e.to_string())?;
    Ok((handle, page))
}
