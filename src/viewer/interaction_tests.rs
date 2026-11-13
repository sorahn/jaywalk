//! These windows use GPUI's test platform: no native activation, mouse or disk settings.
use super::{Arguments, SessionLayout, Settings, Viewer, palette};
use crate::gpui::component::Root;
use clap::Parser;
use gpui_test::{AppContext, Entity, MouseButton, point, px, size};
use gpui_test::{TestAppContext, VisualTestContext};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct Fixture(pub(super) PathBuf);
impl Fixture {
    pub(super) fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "jaywalk-interactions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = Self(root);
        fixture.git(&["init", "-b", "main"]);
        std::fs::write(fixture.0.join("file.txt"), "original\n").unwrap();
        fixture.git(&["add", "."]);
        fixture.git(&["commit", "-m", "initial\n\nFull message body."]);
        std::fs::write(fixture.0.join("file.txt"), "changed\n").unwrap();
        fixture
    }
    pub(super) fn replace(&self, before: &str, after: &str) {
        std::fs::write(self.0.join("file.txt"), before).unwrap();
        self.git(&["add", "file.txt"]);
        self.git(&["commit", "--amend", "--no-edit"]);
        std::fs::write(self.0.join("file.txt"), after).unwrap();
    }
    fn git(&self, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(&self.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(super) fn setup<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Viewer>, &'a mut VisualTestContext) {
    let config = fixture.0.join("settings.toml");
    let contents = "[live]\nenabled=false\n[layout]\nremember_per_repository=false\n[layout.placement]\nhistory_side='left'\nfiles_side='left'\n";
    std::fs::write(&config, contents).unwrap();
    let mut args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    args.config = Some(config);
    args.normalize().unwrap();
    let settings = Settings::parse(contents).unwrap();
    cx.update(|app| {
        gpui_kit::init(app);
        crate::windows::init(app);
        crate::settings_service::init(app);
        crate::recents::init_without_storage(app);
        crate::appearance::init(&settings, app);
        super::init(&args, &settings, app).unwrap();
    });
    let mut viewer = None;
    let (_, cx) = cx.add_window_view(|window, app| {
        let layout = SessionLayout::load(&args, &settings);
        let view = app.new(|cx| Viewer::new(args, settings, layout, window, cx));
        viewer = Some(view.clone());
        Root::new(view, window, app)
    });
    cx.simulate_resize(size(px(1500.), px(800.)));
    draw(cx);
    (viewer.unwrap(), cx)
}
pub(super) fn draw(cx: &mut VisualTestContext) {
    // Resizable groups report measured sizes after layout and schedule a settling frame.
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, app| window.draw(app).clear(app));
    }
}

#[gpui_test::test]
fn recent_picker_keyboard_search_selection_and_escape(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        app.global_mut::<crate::recents::Recents>().repositories =
            vec!["/repos/other".into(), "/repos/man-protos".into()];
    });
    cx.simulate_keystrokes("ctrl-r");
    draw(cx);
    assert!(cx.update(|_, app| matches!(
        viewer.read(app).palette_test_state(app).0,
        Some(palette::PaletteKind::Repositories)
    )));
    cx.simulate_input("pro");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        "pro"
    );
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).2),
        Some(1)
    );
    cx.simulate_keystrokes("up");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).2),
        Some(0)
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        ""
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn drawer_clicks_hide_metadata_and_keep_the_status_at_the_viewer(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (_, cx) = setup(&fixture, cx);
    assert!(cx.debug_bounds("drawer-status-History").is_some());
    assert!(cx.debug_bounds("drawer-status-Files").is_some());
    let toggle = cx.debug_bounds("drawer-toggle-History").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("drawer-status-History").is_none());
    assert!(cx.debug_bounds("drawer-status-Files").is_some());
    let header = cx.debug_bounds("file-header").unwrap();
    let footer = cx.debug_bounds("viewer-footer").unwrap();
    assert!((header.left() - footer.left()).abs() <= px(1.));
    let toggle = cx.debug_bounds("drawer-toggle-Files").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("drawer-status-Files").is_none());
}

