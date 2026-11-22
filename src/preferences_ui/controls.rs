//! Native controls for structured settings; persistence stays in the shared page.
use super::*;
use gpui::component::{
    IndexPath,
    select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState},
};
/// Measure the closed control only; menus keep their own usable width.
fn label_width(label: &str, window: &Window, cx: &App) -> Pixels {
    let run = TextRun {
        len: label.len(),
        font: Font {
            family: gpui::component::Theme::global(cx).font_family.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    window
        .text_system()
        .shape_line(
            label.to_owned().into(),
            window.rem_size() * 0.875,
            &[run],
            None,
        )
        .width
}

pub(super) fn dropdown_width(label: &str, window: &Window, cx: &App) -> Pixels {
    (label_width(label, window, cx) + crate::spacing::space(10., cx))
        .max(px(72.))
        .min(px(240.))
}

impl SettingsPage {
    pub(super) fn number_width(
        &self,
        index: usize,
        text: &str,
        window: &Window,
        cx: &App,
    ) -> Pixels {
        let setting = &catalogue::SETTINGS[index];
        let step = setting.step.unwrap_or(1.).to_string();
        let decimals = step
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len());
        let mut width = label_width(text, window, cx).max(label_width("Auto", window, cx));
        for bound in [&setting.minimum, &setting.maximum].into_iter().flatten() {
            let value = match bound {
                catalogue::NumberBound::Value(value) => Some(*value),
                catalogue::NumberBound::Setting(key) => self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.value(key))
                    .and_then(|value| {
                        value
                            .as_float()
                            .or_else(|| value.as_integer().map(|value| value as f64))
                    }),
            };
            if let Some(value) = value {
                width = width.max(label_width(&format!("{value:.decimals$}"), window, cx));
            }
        }
        // Two native 24px step buttons, plus the text slot's padding and border.
        width + window.rem_size() * 3. + crate::spacing::space(4., cx) + px(2.)
    }
}

#[derive(Clone)]
pub(super) struct FontChoice {
    name: String,
    value: Option<String>,
}
impl SelectItem for FontChoice {
    type Value = Option<String>;
    fn title(&self) -> SharedString {
        self.name.clone().into()
    }
    fn value(&self) -> &Self::Value {
        &self.value
    }
}
pub(super) type FontPicker = SelectState<SearchableVec<FontChoice>>;
impl SettingsPage {
    fn font_choices(&self, current: Option<&str>) -> Vec<FontChoice> {
        let mut choices = vec![FontChoice {
            name: "Use default".into(),
            value: None,
        }];
        if let Some(current) = current
            && !self.font_names.iter().any(|font| font == current)
        {
            choices.push(FontChoice {
                name: current.into(),
                value: Some(current.into()),
            });
        }
        choices.extend(self.font_names.iter().map(|font| FontChoice {
            name: font.clone(),
            value: Some(font.clone()),
        }));
        choices
    }
    pub(super) fn font_control(
        &mut self,
        index: usize,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self
            .value(index)
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        let picker = if let Some(picker) = self.font_pickers.get(&index) {
            picker.clone()
        } else {
            let choices = self.font_choices(current.as_deref());
            let selected = choices
                .iter()
                .position(|choice| choice.value == current)
                .map(IndexPath::new);
            let picker = cx.new(|cx| {
                FontPicker::new(SearchableVec::new(choices), selected, window, cx).searchable(true)
            });
            self.subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |page, _, event, window, cx| {
                    if let SelectEvent::Confirm(Some(value)) = event {
                        page.save(index, value.clone().map(toml::Value::String), window, cx);
                    }
                },
            ));
            self.font_pickers.insert(index, picker.clone());
            picker
        };
        if self.busy != Some(index) && picker.read(cx).selected_value() != Some(&current) {
            let choices = self.font_choices(current.as_deref());
            picker.update(cx, |picker, cx| {
                picker.set_items(SearchableVec::new(choices), window, cx);
                picker.set_selected_value(&current, window, cx);
            });
        }
        let width = dropdown_width(current.as_deref().unwrap_or("Use default"), window, cx)
            + crate::spacing::edge(cx);
        div()
            .w(width)
            .debug_selector(move || format!("setting-{}", catalogue::SETTINGS[index].key))
            .child(
                Select::new(&picker)
                    .small()
                    .w_full()
                    .menu_width(px(360.))
                    .menu_max_h(px(280.))
                    .search_placeholder("Find a font…")
                    .accessibility_label(catalogue::SETTINGS[index].title.clone())
                    .disabled(disabled),
            )
            .into_any_element()
    }
    pub(super) fn step_number(
        &mut self,
        index: usize,
        increment: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() || self.error.is_some() || self.defaults {
            return;
        }
        let input = self.input(index, window, cx);
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        match catalogue::SETTINGS[index].stepped(
            input.read(cx).value().as_ref(),
            increment,
            snapshot,
        ) {
            Ok(value) => {
                let text = preferences::display(Some(&value));
                input.update(cx, |input, cx| input.set_value(text, window, cx));
                let draft = self.drafts.get_mut(&index).expect("input creates a draft");
                draft.dirty = true;
                draft.error = None;
                self.save(index, Some(value), window, cx);
            }
            Err(error) => {
                self.drafts
                    .get_mut(&index)
                    .expect("input creates a draft")
                    .error = Some(error);
                cx.notify();
            }
        }
    }
}

