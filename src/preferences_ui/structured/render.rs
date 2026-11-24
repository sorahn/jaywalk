//! Presentation of the three focused editors. Draft changes stay in the parent.
use super::*;
impl SettingsPage {
    pub(in crate::preferences_ui) fn structured_control(
        &mut self,
        index: usize,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_structured(index, window, cx);
        let form = &self.structured.forms[&index];
        let disabled = disabled || self.busy.is_some();
        if let Shape::Editor { mode, .. } = &form.shape {
            let mode = *mode;
            let owner = cx.entity().downgrade();
            Button::new(("editor-mode", index))
                .small()
                .label(mode.label())
                .dropdown_caret(true)
                .disabled(disabled)
                .debug_selector(|| "setting-editor-mode".into())
                .dropdown_menu(move |menu, _, _| {
                    [Mode::System, Mode::Application, Mode::Command]
                        .into_iter()
                        .fold(menu, |menu, next| {
                            let owner = owner.clone();
                            menu.item(
                                PopupMenuItem::new(next.label())
                                    .checked(mode == next)
                                    .on_click(move |_, window, cx| {
                                        let _ = owner.update(cx, |page, cx| {
                                            page.structured_action(
                                                index,
                                                Action::Mode(next),
                                                window,
                                                cx,
                                            )
                                        });
                                    }),
                            )
                        })
                })
                .into_any_element()
        } else {
            let label = match &form.shape {
                Shape::Folders(fields) => format!("{} folders", fields.len()),
                Shape::Remotes { rules, .. } => format!("{} overrides", rules.len()),
                _ => unreachable!(),
            };
            Button::new(("structured-expand", index))
                .small()
                .label(label)
                .dropdown_caret(true)
                .toggled(form.expanded)
                .disabled(disabled)
                .debug_selector(move || {
                    format!("setting-structured-{}", catalogue::SETTINGS[index].key)
                })
                .on_click(cx.listener(move |page, _, _, cx| {
                    let form = page.structured.forms.get_mut(&index).unwrap();
                    form.expanded = !form.expanded;
                    page.rebuild_rows(cx);
                    cx.notify();
                }))
                .into_any_element()
        }
    }
    fn structured_button(
        &self,
        index: usize,
        label: &'static str,
        action: Action,
        disabled: bool,
        cx: &Context<Self>,
    ) -> Button {
        let id = match action {
            Action::Remove(id) | Action::Move(id, _) => id,
            _ => index,
        };
        Button::new((label, id))
            .debug_selector({
                let position = self.structured.forms[&index]
                    .shape
                    .fields()
                    .iter()
                    .position(|field| field.id == id)
                    .unwrap_or(0);
                let name = match action {
                    Action::Add => "add",
                    Action::Remove(_) => "remove",
                    Action::Move(_, true) => "up",
                    Action::Move(_, false) => "down",
                    Action::Mode(_) => "mode",
                };
                move || {
                    format!(
                        "structured-{name}-{}-{position}",
                        catalogue::SETTINGS[index].key
                    )
                }
            })
            .small()
            .when(matches!(action, Action::Remove(_)), |button| {
                button
                    .icon(gpui::assets::IconName::X)
                    .compact()
                    .ghost()
                    .accessibility_label("Remove row")
                    .tooltip("Remove row")
            })
            .when(matches!(action, Action::Add), |button| {
                button.label("Add new")
            })
            .disabled(disabled)
            .on_click(cx.listener(move |page, _, window, cx| {
                page.structured_action(index, action, window, cx)
            }))
    }
    pub(in crate::preferences_ui) fn structured_details(
        &mut self,
        index: usize,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_structured(index, window, cx);
        let form = &self.structured.forms[&index];
        if !form.expanded {
            return div().into_any_element();
        }
        let disabled = disabled || self.busy.is_some();
        let mut body = div()
            .v_flex()
            .gap(crate::spacing::space(2., cx))
            .pt(crate::spacing::space(2., cx))
            .debug_selector(move || {
                format!("structured-details-{}", catalogue::SETTINGS[index].key)
            });
        let fields: Vec<_> = match &form.shape {
            Shape::Editor {
                mode,
                program,
                arguments,
            } => {
                if *mode == Mode::System {
                    return div().into_any_element();
                }
                let id = program.id;
                body = body.child(
                    div()
                        .h_flex()
                        .flex_wrap()
                        .gap(crate::spacing::space(2., cx))
                        .child(
                            div()
                                .w(px(360.))
                                .max_w_full()
                                .debug_selector(|| "editor-program".into())
                                .child(Input::new(&program.input).small().disabled(disabled)),
                        )
                        .child(
                            Button::new("browse-editor")
                                .debug_selector(|| "editor-browse".into())
                                .small()
                                .icon(gpui::assets::IconName::FolderOpen)
                                .compact()
                                .ghost()
                                .accessibility_label("Choose application or executable")
                                .tooltip("Choose application or executable")
                                .disabled(disabled)
                                .on_click(cx.listener(move |page, _, window, cx| {
                                    page.structured_browse(index, id, false, window, cx)
                                })),
                        ),
                );
                if *mode == Mode::Command {
                    body=body.child(div().text_sm().text_color(gpui::component::Theme::global(cx).muted_foreground).child("One literal argument per field. The settings-file path is appended."));
                    arguments.iter().collect()
                } else {
                    vec![]
                }
            }
            Shape::Folders(fields) => fields.iter().collect(),
            Shape::Remotes { rules, names, name } => {
                let default = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.value("history.remotes.default_visible"))
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true);
                let context = self
                    .structured
                    .repository
                    .as_ref()
                    .map(|path| {
                        format!(
                            "Remote names from {}. Rules apply wherever these names occur.",
                            path.display()
                        )
                    })
                    .unwrap_or_else(|| {
                        "Saved names are available without an open repository.".into()
                    });
                body = body.child(
                    div()
                        .text_sm()
                        .text_color(gpui::component::Theme::global(cx).muted_foreground)
                        .whitespace_normal()
                        .child(context),
                );
                body = body.child(div().text_sm().child(format!(
                    "Default currently means {}.",
                    if default { "Show" } else { "Hide" }
                )));
                if self.structured.remote_loading {
                    body = body.child("Loading remote names…");
                }
                if let Some(error) = &self.structured.remote_error {
                    body = body.child(error.clone());
                }
                let all: BTreeSet<_> = names
                    .iter()
                    .cloned()
                    .chain(rules.keys().cloned())
                    .chain(self.structured.known_remotes.iter().cloned())
                    .collect();
                for name in all {
                    let selected = rules.get(&name).and_then(toml::Value::as_bool);
                    let label = match selected {
                        None => "Default",
                        Some(true) => "Show",
                        Some(false) => "Hide",
                    };
                    let owner = cx.entity().downgrade();
                    let remote = name.clone();
                    body = body.child(
                        div()
                            .h_flex()
                            .flex_wrap()
                            .gap(crate::spacing::space(2., cx))
                            .child(div().w(px(260.)).min_w_0().truncate().child(name.clone()))
                            .child(
                                Button::new(SharedString::from(format!("remote-{name}")))
                                    .small()
                                    .label(label)
                                    .dropdown_caret(true)
                                    .disabled(disabled)
                                    .debug_selector(move || format!("remote-rule-{name}"))
                                    .dropdown_menu(move |menu, _, _| {
                                        [
                                            ("Default", None),
                                            ("Show", Some(true)),
                                            ("Hide", Some(false)),
                                        ]
                                        .into_iter()
                                        .fold(
                                            menu,
                                            |menu, (label, value)| {
                                                let owner = owner.clone();
                                                let remote = remote.clone();
                                                menu.item(
                                                    PopupMenuItem::new(label)
                                                        .checked(selected == value)
                                                        .on_click(move |_, window, cx| {
                                                            let _ = owner.update(cx, |page, cx| {
                                                                if let Shape::Remotes {
                                                                    rules,
                                                                    names,
                                                                    ..
                                                                } = &mut page
                                                                    .structured
                                                                    .forms
                                                                    .get_mut(&index)
                                                                    .unwrap()
                                                                    .shape
                                                                {
                                                                    names.insert(remote.clone());
                                                                    if let Some(value) = value {
                                                                        rules.insert(
                                                                            remote.clone(),
                                                                            value.into(),
                                                                        );
                                                                    } else {
                                                                        rules.remove(&remote);
                                                                    }
                                                                }
                                                                page.structured_changed(
                                                                    index, window, cx,
                                                                );
                                                            });
                                                        }),
                                                )
                                            },
                                        )
                                    }),
                            ),
                    );
                }
                body = body.child(
                    div()
                        .h_flex()
                        .flex_wrap()
                        .gap(crate::spacing::space(2., cx))
                        .child(
                            div()
                                .w(px(260.))
                                .debug_selector(|| "remote-new-name".into())
                                .child(Input::new(&name.input).small().disabled(disabled)),
                        )
                        .child(self.structured_button(
                            index,
                            "Add remote",
                            Action::Add,
                            disabled,
                            cx,
                        )),
                );
                return body.into_any_element();
            }
        };
        for (at, field) in fields.iter().enumerate() {
            let id = field.id;
            let owner = cx.entity_id();
            let label = field.input.read(cx).value().clone();
            let drag = RowDrag {
                owner,
                setting: index,
                field: id,
                label,
            };

            body = body.child(
                div()
                    .id(("structured-row", id))
                    .debug_selector(move || {
                        format!("structured-row-{}-{at}", catalogue::SETTINGS[index].key)
                    })
                    .rounded(gpui::component::Theme::global(cx).radius)
                    .when(!disabled, |row| {
                        row.drag_over::<RowDrag>(move |row, drag, _, cx| {
                            if drag.owner == owner && drag.setting == index {
                                row.bg(gpui::component::Theme::global(cx).primary.opacity(0.12))
                            } else {
                                row
                            }
                        })
                        .on_drop(cx.listener(
                            move |page, drag: &RowDrag, window, cx| {
                                page.structured_reorder(index, drag, id, window, cx)
                            },
                        ))
                    })
                    .h_flex()
                    .flex_wrap()
                    .gap(crate::spacing::space(2., cx))
                    .child(
                        div()
                            .id(("structured-drag", id))
                            .when(!disabled, |handle| {
                                handle.on_drag(drag, |drag, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.new(|_| drag.clone())
                                })
                            })
                            .child(
                                Button::new(("structured-handle", id))
                                    .small()
                                    .compact()
                                    .ghost()
                                    .icon(gpui::assets::IconName::GripVertical)
                                    .cursor_grab()
                                    .track_focus(&field.handle)
                                    .on_click({
                                        let focus = field.handle.clone();
                                        move |_, window, cx| {
                                            window.focus(&focus, cx);
                                            cx.stop_propagation();
                                        }
                                    })
                                    .accessibility_label("Reorder row")
                                    .tooltip(if cfg!(target_os = "macos") {
                                        "Drag to reorder; Option+↑/↓ moves the focused row"
                                    } else {
                                        "Drag to reorder; Alt+↑/↓ moves the focused row"
                                    })
                                    .debug_selector(move || {
                                        format!(
                                            "structured-handle-{}-{at}",
                                            catalogue::SETTINGS[index].key
                                        )
                                    })
                                    .disabled(disabled)
                                    .when(!disabled, |button| {
                                        button.on_key_down(cx.listener(
                                            move |page, event: &KeyDownEvent, window, cx| {
                                                if event.keystroke.modifiers.alt
                                                    && matches!(
                                                        event.keystroke.key.as_str(),
                                                        "up" | "down"
                                                    )
                                                {
                                                    page.structured_action(
                                                        index,
                                                        Action::Move(
                                                            id,
                                                            event.keystroke.key == "up",
                                                        ),
                                                        window,
                                                        cx,
                                                    );
                                                    cx.stop_propagation();
                                                }
                                            },
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(360.))
                            .max_w_full()
                            .debug_selector(move || {
                                format!("structured-field-{}-{at}", catalogue::SETTINGS[index].key)
                            })
                            .child(Input::new(&field.input).small().disabled(disabled)),
                    )
                    .when(matches!(form.shape, Shape::Folders(_)), |row| {
                        row.child(
                            Button::new(("browse-folder", id))
                                .small()
                                .icon(gpui::assets::IconName::FolderOpen)
                                .compact()
                                .ghost()
                                .accessibility_label("Choose profile folder")
                                .tooltip("Choose profile folder")
                                .debug_selector(move || {
                                    format!(
                                        "structured-browse-{}-{at}",
                                        catalogue::SETTINGS[index].key
                                    )
                                })
                                .disabled(disabled)
                                .on_click(cx.listener(move |page, _, window, cx| {
                                    page.structured_browse(index, id, true, window, cx)
                                })),
                        )
                    })
                    .child(self.structured_button(
                        index,
                        "Remove",
                        Action::Remove(id),
                        disabled,
                        cx,
                    )),
            );
        }
        if matches!(form.shape, Shape::Folders(_)) {
            body = body.child(div().h_flex().child(self.structured_button(
                index,
                "Add path",
                Action::Add,
                disabled,
                cx,
            )));
            body = body.child(
                div()
                    .text_sm()
                    .text_color(gpui::component::Theme::global(cx).muted_foreground)
                    .child("Relative paths start beside the config file. Order is preserved."),
            );
        } else if matches!(
            form.shape,
            Shape::Editor {
                mode: Mode::Command,
                ..
            }
        ) {
            body = body.child(div().h_flex().child(self.structured_button(
                index,
                "Add argument",
                Action::Add,
                disabled,
                cx,
            )));
        }
        body.into_any_element()
    }
}