#[gpui_test::test]
fn commit_hover_shows_without_selecting_and_click_inspects(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let original = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    let row = cx.debug_bounds("history-row-1").unwrap();
    cx.simulate_mouse_move(row.center(), None, Default::default());
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    draw(cx);
    assert!(cx.debug_bounds("commit-hover").is_some());
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        original
    );
    cx.simulate_click(row.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("commit-hover").is_none());
    assert!(cx.update(|_, app| matches!(
        viewer.read(app).repository.ready().unwrap().scope,
        super::ComparisonScope::Revision {
            working_copy: false,
            ..
        }
    )));
}

#[gpui_test::test]
fn file_header_tracks_window_and_pane_resizing(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (_, cx) = setup(&fixture, cx);
    for width in [1500., 1100., 1800.] {
        cx.simulate_resize(size(px(width), px(800.)));
        draw(cx);
        draw(cx);
        let header = cx.debug_bounds("before-header").unwrap();
        let gutter = cx.debug_bounds("fluid-gutter").unwrap();
        assert!(
            (header.right() - gutter.left()).abs() <= px(1.),
            "header {header:?}, gutter {gutter:?}"
        );
    }
    let gutter = cx.debug_bounds("fluid-gutter").unwrap();
    let position = point(gutter.left() - px(2.), gutter.top() + px(80.));
    cx.simulate_mouse_down(position, MouseButton::Left, Default::default());
    draw(cx);
    cx.simulate_mouse_move(
        position - point(px(10.), px(0.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    draw(cx);
    cx.simulate_mouse_move(
        position - point(px(100.), px(0.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(
        position - point(px(100.), px(0.)),
        MouseButton::Left,
        Default::default(),
    );
    draw(cx);
    let moved = cx.debug_bounds("fluid-gutter").unwrap();
    assert!(moved.left() < gutter.left() - px(50.));
    let header = cx.debug_bounds("before-header").unwrap();
    assert!(
        (header.right() - moved.left()).abs() <= px(1.),
        "header {header:?}, moved {moved:?}, old {gutter:?}"
    );
}

#[gpui_test::test]
fn footer_stays_single_line_and_controls_fit_narrow_viewers(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    for font_size in [12., 18.] {
        cx.update(|_, app| {
            viewer.update(app, |viewer, cx| {
                viewer.settings.ui.footer_font_size = font_size;
                cx.notify();
            })
        });
        for width in [1500., 900., 700., 500.] {
            cx.simulate_resize(size(px(width), px(800.)));
            draw(cx);
            let footer = cx.debug_bounds("viewer-footer").unwrap();
            let controls = cx.debug_bounds("diff-footer-controls").unwrap();
            let counter = cx.debug_bounds("change-counter").unwrap();
            assert!(
                counter.size.height <= footer.size.height,
                "counter {counter:?}, footer {footer:?}"
            );
            assert!(
                controls.left() >= footer.left(),
                "controls {controls:?}, footer {footer:?}"
            );
            assert!(
                controls.right() <= footer.right(),
                "controls {controls:?}, footer {footer:?}"
            );
            let reserved = counter.size.width;
            cx.update(|_, app| {
                viewer.update(app, |viewer, cx| {
                    viewer.active_change = Some(0);
                    cx.notify();
                })
            });
            draw(cx);
            assert_eq!(
                cx.debug_bounds("change-counter").unwrap().size.width,
                reserved
            );
        }
    }
}

fn offsets(viewer: &Entity<Viewer>, cx: &mut VisualTestContext) -> (f32, f32) {
    cx.update(|_, app| {
        let code = viewer.read(app).code().unwrap();
        (
            -f32::from(code.before.read(app).scroll_offset().y),
            -f32::from(code.after.read(app).scroll_offset().y),
        )
    })
}

#[gpui_test::test]
fn fluid_wheel_shared_drag_and_independent_offsets_keep_their_roles(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let common = (0..80).map(|i| format!("common {i}\n")).collect::<String>();
    fixture.replace(
        &format!("{common}{}tail\n", "before\n".repeat(10)),
        &format!("{common}{}tail\n", "after\n".repeat(160)),
    );
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.navigation_target = None;
            viewer.scroll_comparison(-100000., cx);
        })
    });
    draw(cx);
    assert_eq!(offsets(&viewer, cx), (0., 0.));
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
    let scrolled = offsets(&viewer, cx);
    assert!(scrolled.0 > 0. && scrolled.1 > 0.);
    cx.simulate_event(gpui_test::ScrollWheelEvent {
        position,
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(120.))),
        ..Default::default()
    });
    draw(cx);
    assert_eq!(offsets(&viewer, cx), (0., 0.));
    // Direct editor scrollbar changes can move a pane independently.
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.code().unwrap().after.update(cx, |editor, cx| {
                editor.set_scroll_offset(point(px(0.), px(-300.)), cx)
            });
        })
    });
    draw(cx);
    let independent = offsets(&viewer, cx);
    assert_eq!(independent.0, 0.);
    assert!(independent.1 > 0.);
    let gutter = cx.debug_bounds("fluid-gutter").unwrap();
    let bottom = point(gutter.center().x, gutter.bottom() - px(3.));
    cx.simulate_mouse_down(bottom, MouseButton::Left, Default::default());
    draw(cx);
    cx.simulate_mouse_move(
        point(gutter.center().x, gutter.top() + px(2.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(
        point(gutter.center().x, gutter.top() + px(2.)),
        MouseButton::Left,
        Default::default(),
    );
    draw(cx);
    assert_eq!(offsets(&viewer, cx), (0., 0.));
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.navigation_target = None;
            viewer.scroll_comparison(100000., cx);
        })
    });
    draw(cx);
    let end = offsets(&viewer, cx);
    assert!(end.0 > 0. && end.1 > end.0);
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.navigation_target = None;
            viewer.scroll_comparison(100000., cx);
        })
    });
    draw(cx);
    assert_eq!(offsets(&viewer, cx), end);
}

