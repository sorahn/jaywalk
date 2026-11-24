//! Retained RGB pickers edit the same draft and mode as the adjacent hex field.
use super::*;
use gpui::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};

fn rgb_hex(color: Hsla) -> String {
    let color = Rgba::from(color);
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

pub(super) struct Picker {
    state: Entity<ColorPickerState>,
    dark: bool,
}
impl SettingsPage {
    pub(super) fn color_picker(
        &mut self,
        index: usize,
        swatch: Option<Hsla>,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dark = self
            .drafts
            .get(&index)
            .expect("hex input exists")
            .color_dark;
        let color = swatch
            .or_else(|| {
                controls::color_text(index, self.value(index), dark)
                    .try_into()
                    .ok()
                    .map(|color: jaywalk::settings::RgbColor| Hsla::from(rgb(color.value())))
            })
            .unwrap_or_else(|| gpui::component::Theme::global(cx).accent);
        if let Some(picker) = self.color_pickers.get_mut(&index) {
            let mode_changed = picker.dark != dark;
            picker.dark = dark;
            if mode_changed
                || picker
                    .state
                    .read(cx)
                    .value()
                    .is_none_or(|value| rgb_hex(value) != rgb_hex(color))
            {
                picker.state.update(cx, |picker, cx| {
                    if mode_changed || disabled {
                        picker.set_open(false, cx);
                    }
                    picker.set_value(color, window, cx);
                });
            }
        } else {
            let state = cx.new(|cx| {
                ColorPickerState::new(window, cx)
                    .opaque(true)
                    .default_value(color)
            });
            self.subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |page, _, event: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(Some(color)) = event else {
                        return;
                    };
                    let dark = page
                        .color_pickers
                        .get(&index)
                        .expect("picker retained")
                        .dark;
                    match controls::color_edit(index, page.value(index), &rgb_hex(*color), dark) {
                        Ok(value) => page.stage(index, Some(value), window, cx),
                        Err(error) => {
                            page.message = Some(error);
                            cx.notify();
                        }
                    }
                },
            ));
            self.color_pickers.insert(index, Picker { state, dark });
        }
        let state = self
            .color_pickers
            .get(&index)
            .expect("picker created")
            .state
            .clone();
        div()
            .debug_selector(move || format!("setting-swatch-{}", catalogue::SETTINGS[index].key))
            .flex_shrink_0()
            .when(disabled, |this| {
                this.child(
                    div()
                        .size(crate::spacing::space(5., cx))
                        .bg(color)
                        .rounded(crate::spacing::interactive(cx)),
                )
            })
            .when(!disabled, |this| {
                this.child(
                    ColorPicker::new(&state)
                        .xsmall()
                        .accessibility_label(format!(
                            "Choose {}",
                            catalogue::SETTINGS[index].title
                        )),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
impl SettingsPage {
    pub(crate) fn test_color_picker(&self, key: &str) -> Entity<ColorPickerState> {
        let index = catalogue::SETTINGS
            .iter()
            .position(|setting| setting.key == key)
            .unwrap();
        self.color_pickers[&index].state.clone()
    }
}
