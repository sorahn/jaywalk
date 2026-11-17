//! Regression for fractional trackpad input being mistaken for an edge bounce.
//! Adapted from GPUI Base's Apache-2.0 scroll_bounce regression.
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, ScrollDelta,
    ScrollEvent, ScrollHandle, StatefulInteractiveElement as _, Styled as _, TestAppContext,
    TouchPhase, VisualTestContext, Window, div, point, px,
};
use gpui_kit::base::ScrollBounce;
use gpui_test as gpui;
use std::time::Duration;

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn scroll(cx: &mut VisualTestContext, delta: f32, phase: TouchPhase) {
    cx.simulate_event(ScrollEvent {
        position: point(px(100.), px(100.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
        touch_phase: phase,
        ..Default::default()
    });
}

struct WheelViewport(ScrollHandle, Pixels);

impl Render for WheelViewport {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        ScrollBounce::new(
            "wheel-bounce",
            &self.0,
            div()
                .id("wheel-viewport")
                .w(px(200.))
                .h(px(200.))
                .overflow_y_scroll()
                .track_scroll(&self.0)
                .child(div().h(self.1).w_full()),
        )
        .enabled(true)
        .scroll_wheel(true)
    }
}

#[gpui::test]
fn fractional_wheel_input_keeps_scrolling_deep_in_a_long_list(cx: &mut TestAppContext) {
    for delta in [-0.09_f32, 0.09] {
        let mut app = cx.new_app();
        let handle = ScrollHandle::new();
        let (_, cx) = app.add_window_view({
            let handle = handle.clone();
            move |_, _| WheelViewport(handle, px(2_000_000.))
        });
        draw(cx);
        handle.set_offset(point(px(0.), px(-1_000_000.)));
        draw(cx);
        let origin = handle.bounds().origin;
        scroll(cx, delta, TouchPhase::Started);
        scroll(cx, delta.signum() * 20., TouchPhase::Moved);
        scroll(cx, 0., TouchPhase::Ended);
        cx.executor().advance_clock(Duration::from_millis(16));
        draw(cx);
        scroll(cx, delta, TouchPhase::Moved);
        scroll(cx, 0., TouchPhase::Started);
        scroll(cx, delta.signum() * 20., TouchPhase::Moved);
        draw(cx);
        // Here each 0.09px delta rounds to 0.0625px in the stored offset.
        let expected = -1_000_000. + delta.signum() * 40.125;
        assert!((handle.offset().y.as_f32() - expected).abs() < 0.001);
        assert_eq!(handle.bounds().origin, origin);
    }
}
