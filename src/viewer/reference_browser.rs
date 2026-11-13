//! Fixed-height virtual rows: clipping a Div alone still lays out every reference.
use super::*;
use gpui::component::{
    input::{Input, InputEvent, InputState},
    popover::PopoverState,
};
use gpui::prelude::FluentBuilder;
use jaywalk::{
    history::{Reference, RemoteVisibility},
    references::ReferenceSnapshot,
};
use std::sync::Arc;
actions!(reference_browser, [Cancel, Confirm, SelectDown, SelectUp]);

pub(super) fn init(cx: &mut App) {
    let context = Some("ReferenceBrowser");
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, context),
        KeyBinding::new("down", SelectDown, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Cancel, context),
    ]);
}

#[cfg(test)]
thread_local! { pub(super) static RENDERED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

enum Row {
    Heading(String),
    Reference(usize),
}

pub(super) struct ReferenceBrowser {
    snapshot: Arc<ReferenceSnapshot>,
    target: Option<String>,
    visibility: RemoteVisibility,
    inspected: Option<Reference>,
    labels: Vec<String>,
    rows: Vec<Row>,
    selected: Option<usize>,
    input: Entity<InputState>,
    scroll: UniformListScrollHandle,
    owner: WeakEntity<Viewer>,
    popup: WeakEntity<PopoverState>,
    _subscription: Subscription,
    #[cfg(test)]
    pub(super) rendered_rows: usize,
}
impl ReferenceBrowser {
    pub(super) fn new(
        owner: WeakEntity<Viewer>,
        popup: WeakEntity<PopoverState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search branches and bookmarks…"));
        let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.filter(cx);
                cx.notify();
            }
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        Self {
            snapshot: Arc::default(),
            target: None,
            visibility: RemoteVisibility::default(),
            inspected: None,
            labels: vec![],
            rows: vec![],
            selected: None,
            input,
            scroll: UniformListScrollHandle::new(),
            owner,
            popup,
            _subscription: subscription,
            #[cfg(test)]
            rendered_rows: 0,
        }
    }
    pub(super) fn configure(
        &mut self,
        snapshot: Arc<ReferenceSnapshot>,
        target: Option<&str>,
        visibility: &RemoteVisibility,
        inspected: Option<&Reference>,
        popup: WeakEntity<PopoverState>,
        cx: &mut Context<Self>,
    ) {
        self.popup = popup;
        if Arc::ptr_eq(&snapshot, &self.snapshot)
            && target == self.target.as_deref()
            && visibility == &self.visibility
            && inspected == self.inspected.as_ref()
        {
            return;
        }
        self.snapshot = snapshot;
        self.target = target.map(str::to_owned);
        self.visibility = visibility.clone();
        self.inspected = inspected.cloned();
        self.labels = self
            .snapshot
            .references
            .iter()
            .map(|r| r.display_name().to_lowercase())
            .collect();
        self.filter(cx);
    }
    fn filter(&mut self, cx: &App) {
        let query = self.input.read(cx).value().to_lowercase();
        let mut group = None;
        self.rows.clear();
        for (index, reference) in self.snapshot.references.iter().enumerate() {
            if self
                .target
                .as_ref()
                .is_some_and(|target| target != &reference.target)
                || !self.labels[index].contains(&query)
            {
                continue;
            }
            let name = reference.remote().unwrap_or("Local");
            if group != Some(name) {
                group = Some(name);
                self.rows.push(Row::Heading(format!(
                    "{name}{}",
                    if reference
                        .remote()
                        .is_some_and(|remote| !self.visibility.includes(remote))
                    {
                        " · hidden from graph"
                    } else {
                        ""
                    }
                )));
            }
            self.rows.push(Row::Reference(index));
        }
        self.selected = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Reference(_)));
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
    fn step(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(current) = self.selected else {
            return;
        };
        let next = if delta > 0 {
            ((current + 1)..self.rows.len()).find(|&i| matches!(self.rows[i], Row::Reference(_)))
        } else {
            (0..current)
                .rev()
                .find(|&i| matches!(self.rows[i], Row::Reference(_)))
        };
        if let Some(next) = next {
            self.selected = Some(next);
            self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
            cx.notify();
        }
    }
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Row::Reference(index)) = self.selected.and_then(|i| self.rows.get(i)) else {
            return;
        };
        let reference = self.snapshot.references[*index].clone();
        let _ = self.popup.update(cx, |popup, cx| popup.dismiss(window, cx));
        let _ = self.owner.update(cx, |viewer, cx| {
            viewer.references.inspected = Some(reference.clone());
            viewer.open_reference_target(reference.target, true, window, cx);
        });
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input.read(cx).value().is_empty() {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.filter(cx);
            cx.notify();
        } else {
            let _ = self.popup.update(cx, |popup, cx| popup.dismiss(window, cx));
        }
    }
}
impl Render for ReferenceBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = uniform_list(
            "reference-rows",
            self.rows.len(),
            cx.processor(|this, visible: std::ops::Range<usize>, _, cx| {
                #[cfg(test)]
                {
                    this.rendered_rows += visible.len();
                    RENDERED.with(|count| count.set(count.get() + visible.len()));
                }
                visible
                    .map(|index| match &this.rows[index] {
                        Row::Heading(label) => div()
                            .h(px(30.))
                            .flex_shrink_0()
                            .items_center()
                            .h_flex()
                            .text_size(px(12.))
                            .child(label.clone())
                            .into_any_element(),
                        Row::Reference(reference) => {
                            let row = super::references::reference_row(
                                &this.snapshot.references[*reference],
                                this.inspected.as_ref(),
                                this.owner.clone(),
                                this.popup.clone(),
                            );
                            let selected = this.selected == Some(index);
                            div()
                                .h(px(30.))
                                .flex_shrink_0()
                                .min_w_0()
                                .overflow_hidden()
                                .when(selected, |row| {
                                    row.bg(gpui::component::Theme::global(cx).list_active)
                                })
                                .child(row)
                                .into_any_element()
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .h(px((self.rows.len().clamp(1, 14) * 30) as f32))
        .w_full();
        div()
            .id("reference-browser")
            .debug_selector(|| "reference-browser".to_owned())
            .key_context("ReferenceBrowser")
            .v_flex()
            .gap_1()
            .p_2()
            .w(px(400.))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel(window, cx)))
            .child(Input::new(&self.input))
            .child(if self.rows.is_empty() {
                div()
                    .p_2()
                    .child("No matching references")
                    .into_any_element()
            } else {
                rows.into_any_element()
            })
    }
}
