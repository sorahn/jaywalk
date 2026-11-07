use super::*;

impl Viewer {
    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .v_flex()
            .size_full()
            .bg(rgb(0x253448))
            .child(div().p_3().text_sm().child("CHANGED FILES / debug blue"));

        match self.comparison_state() {
            LoadState::Loading => panel = panel.child(div().p_3().child("Loading comparison…")),
            LoadState::Failed(error) => panel = panel.child(div().p_3().child(error.clone())),
            LoadState::Loaded(comparison) => {
                let mut list = div()
                    .id("changed-files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll();
                for (index, file) in comparison.files.iter().enumerate() {
                    let background = if self.selected_index() == Some(index) {
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
}