#[gpui_test::test]
fn fluid_wrap_and_resize_preserve_the_reading_line(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let source = (0..140)
        .map(|i| format!("line {i} {}\n", "long text ".repeat(15)))
        .collect::<String>();
    fixture.replace(&source, &source.replace("line 90", "changed line 90"));
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.navigation_target = None;
            viewer.scroll_comparison(-100000., cx);
        })
    });
    draw(cx);
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.navigation_target = None;
            viewer.scroll_comparison(800., cx);
        })
    });
    draw(cx);
    let line = cx.update(|_, app| {
        viewer
            .read(app)
            .code()
            .unwrap()
            .before
            .read(app)
            .visible_row_range()
            .unwrap()
            .start
    });
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: jaywalk::commands::CommandId::ToggleWrap,
    });
    draw(cx);
    let wrapped = cx.update(|_, app| {
        viewer
            .read(app)
            .code()
            .unwrap()
            .before
            .read(app)
            .visible_row_range()
            .unwrap()
            .start
    });
    assert!(line.abs_diff(wrapped) <= 1, "line {line} -> {wrapped}");
    cx.simulate_resize(size(px(1100.), px(800.)));
    draw(cx);
    let resized = cx.update(|_, app| {
        viewer
            .read(app)
            .code()
            .unwrap()
            .before
            .read(app)
            .visible_row_range()
            .unwrap()
            .start
    });
    assert!(
        wrapped.abs_diff(resized) <= 1,
        "wrapped {wrapped} -> resized {resized}"
    );
    let gutter = cx.debug_bounds("fluid-gutter").unwrap();
    let position = point(gutter.left() - px(2.), gutter.top() + px(80.));
    cx.simulate_mouse_down(position, MouseButton::Left, Default::default());
    draw(cx);
    for delta in [10., 80.] {
        cx.simulate_mouse_move(
            position - point(px(delta), px(0.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        draw(cx);
    }
    cx.simulate_mouse_up(
        position - point(px(80.), px(0.)),
        MouseButton::Left,
        Default::default(),
    );
    draw(cx);
    let dragged = cx.update(|_, app| {
        viewer
            .read(app)
            .code()
            .unwrap()
            .before
            .read(app)
            .visible_row_range()
            .unwrap()
            .start
    });
    assert!(
        resized.abs_diff(dragged) <= 1,
        "resized {resized} -> dragged {dragged}"
    );
}

#[gpui_test::test]
fn choosing_a_branch_keeps_the_loaded_overview_caption_visible(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let branch = "feature/a-long-selected-branch-name";
    fixture.git(&["checkout", "-b", branch]);
    std::fs::write(fixture.0.join("file.txt"), "feature row\n".repeat(100)).unwrap();
    fixture.git(&["add", "file.txt"]);
    fixture.git(&["commit", "-m", "Feature summary"]);
    let (viewer, cx) = setup(&fixture, cx);
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: jaywalk::commands::CommandId::Overview,
    });
    draw(cx);
    let reference = |name: &str| jaywalk::branch_review::RefName {
        name: name.into(),
        remote: None,
    };
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.open_review_target(
                jaywalk::branch_review::Target::Branch {
                    tip: jaywalk::branch_review::Tip::Reference(reference(branch)),
                    base: reference("main"),
                },
                window,
                cx,
            )
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).scope_label().contains(branch)));
    assert!(cx.update(|_, app| viewer.read(app).selected_file.is_none()));
    let caption = cx.debug_bounds("comparison-caption").unwrap();
    assert!(caption.size.width > px(100.), "caption {caption:?}");
    cx.simulate_event(gpui_test::ScrollWheelEvent {
        position: point(caption.left() + px(20.), caption.bottom() + px(200.)),
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-1000.))),
        ..Default::default()
    });
    draw(cx);
    assert!(cx.debug_bounds("comparison-caption").unwrap().size.width > px(100.));
    cx.simulate_resize(size(px(800.), px(800.)));
    draw(cx);
    assert!(cx.debug_bounds("comparison-caption").unwrap().size.width > px(40.));
}

