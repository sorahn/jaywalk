//! Opt-in measurements, separate from correctness tests and native GPU timings.
use super::interaction_tests::{Fixture, draw, setup};
use gpui_test::{TestAppContext, point, px, size};
use jaywalk::{
    history::{History, Reference, ReferenceKind, TrackingState},
    references::ReferenceSnapshot,
    settings::DiffView,
};
use std::{rc::Rc, sync::Arc, time::Instant};

fn record(metric: &str, elapsed: std::time::Duration) {
    eprintln!(
        "{}",
        serde_json::json!({"metric":metric,"elapsed_ms":elapsed.as_secs_f64()*1000.,"frames_per_draw":3,"optimized":!cfg!(debug_assertions)})
    );
}

#[gpui_test::test]
#[ignore = "opt-in synthetic performance measurement"]
fn profile_reference_browser(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    for count in [70usize, 700, 1400, 5000] {
        cx.update(|_, app| {
            viewer.update(app, |viewer, cx| {
                let super::LoadState::Loaded(session) = &mut viewer.repository else {
                    panic!("fixture not loaded")
                };
                let old = session.history.as_ref().unwrap();
                let references = ReferenceSnapshot {
                    references: (0..count)
                        .map(|i| Reference {
                            name: format!("feature/team-long-branch-name-{i:05}"),
                            target: format!("{i:040x}"),
                            kind: if i < count / 2 {
                                ReferenceKind::Local
                            } else {
                                ReferenceKind::Remote {
                                    remote: "origin".into(),
                                    tracking: Some(TrackingState::Untracked),
                                }
                            },
                            tracked_by: vec![],
                            conflicted: false,
                        })
                        .collect(),
                    remotes: vec!["origin".into()],
                    ..Default::default()
                };
                session.history = Ok(Rc::new(History {
                    revisions: old.revisions.clone(),
                    graph: old.graph.clone(),
                    lane_count: old.lane_count,
                    references: Arc::new(references),
                }));
                viewer.references.open = false;
                cx.notify();
            })
        });
        draw(cx);
        for attempt in 0..2 {
            let start = Instant::now();
            cx.update(|_, app| {
                viewer.update(app, |v, cx| {
                    v.references.open = true;
                    cx.notify();
                })
            });
            draw(cx);
            record(
                &format!("references/{count}/open-{attempt}"),
                start.elapsed(),
            );
            let start = Instant::now();
            draw(cx);
            record(
                &format!("references/{count}/redraw-{attempt}"),
                start.elapsed(),
            );
            cx.update(|_, app| {
                viewer.update(app, |v, cx| {
                    v.references.open = false;
                    cx.notify();
                })
            });
            draw(cx);
        }
    }
}

#[gpui_test::test]
#[ignore = "opt-in synthetic performance measurement"]
fn profile_viewer_modes(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let before: String = (0..5000)
        .map(|i| format!("source line {i}: {}\n", "synthetic wrapped text ".repeat(5)))
        .collect();
    let after = before
        .replace("line 20:", "changed line 20:")
        .replace("line 4500:", "changed line 4500:");
    fixture.replace(&before, &after);
    let start = Instant::now();
    let (viewer, cx) = setup(&fixture, cx);
    record("viewer/open-5000-lines", start.elapsed());
    for mode in [DiffView::Fluid, DiffView::Inline, DiffView::SideBySide] {
        cx.update(|_, app| viewer.update(app, |v, cx| v.set_view_mode(mode, cx)));
        draw(cx);
        for width in [1500., 850., 1200.] {
            let start = Instant::now();
            cx.simulate_resize(size(px(width), px(800.)));
            draw(cx);
            record(&format!("viewer/{mode:?}/resize-{width}"), start.elapsed());
        }
        let start = Instant::now();
        for _ in 0..10 {
            let position = cx.update(|_, app| {
                viewer
                    .read(app)
                    .code()
                    .unwrap()
                    .before
                    .read(app)
                    .input_bounds()
                    .center()
            });
            cx.simulate_event(gpui_test::ScrollWheelEvent {
                position,
                delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-120.))),
                ..Default::default()
            });
            draw(cx);
        }
        record(&format!("viewer/{mode:?}/10-wheel-events"), start.elapsed());
    }
}
