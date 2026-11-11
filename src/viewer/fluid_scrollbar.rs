//! The connector gutter doubles as a shared virtual-comparison scrollbar.
use super::*;
use jaywalk::fluid::SharedScrollTrack;

pub(super) fn paint_shared_scrollbar(
    viewer: WeakEntity<Viewer>,
    clip: Bounds<Pixels>,
    geometry: &DiffGeometry,
    model: &FileDiff,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(entity) = viewer.upgrade() else {
        return;
    };
    let state = entity.read(cx);
    let viewport = f32::from(
        state
            .code()
            .expect("loaded fluid comparison")
            .before
            .read(cx)
            .input_bounds()
            .size
            .height,
    );
    let extent = geometry.scroll_extent(model);
    let track = SharedScrollTrack::new(
        extent,
        viewport,
        f32::from(clip.size.height),
        state.settings.panels.diff.shared_scrollbar_height,
        state.fluid.position,
    );
    if !track.scrollable() {
        return;
    }
    let hovered = clip.contains(&window.mouse_position());
    let active = state.fluid_drag.is_some();
    if active {
        window.set_window_cursor_style(CursorStyle::ResizeUpDown);
    }
    let opacity = state.settings.panels.diff.shared_scrollbar_opacity;
    let color = gpui::component::Theme::global(cx)
        .foreground
        .opacity((opacity * if hovered || active { 2. } else { 1. }).min(1.));
    // This is painted before row tints and correspondence paths. The whole
    // gutter is the hit target even where a connector crosses the faint thumb.
    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
        window.paint_quad(fill(
            Bounds::new(
                point(clip.left(), clip.top() + px(track.thumb.start)),
                size(clip.size.width, px(track.thumb.end - track.thumb.start)),
            ),
            color,
        ));
    });
    let down_viewer = viewer.clone();
    let thumb = track.thumb.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture
            || event.button != MouseButton::Left
            || !clip.contains(&event.position)
        {
            return;
        }
        let y = f32::from(event.position.y - clip.top());
        let grab = if thumb.contains(&y) {
            (y - thumb.start) / (thumb.end - thumb.start)
        } else {
            0.5
        };
        let _ = down_viewer.update(cx, |viewer, cx| {
            viewer.fluid_drag = Some(grab);
            viewer.navigation_target = None;
            if !thumb.contains(&y) {
                viewer.scroll_comparison(track.position_at(y, grab) - viewer.fluid.position, cx);
            }
            cx.notify();
        });
        cx.stop_propagation();
    });
    let move_viewer = viewer.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let _ = move_viewer.update(cx, |viewer, cx| {
            if clip.contains(&event.position) != hovered {
                cx.notify();
            }
            if event.pressed_button != Some(MouseButton::Left) {
                if viewer.fluid_drag.take().is_some() {
                    cx.notify();
                }
                return;
            }
            let Some(grab) = viewer.fluid_drag else {
                return;
            };
            let track = SharedScrollTrack::new(
                extent,
                viewport,
                f32::from(clip.size.height),
                viewer.settings.panels.diff.shared_scrollbar_height,
                viewer.fluid.position,
            );
            let target = track.position_at(f32::from(event.position.y - clip.top()), grab);
            viewer.scroll_comparison(target - viewer.fluid.position, cx);
            cx.stop_propagation();
        });
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
            return;
        }
        let _ = viewer.update(cx, |viewer, cx| {
            if viewer.fluid_drag.take().is_some() {
                cx.notify();
                cx.stop_propagation();
            }
        });
    });
}