#[gpui_test::test]
fn loading_completion_repaints_after_initial_geometry_frames(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.selected_file = None;
            viewer.panels = super::panels::PanelLayout::new(cx);
            viewer.load_repository(Some("file.txt".into()), window, cx);
        })
    });
    // Unlike setup(), paint geometry while the load task is still pending.
    // Native startup can take this order; parking the executor first hides it.
    for _ in 0..2 {
        cx.update(|window, app| window.draw(app).clear(app));
    }
    assert!(cx.debug_bounds("history-row-0").is_none());
    cx.run_until_parked();
    cx.update(|window, app| window.draw(app).clear(app));
    assert!(cx.debug_bounds("history-row-0").is_some());
    assert!(cx.debug_bounds("file-header").is_some());
}

#[gpui_test::test]
fn page_click_moves_one_viewport_without_starting_a_jump_drag(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fixture.replace(&"before\n".repeat(200), &"after\n".repeat(400));
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.settings.panels.diff.shared_scrollbar_click =
                jaywalk::settings::ScrollbarClick::Page;
            viewer.navigation_target = None;
            viewer.scroll_comparison(-100000., cx);
        })
    });
    draw(cx);
    let viewport = cx.update(|_, app| {
        f32::from(
            viewer
                .read(app)
                .code()
                .unwrap()
                .before
                .read(app)
                .input_bounds()
                .size
                .height,
        )
    });
    let gutter = cx.debug_bounds("fluid-gutter").unwrap();
    let click = point(gutter.center().x, gutter.bottom() - px(2.));
    cx.simulate_mouse_down(click, MouseButton::Left, Default::default());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).fluid.position),
        viewport
    );
    assert!(cx.update(|_, app| viewer.read(app).fluid_drag.is_none()));
    cx.simulate_mouse_move(
        point(gutter.center().x, gutter.top() + px(2.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).fluid.position),
        viewport
    );
    cx.simulate_mouse_up(click, MouseButton::Left, Default::default());
    draw(cx);
    let thumb = cx.update(|_, app| {
        let viewer = viewer.read(app);
        let code = viewer.code().unwrap();
        let geometry =
            super::comparison_geometry(code.before.read(app), code.after.read(app), &code.model);
        jaywalk::fluid::SharedScrollTrack::new(
            geometry.scroll_extent(&code.model),
            viewport,
            f32::from(gutter.size.height),
            viewer.settings.panels.diff.shared_scrollbar_height,
            viewer.fluid.position,
        )
        .thumb
    });
    let grab = point(
        gutter.center().x,
        gutter.top() + px((thumb.start + thumb.end) / 2.),
    );
    cx.simulate_mouse_down(grab, MouseButton::Left, Default::default());
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).fluid_drag.is_some()));
    cx.simulate_mouse_move(
        point(gutter.center().x, gutter.top()),
        Some(MouseButton::Left),
        Default::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(
        point(gutter.center().x, gutter.top()),
        MouseButton::Left,
        Default::default(),
    );
    draw(cx);
    assert_eq!(cx.update(|_, app| viewer.read(app).fluid.position), 0.);
}