/// Fixed light/dark seed fields already select one mode; other colors can be shared.
pub(super) fn paired_color(index: usize) -> bool {
    let setting = &catalogue::SETTINGS[index];
    (setting.kind == Kind::Color || setting.key == "ui.accent_color")
        && !setting.key.ends_with(".light")
        && !setting.key.ends_with(".dark")
}

pub(super) fn color_text(index: usize, value: Option<&toml::Value>, dark: bool) -> String {
    let value = if paired_color(index) {
        value.and_then(|value| {
            if value.is_table() {
                value.get(if dark { "dark" } else { "light" })
            } else {
                Some(value)
            }
        })
    } else {
        value
    };
    input_text(value)
}

pub(super) fn color_edit(
    index: usize,
    current: Option<&toml::Value>,
    text: &str,
    dark: bool,
) -> Result<toml::Value, String> {
    let value = catalogue::SETTINGS[index].parse_input(text)?;
    if paired_color(index)
        && let Some(current) = current.filter(|value| value.is_table())
    {
        // Each field edits a single RGB value. Only a shared accent may use system.
        jaywalk::settings::RgbColor::try_from(text.trim().to_owned())?;
        let mut pair = current.clone();
        pair[if dark { "dark" } else { "light" }] = value;
        Ok(pair)
    } else {
        Ok(value)
    }
}

pub(super) fn separate_color(value: &toml::Value, separate: bool, dark: bool) -> toml::Value {
    let selected = if value.is_table() {
        &value[if dark { "dark" } else { "light" }]
    } else {
        value
    };
    if separate {
        toml::Value::Table(toml::Table::from_iter([
            ("light".into(), selected.clone()),
            ("dark".into(), selected.clone()),
        ]))
    } else {
        selected.clone()
    }
}

#[cfg(test)]
mod color_tests {
    use super::{color_edit, color_text, paired_color, separate_color};
    use jaywalk::preferences::catalogue;
    fn index(key: &str) -> usize {
        catalogue::SETTINGS
            .iter()
            .position(|setting| setting.key == key)
            .unwrap()
    }
    #[test]
    fn color_pairs_edit_only_the_selected_appearance_and_can_be_unified() {
        let index = index("diff.colors.added_background");
        let pair: toml::Value =
            toml::from_str::<toml::Table>("value = { light = '#112233', dark = '#445566' }")
                .unwrap()
                .remove("value")
                .unwrap();
        assert_eq!(color_text(index, Some(&pair), false), "#112233");
        assert_eq!(color_text(index, Some(&pair), true), "#445566");
        let edited = color_edit(index, Some(&pair), "#abcdef", true).unwrap();
        assert_eq!(edited["light"].as_str(), Some("#112233"));
        assert_eq!(edited["dark"].as_str(), Some("#abcdef"));
        assert!(color_edit(index, Some(&pair), "system", true).is_err());
        let shared = separate_color(&edited, false, true);
        assert_eq!(shared.as_str(), Some("#abcdef"));
        let separated = separate_color(&shared, true, false);
        assert_eq!(separated["light"], shared);
        assert_eq!(separated["dark"], shared);
        assert_eq!(
            color_edit(index, Some(&shared), "#123456", false)
                .unwrap()
                .as_str(),
            Some("#123456")
        );
    }
    #[test]
    fn fixed_seed_fields_stay_scalar_and_accent_supports_shared_system() {
        assert!(!paired_color(index("theme.shell_auto.seed.light")));
        assert!(!paired_color(index("theme.shell_auto.seed.dark")));
        let index = index("ui.accent_color");
        assert!(paired_color(index));
        assert_eq!(
            color_edit(index, Some(&"system".into()), "system", false)
                .unwrap()
                .as_str(),
            Some("system")
        );
    }
}
