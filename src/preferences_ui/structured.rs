//! Focused editors for three existing values; aggregate drafts reuse the normal
//! conflict checks and Save/Discard path. No executable is launched here.
mod render;
use super::*;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Default)]
pub(super) struct Editors {
    forms: HashMap<usize, Form>,
    next_id: usize,
    pub(super) prompt_serial: u64,
    repository: Option<PathBuf>,
    remote_serial: u64,
    known_remotes: Vec<String>,
    remote_error: Option<String>,
    remote_loading: bool,
}
#[derive(Clone)]
struct RowDrag {
    owner: EntityId,
    setting: usize,
    field: usize,
    label: SharedString,
}
impl Render for RowDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .gap(crate::spacing::space(2., cx))
            .p(crate::spacing::space(2., cx))
            .bg(gpui::component::Theme::global(cx).popover)
            .rounded(gpui::component::Theme::global(cx).radius)
            .border_1()
            .border_color(gpui::component::Theme::global(cx).border)
            .child(gpui::component::Icon::new(gpui::assets::IconName::GripVertical).small())
            .child(div().max_w(px(360.)).truncate().child(self.label.clone()))
    }
}
struct Field {
    id: usize,
    handle: FocusHandle,
    input: Entity<InputState>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    System,
    Application,
    Command,
}
impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::System => "System default",
            Self::Application => "Application",
            Self::Command => "Command",
        }
    }
}
enum Shape {
    Editor {
        mode: Mode,
        program: Field,
        arguments: Vec<Field>,
    },
    Folders(Vec<Field>),
    Remotes {
        rules: toml::Table,
        names: BTreeSet<String>,
        name: Field,
    },
}
struct Form {
    synced: Option<toml::Value>,
    expanded: bool,
    shape: Shape,
}
#[derive(Clone, Copy)]
enum Action {
    Mode(Mode),
    Add,
    Remove(usize),
    Move(usize, bool),
}
impl Shape {
    fn fields(&self) -> Vec<&Field> {
        match self {
            Self::Editor {
                program, arguments, ..
            } => std::iter::once(program).chain(arguments.iter()).collect(),
            Self::Folders(fields) => fields.iter().collect(),
            Self::Remotes { name, .. } => vec![name],
        }
    }
    fn list_mut(&mut self) -> Option<&mut Vec<Field>> {
        match self {
            Self::Editor { arguments, .. } => Some(arguments),
            Self::Folders(fields) => Some(fields),
            _ => None,
        }
    }
    fn value(&self, cx: &App) -> Option<toml::Value> {
        let text = |field: &Field| toml::Value::String(field.input.read(cx).value().to_string());
        match self {
            Self::Editor {
                mode: Mode::System, ..
            } => None,
            Self::Editor {
                mode: Mode::Application,
                program,
                ..
            } => Some(text(program)),
            Self::Editor {
                program, arguments, ..
            } => Some(toml::Value::Array(
                std::iter::once(program)
                    .chain(arguments.iter())
                    .map(text)
                    .collect(),
            )),
            Self::Folders(fields) => Some(toml::Value::Array(fields.iter().map(text).collect())),
            Self::Remotes { rules, .. } => {
                (!rules.is_empty()).then(|| toml::Value::Table(rules.clone()))
            }
        }
    }
}
impl Editors {
    pub(super) fn inputs(&self) -> Vec<&Entity<InputState>> {
        self.forms
            .values()
            .flat_map(|form| form.shape.fields())
            .map(|field| &field.input)
            .collect()
    }
}
impl SettingsPage {
    pub(crate) fn set_repository_context(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.structured.repository == root {
            return;
        }
        self.structured.repository = root.clone();
        self.structured.remote_serial += 1;
        self.structured.known_remotes.clear();
        self.structured.remote_error = None;
        self.structured.remote_loading = root.is_some();
        let serial = self.structured.remote_serial;
        if let Some(root) = root {
            cx.spawn(async move |page, cx| {
                let result = cx
                    .background_spawn(
                        async move { jaywalk::references::configured_remote_names(&root) },
                    )
                    .await;
                let _ =
                    page.update(cx, |page, cx| {
                        if page.structured.remote_serial != serial {
                            return;
                        }
                        page.structured.remote_loading = false;
                        match result {
                            Ok(names) => page.structured.known_remotes = names,
                            Err(_) => page.structured.remote_error = Some(
                                "Could not read repository remotes. Saved names remain available."
                                    .into(),
                            ),
                        }
                        cx.notify();
                    });
            })
            .detach();
        }
        cx.notify();
    }
    fn structured_field(
        &mut self,
        index: usize,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        let id = self.structured.next_id;
        self.structured.next_id += 1;
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(
                match catalogue::SETTINGS[index].key.as_str() {
                    "settings.editor" => "Application, executable or argument",
                    "keyboard.additional_directories" => "Profile folder",
                    _ => "Remote name",
                },
            );
            input.set_value(value, window, cx);
            input
        });
        self.subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |page, _, event, window, cx| {
                let Some(form) = page.structured.forms.get(&index) else {
                    return;
                };
                if !form.shape.fields().iter().any(|field| field.id == id) {
                    return;
                }
                if matches!(&form.shape, Shape::Remotes { .. }) {
                    return;
                }
                if matches!(
                    event,
                    InputEvent::Change | InputEvent::PressEnter { .. } | InputEvent::Blur
                ) {
                    page.structured_changed(index, window, cx);
                }
            },
        ));
        Field {
            id,
            input,
            handle: cx.focus_handle(),
        }
    }
    fn ensure_structured(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.input(index, window, cx);
        let value = self.value(index).cloned();
        let dirty = self.drafts.get(&index).is_some_and(|draft| draft.dirty);
        if let Some(form) = self.structured.forms.get(&index)
            && (dirty || form.synced == value)
        {
            return;
        }
        let expanded = self
            .structured
            .forms
            .get(&index)
            .is_some_and(|form| form.expanded);
        let shape = match catalogue::SETTINGS[index].key.as_str() {
            "settings.editor" => {
                let (mode, program, args) = match &value {
                    Some(toml::Value::String(program)) => {
                        (Mode::Application, program.clone(), vec![])
                    }
                    Some(toml::Value::Array(parts)) => (
                        Mode::Command,
                        parts
                            .first()
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        parts
                            .iter()
                            .skip(1)
                            .filter_map(toml::Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                    ),
                    _ => (Mode::System, String::new(), vec![]),
                };
                let program = self.structured_field(index, &program, window, cx);
                let arguments = args
                    .iter()
                    .map(|arg| self.structured_field(index, arg, window, cx))
                    .collect();
                Shape::Editor {
                    mode,
                    program,
                    arguments,
                }
            }
            "keyboard.additional_directories" => Shape::Folders(
                value
                    .as_ref()
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
                    .iter()
                    .map(|path| self.structured_field(index, path, window, cx))
                    .collect(),
            ),
            "history.remotes.overrides" => {
                let rules = value
                    .as_ref()
                    .and_then(toml::Value::as_table)
                    .cloned()
                    .unwrap_or_default();
                let names = rules.keys().cloned().collect();
                let name = self.structured_field(index, "", window, cx);
                Shape::Remotes { rules, names, name }
            }
            _ => unreachable!("only three catalogue values use structured controls"),
        };
        let expanded = expanded
            || matches!(
                &shape,
                Shape::Editor {
                    mode: Mode::Application | Mode::Command,
                    ..
                }
            );
        self.structured.forms.insert(
            index,
            Form {
                synced: value,
                expanded,
                shape,
            },
        );
    }
    fn structured_changed(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.structured.forms[&index].shape.value(cx);
        if self.structured.forms[&index].synced == value {
            self.rebuild_rows(cx);
            cx.notify();
            return;
        }
        self.structured.prompt_serial += 1;
        self.structured.forms.get_mut(&index).unwrap().synced = value.clone();
        let text = value.as_ref().map(preferences::literal).unwrap_or_default();
        let draft = self.drafts.get_mut(&index).expect("aggregate draft exists");
        draft
            .input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        draft.dirty = true;
        draft.error = None;
        self.stage(index, value, window, cx);
        if self.drafts.get(&index).is_some_and(|draft| draft.dirty) {
            self.rebuild_rows(cx);
        }
        cx.notify();
    }
    fn structured_reorder(
        &mut self,
        index: usize,
        drag: &RowDrag,
        target: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.owner != cx.entity_id()
            || drag.setting != index
            || self.busy.is_some()
            || self.defaults
            || self.error.is_some()
        {
            return;
        }
        let Some(form) = self.structured.forms.get_mut(&index) else {
            return;
        };
        let Some(fields) = form.shape.list_mut() else {
            return;
        };
        let (Some(from), Some(to)) = (
            fields.iter().position(|field| field.id == drag.field),
            fields.iter().position(|field| field.id == target),
        ) else {
            return;
        };
        if from == to {
            return;
        }
        let field = fields.remove(from);
        fields.insert(to, field);
        self.structured_changed(index, window, cx);
    }
    fn structured_action(
        &mut self,
        index: usize,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() || self.defaults || self.error.is_some() {
            return;
        }
        let mut form = self.structured.forms.remove(&index).unwrap();
        match action {
            Action::Mode(next) => {
                if let Shape::Editor { mode, .. } = &mut form.shape {
                    *mode = next;
                    form.expanded = true;
                }
            }
            Action::Add => match &mut form.shape {
                Shape::Remotes { names, name, .. } => {
                    let text = name.input.read(cx).value().trim().to_owned();
                    if text.is_empty() || text.chars().any(char::is_control) {
                        self.message =
                            Some("Enter a remote name without control characters.".into());
                        self.structured.forms.insert(index, form);
                        cx.notify();
                        return;
                    }
                    names.insert(text);
                    name.input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                shape => {
                    if let Some(fields) = shape.list_mut() {
                        fields.push(self.structured_field(index, "", window, cx));
                    }
                }
            },
            Action::Remove(id) => {
                if let Some(fields) = form.shape.list_mut() {
                    fields.retain(|field| field.id != id);
                }
            }
            Action::Move(id, up) => {
                if let Some(fields) = form.shape.list_mut()
                    && let Some(at) = fields.iter().position(|field| field.id == id)
                {
                    let next = if up {
                        at.checked_sub(1)
                    } else {
                        (at + 1 < fields.len()).then_some(at + 1)
                    };
                    if let Some(next) = next {
                        fields.swap(at, next);
                    }
                }
            }
        }
        self.structured.forms.insert(index, form);
        self.structured_changed(index, window, cx);
    }
    fn structured_browse(
        &mut self,
        index: usize,
        field: usize,
        directories: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.structured.prompt_serial += 1;
        let serial = self.structured.prompt_serial;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some(
                if directories {
                    "Choose profile folders"
                } else {
                    "Choose editor"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |page, cx| {
            let result = paths.await;
            let _ = page.update_in(cx, |page, window, cx| {
                if page.structured.prompt_serial != serial {
                    return;
                }
                let paths = match result {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => return,
                    _ => {
                        page.message = Some("Could not open the path chooser.".into());
                        cx.notify();
                        return;
                    }
                };
                let paths: Result<Vec<_>, _> = paths
                    .iter()
                    .map(|path| {
                        path.to_str()
                            .map(str::to_owned)
                            .ok_or("Choose a path representable as text.")
                    })
                    .collect();
                let paths = match paths {
                    Ok(paths) => paths,
                    Err(error) => {
                        page.message = Some(error.into());
                        cx.notify();
                        return;
                    }
                };
                let form = page.structured.forms.remove(&index).unwrap();
                if let (Some(input), Some(path)) = (
                    form.shape
                        .fields()
                        .into_iter()
                        .find(|entry| entry.id == field),
                    paths.first(),
                ) {
                    input
                        .input
                        .update(cx, |input, cx| input.set_value(path.clone(), window, cx));
                }
                page.structured.forms.insert(index, form);
                page.structured_changed(index, window, cx);
            });
        })
        .detach();
    }
}

#[cfg(test)]
impl SettingsPage {
    pub(crate) fn test_structured_fields(&self, key: &str) -> Vec<(usize, Entity<InputState>)> {
        let index = catalogue::SETTINGS
            .iter()
            .position(|setting| setting.key == key)
            .unwrap();
        self.structured.forms[&index]
            .shape
            .fields()
            .iter()
            .map(|field| (field.id, field.input.clone()))
            .collect()
    }
}