#[gpui_test::test]
fn standard_folding_disclosures_and_navigation_match_both_modes(cx: &mut TestAppContext) {
    use jaywalk::settings::DiffView;
    let fixture = Fixture::new();
    let before = (1..=180)
        .map(|line| format!("unchanged λ {line}\n"))
        .collect::<String>();
    let after = before
        .replace("λ 20\n", "changed 20\n")
        .replace("λ 140\n", "changed 140\n");
    fixture.replace(&before, &after);
    let (viewer, cx) = setup(&fixture, cx);
    for mode in [DiffView::SideBySide, DiffView::Inline] {
        cx.update(|_, app| {
            viewer.update(app, |viewer, cx| {
                viewer.collapse_unchanged = false;
                viewer.apply_standard_context(cx);
                viewer.set_view_mode(mode, cx);
            })
        });
        draw(cx);
        let state = cx.update(|_, app| {
            viewer
                .read(app)
                .code()
                .unwrap()
                .standard
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .clone()
        });
        assert_eq!(cx.update(|_, app| state.read(app).context_lines()), None);
        // The standard widget's selected group drives the same fixed-width counter.
        cx.simulate_keystrokes("f7");
        draw(cx);
        let first = cx.update(|_, app| state.read(app).change_navigation_position());
        assert_eq!(first.1, 2);
        assert!(first.0.is_some());
        assert!(cx.debug_bounds("change-counter").is_some());
        let width = cx.debug_bounds("change-counter").unwrap().size.width;
        cx.simulate_keystrokes("f7");
        draw(cx);
        assert_ne!(
            cx.update(|_, app| state.read(app).change_navigation_position().0),
            first.0
        );
        assert_eq!(cx.debug_bounds("change-counter").unwrap().size.width, width);
        let button = cx.debug_bounds("file-context").unwrap().center();
        cx.simulate_click(button, Default::default());
        draw(cx);
        assert_eq!(cx.update(|_, app| state.read(app).context_lines()), Some(3));
        assert!(cx.debug_bounds("change-counter").is_none());
        // Copy uses source ranges, including folded Unicode lines, without fold labels.
        cx.update(|_, app| {
            state.update(app, |state, cx| {
                let path = state.files()[0].path().clone();
                state.set_selected_lines(
                    Some(gpui_kit::component::diff::DiffLineRange::new(
                        path,
                        gpui_kit::component::diff::DiffSide::Modified,
                        1,
                        180,
                    )),
                    cx,
                );
                assert_eq!(state.selected_text(cx), after);
                state.set_selected_lines(None, cx);
            });
        });
        let offset = cx.update(|_, app| state.read(app).viewport_offset());
        // Disabled buttons and remappable shortcuts must both leave context alone.
        let button = cx.debug_bounds("next-change").unwrap().center();
        cx.simulate_click(button, Default::default());
        cx.simulate_keystrokes("f7 shift-f7");
        draw(cx);
        let actual = cx.update(|_, app| state.read(app).viewport_offset());
        assert_eq!(actual.item_ix, offset.item_ix);
        assert_eq!(actual.offset_in_item, offset.offset_in_item);
        cx.update(|_, app| {
            state.update(app, |state, cx| {
                state.restore_viewport_offset(
                    gpui_test::ListOffset {
                        item_ix: 0,
                        offset_in_item: px(0.),
                    },
                    cx,
                )
            })
        });
        draw(cx);
        let disclosure = cx
            .debug_bounds("diff-expand-0-0")
            .expect("leading unchanged range has a disclosure");
        cx.simulate_click(disclosure.center(), Default::default());
        draw(cx);
        assert!(
            cx.debug_bounds("diff-expand-0-0").is_none(),
            "the shared disclosure expands the row in either mode"
        );
        let button = cx.debug_bounds("file-context").unwrap().center();
        cx.simulate_click(button, Default::default());
        draw(cx);
        assert_eq!(cx.update(|_, app| state.read(app).context_lines()), None);
        assert!(cx.debug_bounds("change-counter").is_some());
    }
    // Fluid ignores the standard folding preference.
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.collapse_unchanged = true;
            viewer.apply_standard_context(cx);
            viewer.set_view_mode(DiffView::Fluid, cx);
        })
    });
    draw(cx);
    assert!(cx.debug_bounds("file-context").is_none());
    assert!(cx.debug_bounds("change-counter").is_some());
}

