use super::*;

impl Viewer {
    pub(super) fn render_fluid_gutter(
        &self,
        gutter_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let CodeComparison {
            before,
            after,
            model,
            ..
        } = self
            .code()
            .expect("Fluid canvas is created only for a loaded file");
        let before = before.clone();
        let after = after.clone();
        let model = model.clone();
        let viewer = cx.entity().downgrade();
        let navigation_target = self.navigation_target;
        canvas(
            |_, _, _| (),
            move |overlay_bounds, _, window, cx| {
                let Some(bounds) = gutter_bounds.get() else {
                    return;
                };
                let before_state = before.read(cx);
                let after_state = after.read(cx);
                let before_bounds = before_state.input_bounds();
                let after_bounds = after_state.input_bounds();
                let line_height = before_state.line_height().unwrap_or(px(20.));
                let before_origin = before_state.text_bounds().unwrap_or(before_bounds).origin.y;
                let after_origin = after_state.text_bounds().unwrap_or(after_bounds).origin.y;
                let geometry = comparison_geometry(before_state, after_state, &model);
                let resolved_horizontal = -f32::from(
                    before_state
                        .scroll_offset()
                        .x
                        .min(after_state.scroll_offset().x),
                );
                // text_bounds already includes the editor's resolved scroll.
                // This canvas paints after both editors so both geometries are
                // from this frame, including deferred/clamped scroll updates.
                let clip = Bounds::from_corners(
                    point(bounds.left(), before_bounds.top().max(after_bounds.top())),
                    point(
                        bounds.right(),
                        before_bounds.bottom().min(after_bounds.bottom()),
                    ),
                );
                let changes: Vec<_> = model.blocks.iter().filter(|block| block.changed).collect();
                let focus = clip.center().y;
                let distance =
                    |origin: Pixels, positions: &[f32], lines: &std::ops::Range<usize>| {
                        let top = origin + px(positions[lines.start]);
                        let bottom = origin + px(positions[lines.end]);
                        (focus - focus.clamp(top, bottom)).abs()
                    };
                let active = changes
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let score = |block: &&jaywalk::diff_model::Correspondence| {
                            distance(before_origin, &geometry.before, &block.before)
                                + distance(after_origin, &geometry.after, &block.after)
                        };
                        score(a)
                            .partial_cmp(&score(b))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(index, _)| index);
                let active = navigation_target.or(active);
                let text_start = |state: &EditorState, offsets: &[usize]| {
                    let row = state
                        .display_map()
                        .display_row_to_buffer_line(state.visible_row_range()?.start);
                    let offset = *offsets.get(row)?;
                    Some(state.range_to_bounds(&(offset..offset))?.left() - state.scroll_offset().x)
                };
                let before_text = text_start(before_state, &model.before_offsets);
                let after_text = text_start(after_state, &model.after_offsets);
                // Full-row tint shares the connector geometry, including the
                // empty space after short lines and the line-number gutter.
                for (viewport, origin, before_side, text_start, positions) in [
                    (
                        Bounds::from_corners(
                            point(overlay_bounds.left(), before_bounds.top()),
                            point(bounds.left(), before_bounds.bottom()),
                        ),
                        before_origin,
                        true,
                        before_text,
                        &geometry.before,
                    ),
                    (
                        Bounds::from_corners(
                            point(bounds.right(), after_bounds.top()),
                            point(overlay_bounds.right(), after_bounds.bottom()),
                        ),
                        after_origin,
                        false,
                        after_text,
                        &geometry.after,
                    ),
                ] {
                    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
                        for (index, block) in changes.iter().enumerate() {
                            let lines = if before_side {
                                &block.before
                            } else {
                                &block.after
                            };
                            let selected = active == Some(index);
                            let rgb = if block.before.is_empty() {
                                0x4ec96e00
                            } else if block.after.is_empty() {
                                0xf8514900
                            } else {
                                0x3794ff00
                            };
                            if lines.is_empty() {
                                let y = origin + px(positions[lines.start]);
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(viewport.left(), y - px(0.5)),
                                        size(viewport.size.width, px(1.)),
                                    ),
                                    rgba(rgb | if selected { 0xc0 } else { 0x70 }),
                                ));
                                continue;
                            }
                            let color = rgba(rgb | if selected { 0x30 } else { 0x20 });
                            let top = (origin + px(positions[lines.start])).max(viewport.top());
                            let bottom = (origin + px(positions[lines.end])).min(viewport.bottom());
                            if bottom > top {
                                window.paint_quad(fill(
                                    Bounds::from_corners(
                                        point(viewport.left(), top),
                                        point(viewport.right(), bottom),
                                    ),
                                    color,
                                ));
                                if selected && let Some(text_start) = text_start {
                                    window.paint_quad(fill(
                                        Bounds::from_corners(
                                            point(viewport.left(), top),
                                            point(
                                                text_start.clamp(viewport.left(), viewport.right()),
                                                bottom,
                                            ),
                                        ),
                                        rgba(rgb | 0x20),
                                    ));
                                }
                            }
                        }
                    });
                }
                window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                    for (index, block) in changes.iter().enumerate() {
                        let selected = active == Some(index);
                        let rgb = if block.before.is_empty() {
                            0x4ec96e00
                        } else if block.after.is_empty() {
                            0xf8514900
                        } else {
                            0x3794ff00
                        };
                        let color = rgba(rgb | if selected { 0xb0 } else { 0x70 });
                        let before_top = before_origin + px(geometry.before[block.before.start]);
                        let before_bottom = before_origin + px(geometry.before[block.before.end]);
                        let after_top = after_origin + px(geometry.after[block.after.start]);
                        let after_bottom = after_origin + px(geometry.after[block.after.end]);
                        if before_bottom.max(after_bottom) < clip.top()
                            || before_top.min(after_top) > clip.bottom()
                        {
                            continue;
                        }

                        // Most changes stay midpoint-to-midpoint. Only a block
                        // taller than the viewport needs its attachment to slide
                        // into view. The clamp is continuous during scrolling.
                        let attachment = |top: Pixels, bottom: Pixels| {
                            let middle = (top + bottom) / 2.;
                            if bottom - top <= clip.size.height {
                                middle
                            } else {
                                let margin = clip.size.height / 4.;
                                middle.clamp(clip.top() + margin, clip.bottom() - margin)
                            }
                        };
                        let before_y = attachment(before_top, before_bottom);
                        let after_y = attachment(after_top, after_bottom);
                        let left = bounds.left() + px(1.);
                        let right = bounds.right() - px(1.);
                        let mut path = PathBuilder::stroke(px(1.));
                        path.move_to(point(left, before_top));
                        path.line_to(point(left, before_bottom));
                        path.move_to(point(right, after_top));
                        path.line_to(point(right, after_bottom));
                        path.move_to(point(left, before_y));
                        path.cubic_bezier_to(
                            point(right, after_y),
                            point(bounds.center().x, before_y),
                            point(bounds.center().x, after_y),
                        );
                        if let Ok(path) = path.build() {
                            window.paint_path(path, color);
                        }
                    }
                });
                let _ = viewer.update(cx, |viewer, cx| {
                    viewer.horizontal_position = resolved_horizontal;
                    if let Some((left_line, right_line, horizontal)) =
                        viewer.live.pending_position.take()
                    {
                        viewer.fluid.position = geometry.position_for_line(&model, left_line, true);
                        for (state, positions, line) in [
                            (&before, &geometry.before, left_line),
                            (&after, &geometry.after, right_line),
                        ] {
                            let y = positions[line.min(positions.len() - 1)];
                            state.update(cx, |editor, cx| {
                                editor.set_scroll_offset(point(px(-horizontal), px(-y)), cx)
                            });
                        }
                        viewer.horizontal_position = horizontal;
                        viewer.reveal_after_layout = false;
                        cx.notify();
                    }
                    if viewer.reveal_after_layout {
                        viewer.reveal_after_layout = false;
                        viewer.navigate_change(0, cx);
                    }
                    if viewer.active_change != active {
                        viewer.active_change = active;
                        cx.notify();
                    }
                });
                let viewer = viewer.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture
                        || !(before_bounds.contains(&event.position)
                            || after_bounds.contains(&event.position))
                    {
                        return;
                    }
                    let delta = event.delta.pixel_delta(line_height);
                    if let Some(delta) = horizontal_wheel(event, line_height) {
                        let _ = viewer.update(cx, |viewer, cx| viewer.scroll_horizontal(delta, cx));
                        cx.stop_propagation();
                        return;
                    }
                    let _ = viewer.update(cx, |viewer, cx| {
                        viewer.navigation_target = None;
                        viewer.scroll_comparison(-f32::from(delta.y), cx)
                    });
                    cx.stop_propagation();
                });
            },
        )
        .size_full()
        .into_any_element()
    }
}
