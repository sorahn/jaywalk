//! Thin controls over the async local asset service; selection uses normal drafts.
use super::*;
use gpui::component::IconName;
impl SettingsPage {
    pub(crate) fn show_themes(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_defaults(false, cx);
        self.query
            .update(cx, |input, cx| input.set_value("theme.code", window, cx));
        self.filter(cx);
        match command {
            CommandId::ImportCodeTheme => self.import_theme(None, window, cx),
            CommandId::InspectCodeTheme => self.inspect_theme(window, cx),
            _ => {}
        }
    }
    pub(super) fn theme_control(
        &self,
        index: usize,
        disabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dark = catalogue::SETTINGS[index].key.ends_with(".dark");
        let builtin = if dark {
            "builtin:dark"
        } else {
            "builtin:light"
        };
        let value = self
            .value(index)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| builtin.into());
        let service = crate::theme_service::service(cx);
        let state = service.read(cx);
        let entries = state
            .catalog
            .entries
            .iter()
            .filter(|entry| entry.mode.is_none_or(|mode| mode.dark() == dark))
            .cloned()
            .collect::<Vec<_>>();
        let label = if value == builtin {
            "Jaywalk built-in".into()
        } else {
            entries
                .iter()
                .find(|entry| entry.id == value)
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| "Unavailable theme".into())
        };
        let warning = state.diagnostics.first().cloned();
        let inspect_label = warning.as_ref().map_or_else(
            || "Inspect active code themes and import coverage".to_owned(),
            |message| format!("Inspect code themes — {message}"),
        );
        let owner = cx.entity().downgrade();
        div()
            .h_flex()
            .items_center()
            .gap(crate::spacing::space(1., cx))
            .child(
                Button::new(("setting-theme", index))
                    .debug_selector(move || format!("setting-{}", catalogue::SETTINGS[index].key))
                    .small()
                    .label(label)
                    .dropdown_caret(true)
                    .disabled(disabled)
                    .dropdown_menu(move |menu, _, _| {
                        let owner = owner.clone();
                        let reset = owner.clone();
                        let mut menu = menu.item(
                            PopupMenuItem::new("Jaywalk built-in")
                                .checked(value == builtin)
                                .on_click(move |_, window, cx| {
                                    let _ = reset.update(cx, |page, cx| {
                                        page.stage(index, Some(builtin.into()), window, cx)
                                    });
                                }),
                        );
                        for entry in &entries {
                            let owner = owner.clone();
                            let id = entry.id.clone();
                            let duplicate =
                                entries.iter().filter(|e| e.name == entry.name).count() > 1;
                            let label = if duplicate {
                                format!("{} ({})", entry.name, &entry.id[7..15])
                            } else {
                                entry.name.clone()
                            };
                            menu =
                                menu.item(PopupMenuItem::new(label).checked(value == id).on_click(
                                    move |_, window, cx| {
                                        let _ = owner.update(cx, |page, cx| {
                                            page.stage(index, Some(id.clone().into()), window, cx)
                                        });
                                    },
                                ));
                        }
                        menu
                    }),
            )
            .child(
                Button::new(("setting-theme-import", index))
                    .small()
                    .icon(IconName::FolderOpen)
                    .disabled(disabled)
                    .command_tooltip("Import local VS Code theme…", CommandId::ImportCodeTheme)
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.import_theme(Some(index), window, cx)
                    })),
            )
            .child(
                Button::new(("setting-theme-inspect", index))
                    .small()
                    .icon(IconName::Info)
                    .command_tooltip(inspect_label, CommandId::InspectCodeTheme)
                    .when(warning.is_some(), |button| {
                        button.text_color(gpui::component::Theme::global(cx).warning)
                    })
                    .on_click(cx.listener(|page, _, window, cx| page.inspect_theme(window, cx))),
            )
            .into_any_element()
    }
    fn inspect_theme(&self, window: &mut Window, cx: &mut Context<Self>) {
        let service = crate::theme_service::service(cx);
        let state = service.read(cx);
        let mut message = String::from(
            "Local themes are linked read-only. Syntax uses approximate TextMate-to-Tree-sitter mapping; semantic tokens and scope stacks are not reproduced.\n",
        );
        for dark in [false, true] {
            let key = if dark {
                "theme.code.dark"
            } else {
                "theme.code.light"
            };
            let id = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.value(key))
                .and_then(toml::Value::as_str)
                .unwrap_or("built-in");
            message.push_str(&format!("\n{}: {id}", if dark { "Dark" } else { "Light" }));
            if let Some(entry) = state.catalog.entries.iter().find(|entry| entry.id == id) {
                message.push_str(&format!("\n{}\n{}", entry.name, entry.source.display()));
                if let Some(asset) = state.selected[usize::from(dark)]
                    .as_ref()
                    .filter(|asset| asset.source == entry.source)
                {
                    message.push_str(&format!(
                        "\nContent: {}\n{} dependency files",
                        asset.identity,
                        asset.dependencies.len()
                    ));
                    for path in &asset.dependencies {
                        message.push_str(&format!("\n{}", path.display()));
                    }
                }
            }
        }
        for diagnostic in state.diagnostics.iter().take(12) {
            message.push_str(&format!("\n\n{diagnostic}"));
        }
        if state.diagnostics.is_empty() {
            message.push_str("\n\nNo import diagnostics.");
        }
        let summary = message.chars().take(1600).collect::<String>();
        let prompt = window.prompt(
            PromptLevel::Info,
            "Code theme inspection",
            Some(&summary),
            &["OK", "Copy details"],
            cx,
        );
        cx.spawn_in(window, async move |_, cx| {
            if prompt.await == Ok(1) {
                let _ =
                    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(message)));
            }
        })
        .detach();
    }
    fn import_theme(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_prompt_serial += 1;
        let serial = self.theme_prompt_serial;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import VS Code theme JSON/JSONC".into()),
        });
        let config = self.source.path().to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let path = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => return,
                Err(error) => {
                    let _ = this.update(cx, |page, cx| {
                        if page.theme_prompt_serial == serial {
                            page.message = Some(format!("Theme file chooser failed: {error}"));
                            cx.notify();
                        }
                    });
                    return;
                }
                Ok(Err(error)) => {
                    let _ = this.update(cx, |page, cx| {
                        if page.theme_prompt_serial == serial {
                            page.message = Some(format!("Theme file chooser failed: {error}"));
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            // A closed window invalidates the prompt before any catalog write.
            if !this
                .update(cx, |page, _| page.theme_prompt_serial == serial)
                .unwrap_or(false)
            {
                return;
            }
            let result = cx
                .background_spawn(async move { jaywalk::theme::assets::import(&config, &path) })
                .await;
            let _ = this.update_in(cx, |page, window, cx| {
                if page.theme_prompt_serial != serial {
                    return;
                }
                match result {
                    Ok(entry) => {
                        crate::theme_service::rescan(cx);
                        let dark = entry.mode.map_or_else(
                            || {
                                index.map_or(gpui::component::Theme::global(cx).is_dark(), |i| {
                                    catalogue::SETTINGS[i].key.ends_with(".dark")
                                })
                            },
                            |m| m.dark(),
                        );
                        let target = if dark {
                            "theme.code.dark"
                        } else {
                            "theme.code.light"
                        };
                        let index = index
                            .filter(|index| catalogue::SETTINGS[*index].key == target)
                            .or_else(|| {
                                catalogue::SETTINGS
                                    .iter()
                                    .position(|setting| setting.key == target)
                            });
                        if let Some(index) = index {
                            page.stage(index, Some(entry.id.into()), window, cx);
                        }
                        page.message = Some(format!("Imported {}.", entry.name));
                    }
                    Err(error) => {
                        page.message = Some(format!("Could not import code theme: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