#[gpui_test::test]
fn standard_folding_settings_apply_live_and_preserve_toolbar_overrides(cx: &mut TestAppContext) {
    use jaywalk::settings::DiffView;
    let fixture = Fixture::new();
    let before = (1..=100)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    fixture.replace(&before, &before.replace("line 50\n", "changed\n"));
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.set_view_mode(DiffView::SideBySide, cx);
            let mut settings = viewer.settings.clone();
            settings.diff.collapse_unchanged = true;
            settings.diff.context_lines = 1;
            settings.diff.expansion_lines = 5;
            viewer.apply_settings(settings, window, cx);
        })
    });
    draw(cx);
    let state = cx.update(|_, app| {
        viewer
            .read(app)
            .code()
            .unwrap()
            .standard
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone()
    });
    assert_eq!(cx.update(|_, app| state.read(app).context_lines()), Some(1));
    let button = cx.debug_bounds("file-context").unwrap().center();
    cx.simulate_click(button, Default::default());
    draw(cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            let mut settings = viewer.settings.clone();
            settings.diff.context_lines = 2;
            viewer.apply_settings(settings, window, cx);
            assert!(
                !viewer.collapse_unchanged,
                "unrelated settings retain the toolbar override"
            );
        })
    });
    draw(cx);
    assert_eq!(cx.update(|_, app| state.read(app).context_lines()), None);
    let button = cx.debug_bounds("file-context").unwrap().center();
    cx.simulate_click(button, Default::default());
    draw(cx);
    assert_eq!(cx.update(|_, app| state.read(app).context_lines()), Some(2));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).layout.saved.collapse_unchanged),
        None,
        "fixture persistence stays disabled"
    );
}

#[gpui_test::test]
fn reference_browser_bounds_work_and_supports_search_keyboard_and_reopen(cx: &mut TestAppContext) {
    use jaywalk::{
        history::{History, Reference, ReferenceKind},
        references::ReferenceSnapshot,
    };
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        viewer.update(app, |v, cx| {
            let super::LoadState::Loaded(session) = &mut v.repository else {
                panic!("fixture loaded")
            };
            let old = session.history.as_ref().unwrap();
            let target = old.revisions.last().unwrap().commit_id.clone();
            let snapshot = ReferenceSnapshot {
                references: (0..1400)
                    .map(|i| Reference {
                        name: format!("feature-{i:04}"),
                        target: target.clone(),
                        kind: ReferenceKind::Local,
                        tracked_by: vec![],
                        conflicted: false,
                    })
                    .collect(),
                ..Default::default()
            };
            session.history = Ok(std::rc::Rc::new(History {
                revisions: old.revisions.clone(),
                graph: old.graph.clone(),
                lane_count: old.lane_count,
                references: std::sync::Arc::new(snapshot),
            }));
            v.references.open = true;
            cx.notify();
        })
    });
    super::reference_browser::RENDERED.with(|count| count.set(0));
    draw(cx);
    let rendered = super::reference_browser::RENDERED.with(|count| count.get());
    assert!(
        rendered > 0 && rendered < 150,
        "only visible rows should render: {rendered}"
    );
    assert!(cx.debug_bounds("reference-browser").is_some());
    cx.simulate_input("1399");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .references
            .inspected
            .as_ref()
            .unwrap()
            .name
            .clone()),
        "feature-1399"
    );
    assert!(!cx.update(|_, app| viewer.read(app).references.open));
    cx.update(|_, app| {
        viewer.update(app, |v, cx| {
            v.references.open = true;
            cx.notify();
        })
    });
    draw(cx);
    cx.simulate_input("feature-139");
    draw(cx);
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .references
            .inspected
            .as_ref()
            .unwrap()
            .name
            .clone()),
        "feature-1391"
    );
    cx.update(|_, app| {
        viewer.update(app, |v, cx| {
            v.references.open = true;
            cx.notify();
        })
    });
    draw(cx);
    cx.simulate_input("no-match");
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).references.open));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!cx.update(|_, app| viewer.read(app).references.open));
}

#[gpui_test::test]
fn history_scrollbar_drags_without_changing_the_comparison(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    for index in 0..70 {
        fixture.git(&[
            "commit",
            "--allow-empty",
            "-m",
            &format!("revision {index}"),
        ]);
    }
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        gpui_kit::component::Theme::set_scrollbar_mode(
            gpui_kit::component::scroll::ScrollbarMode::Always,
            app,
        );
    });
    draw(cx);
    let (bounds, before, selected) = cx.update(|_, app| {
        let viewer = viewer.read(app);
        let scroll = viewer.history_scroll.0.borrow();
        (
            scroll.base_handle.bounds(),
            scroll.base_handle.offset(),
            viewer.comparison().unwrap().identity(),
        )
    });
    let grab = point(bounds.right() - px(7.), bounds.top() + px(12.));
    let end = point(grab.x, bounds.bottom() - px(30.));
    cx.simulate_mouse_move(grab, None, Default::default());
    draw(cx);
    cx.simulate_mouse_down(grab, MouseButton::Left, Default::default());
    draw(cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
    draw(cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    draw(cx);
    cx.update(|_, app| {
        let viewer = viewer.read(app);
        let after = viewer.history_scroll.0.borrow().base_handle.offset();
        assert!(
            after.y < before.y - px(300.),
            "history did not scroll: {before:?} -> {after:?}"
        );
        assert_eq!(viewer.comparison().unwrap().identity(), selected);
    });
}
