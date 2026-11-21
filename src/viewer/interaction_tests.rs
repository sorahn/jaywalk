//! These windows use GPUI's test platform: no native activation, mouse or disk settings.
use super::{Arguments, SessionLayout, Settings, Viewer, palette};
use crate::gpui::component::Root;
use clap::Parser;
use gpui_test::{AppContext, Entity, MouseButton, point, px, size};
use gpui_test::{Focusable, TestAppContext, VisualTestContext};
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
    pub(super) fn git(&self, args: &[&str]) {
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
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    setup_arguments(fixture, args, cx)
}

fn setup_arguments<'a>(
    fixture: &Fixture,
    args: Arguments,
    cx: &'a mut TestAppContext,
) -> (Entity<Viewer>, &'a mut VisualTestContext) {
    setup_arguments_with_settings(fixture, args, "", cx)
}
fn setup_arguments_with_settings<'a>(
    fixture: &Fixture,
    args: Arguments,
    extra: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Viewer>, &'a mut VisualTestContext) {
    let contents = "[local_review]\nfetch_missing=false\n[live]\nenabled=false\n[layout]\nremember_per_repository=false\n[layout.placement]\nhistory_side='left'\nfiles_side='left'\n";
    let contents = format!("{contents}{extra}");
    setup_arguments_with_config(fixture, args, &contents, cx)
}
fn setup_arguments_with_config<'a>(
    fixture: &Fixture,
    mut args: Arguments,
    contents: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Viewer>, &'a mut VisualTestContext) {
    let config = fixture.0.join("settings.toml");
    std::fs::write(&config, contents).unwrap();
    args.config = Some(config);
    args.normalize().unwrap();
    let settings = Settings::parse(contents).unwrap();
    cx.update(|app| {
        gpui_kit::init(app);
        crate::windows::init(app);
        if settings.layout.remember_per_repository {
            crate::interface_state::init_for_test(app);
        }
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
        "~pro"
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
    cx.simulate_event(gpui_test::ScrollEvent {
        position,
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-120.))),
        ..Default::default()
    });
    draw(cx);
    let scrolled = offsets(&viewer, cx);
    assert!(scrolled.0 > 0. && scrolled.1 > 0.);
    cx.simulate_event(gpui_test::ScrollEvent {
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
    cx.simulate_event(gpui_test::ScrollEvent {
        position: point(caption.left() + px(20.), caption.bottom() + px(200.)),
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-1000.))),
        ..Default::default()
    });
    draw(cx);
    assert!(
        cx.debug_bounds("comparison-caption").unwrap().size.width > caption.size.width,
        "collapsed subject should grow the caption"
    );
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
            session.history = Ok(std::sync::Arc::new(History {
                revisions: old.revisions.clone(),
                graph: old.graph.clone(),
                lane_count: old.lane_count,
                references: std::sync::Arc::new(snapshot),
                pagination: None,
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

#[gpui_test::test]
fn external_diff_caption_fits_without_filling_the_header(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let left = fixture.0.join("before.txt");
    let right = fixture.0.join("after.txt");
    std::fs::write(&left, "before\n").unwrap();
    std::fs::write(&right, "after\n").unwrap();
    let args = Arguments::parse_from([
        "jaywalk",
        "--diff",
        left.to_str().unwrap(),
        right.to_str().unwrap(),
    ]);
    let (viewer, cx) = setup_arguments(&fixture, args, cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).scope_label()),
        "External diff"
    );
    for file_selected in [false, true] {
        if file_selected {
            cx.update(|window, app| {
                viewer.update(app, |viewer, cx| {
                    viewer.select_path("after.txt", window, cx)
                });
            });
        }
        for width in [1500., 800., 1500.] {
            cx.simulate_resize(size(px(width), px(800.)));
            draw(cx);
            let header = cx.debug_bounds("comparison-header").unwrap();
            let label = cx.debug_bounds("review-source-label").unwrap();
            let button = cx.debug_bounds("review-source-button").unwrap();
            let measured = cx.update(|window, app| {
                let text: gpui_test::SharedString = "External diff".into();
                let run = gpui_test::TextRun {
                    len: text.len(),
                    font: gpui_test::Font {
                        family: gpui_kit::component::Theme::global(app).font_family.clone(),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                window
                    .text_system()
                    .shape_line(text, px(14.), &[run], None)
                    .width
            });
            assert!(
                label.size.width >= measured,
                "caption clipped: {label:?}, text {measured:?}, button {button:?}"
            );
            assert!(
                button.size.width < px(220.),
                "hover target too wide: {button:?}"
            );
            assert!(button.right() < header.right() - px(100.));
        }
    }
}

fn check_folded_external_context(first_change: usize, cx: &mut TestAppContext) {
    use jaywalk::settings::DiffView;
    let fixture = Fixture::new();
    let before = (1..=90)
        .map(|line| format!("unchanged {line}\n"))
        .collect::<String>();
    let after = before.replace(
        &format!("unchanged {first_change}\n"),
        &format!("changed {first_change}\n"),
    );
    let left = fixture.0.join("before.txt");
    let right = fixture.0.join("after.txt");
    std::fs::write(&left, before).unwrap();
    std::fs::write(&right, after).unwrap();
    let args = Arguments::parse_from([
        "jaywalk",
        "--diff",
        left.to_str().unwrap(),
        right.to_str().unwrap(),
        "--file",
        "after.txt",
    ]);
    let (viewer, cx) = setup_arguments(&fixture, args, cx);
    for mode in [DiffView::SideBySide, DiffView::Inline] {
        cx.update(|_, app| {
            viewer.update(app, |viewer, cx| {
                viewer.collapse_unchanged = true;
                viewer.apply_standard_context(cx);
                viewer.set_view_mode(mode, cx);
            })
        });
        draw(cx);
        assert!(
            cx.debug_bounds("diff-expand-0-0").is_some(),
            "{mode:?} expanded unchanged lines before change {first_change}"
        );
        cx.update(|_, app| {
            viewer.update(app, |viewer, cx| viewer.set_view_mode(DiffView::Fluid, cx))
        });
        draw(cx);
    }
    // An explicit source-line reveal still opens hidden context.
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.set_view_mode(DiffView::SideBySide, cx);
            let state = viewer
                .code()
                .unwrap()
                .standard
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .clone();
            state.update(cx, |state, cx| {
                let path = state.files()[0].path().clone();
                state.scroll_to_line(
                    gpui_kit::component::diff::DiffLinePosition::new(
                        path,
                        gpui_kit::component::diff::DiffSide::Modified,
                        1,
                    ),
                    cx,
                );
            });
        })
    });
    draw(cx);
    assert!(
        cx.debug_bounds("diff-expand-0-0").is_none(),
        "explicit reveal should expand the leading fold"
    );
}

#[gpui_test::test]
fn external_line_29_change_keeps_leading_context_folded(cx: &mut TestAppContext) {
    check_folded_external_context(29, cx);
}

#[gpui_test::test]
fn external_line_36_change_keeps_leading_context_folded(cx: &mut TestAppContext) {
    check_folded_external_context(36, cx);
}

#[gpui_test::test]
fn pair_gesture_swap_and_all_changes_retain_captured_working_contents(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let head = cx.update(|_, app| {
        viewer
            .read(app)
            .repository
            .ready()
            .unwrap()
            .home
            .base()
            .to_owned()
    });
    let row = cx.debug_bounds("history-row-1").unwrap();
    let modifiers = gpui_test::Modifiers {
        platform: cfg!(target_os = "macos"),
        control: !cfg!(target_os = "macos"),
        ..Default::default()
    };
    cx.simulate_click(row.center(), modifiers);
    draw(cx);
    cx.update(|_, app| {
        let session = viewer.read(app).repository.ready().unwrap();
        assert!(matches!(
            session.review.source,
            super::review_context::ReviewSource::Target(
                jaywalk::branch_review::Target::Pair { .. }
            )
        ));
        assert_eq!(session.review.aggregate.base(), head);
        assert_eq!(session.review.aggregate.tip(), "working tree");
    });
    std::fs::write(fixture.0.join("file.txt"), "later edit\n").unwrap();
    cx.update(|window, app| viewer.update(app, |v, cx| v.swap_comparison(window, cx)));
    draw(cx);
    cx.update(|_, app| {
        let session = viewer.read(app).repository.ready().unwrap();
        assert_eq!(session.review.aggregate.base(), "working tree");
        let file = &session.review.aggregate.files()[0];
        assert_eq!(
            session.review.aggregate.contents(file).unwrap(),
            ("changed\n".into(), "original\n".into())
        );
    });
    cx.update(|window, app| viewer.update(app, |v, cx| v.select_revision(&head, window, cx)));
    draw(cx);
    cx.update(|window, app| viewer.update(app, |v, cx| v.all_review_changes(None, window, cx)));
    draw(cx);
    cx.update(|_, app| {
        let session = viewer.read(app).repository.ready().unwrap();
        assert_eq!(session.comparison.ready().unwrap().base(), "working tree");
        assert_eq!(
            session
                .comparison
                .ready()
                .unwrap()
                .contents(&session.review.aggregate.files()[0])
                .unwrap()
                .0,
            "changed\n"
        );
    });

    cx.update(|window, app| viewer.update(app, |v, cx| v.navigate_view(false, window, cx)));
    draw(cx);
    cx.update(|_, app| {
        assert!(matches!(
            viewer.read(app).repository.ready().unwrap().scope,
            super::ComparisonScope::Revision { .. }
        ))
    });
    cx.update(|window, app| viewer.update(app, |v, cx| v.navigate_view(true, window, cx)));
    draw(cx);
    cx.update(|_, app| {
        assert_eq!(
            viewer
                .read(app)
                .repository
                .ready()
                .unwrap()
                .review
                .aggregate
                .base(),
            "working tree"
        )
    });
}

#[test]
fn snapshot_pairs_order_by_ancestry_or_selection_and_keep_divergent_direction() {
    use jaywalk::{
        branch_review::{Catalog, Target},
        repository::Comparison,
        settings::PairOrder,
    };
    let fixture = Fixture::new();
    let base = Comparison::load(&fixture.0, None, "@")
        .unwrap()
        .recorded_tip()
        .to_owned();
    fixture.git(&["add", "file.txt"]);
    fixture.git(&["commit", "-m", "second"]);
    let home = Comparison::load(&fixture.0, None, "@").unwrap();
    let tip = home.recorded_tip().to_owned();
    let catalog = Catalog::capture(&home).unwrap();
    let ancestry = catalog.pair(&tip, &base, PairOrder::Ancestry).unwrap();
    assert_eq!(
        ancestry,
        Target::Pair {
            left: base.clone(),
            right: tip.clone()
        }
    );
    let selected = catalog.pair(&tip, &base, PairOrder::Selection).unwrap();
    let reverse = catalog.resolve(&selected).unwrap();
    assert_eq!(
        reverse.contents(&reverse.files()[0]).unwrap(),
        ("changed\n".into(), "original\n".into())
    );
    let equal = catalog
        .resolve(&Target::Pair {
            left: tip.clone(),
            right: tip,
        })
        .unwrap();
    assert!(equal.files().is_empty());
    fixture.git(&["checkout", "-b", "other", &base]);
    std::fs::write(fixture.0.join("other.txt"), "other\n").unwrap();
    fixture.git(&["add", "other.txt"]);
    fixture.git(&["commit", "-m", "divergent"]);
    let other = Comparison::load(&fixture.0, None, "@").unwrap();
    let catalog = Catalog::capture(&other).unwrap();
    let first = home.recorded_tip();
    let second = other.recorded_tip();
    assert_eq!(
        catalog.pair(first, second, PairOrder::Ancestry).unwrap(),
        Target::Pair {
            left: first.into(),
            right: second.into()
        }
    );
}

#[gpui_test::test]
fn incremental_difftool_delivery_preserves_inspection_and_restores_the_full_capture(
    cx: &mut TestAppContext,
) {
    use jaywalk::difftool::{Capture, File, Identity, Intent};
    let fixture = Fixture::new();
    let head = jaywalk::repository::Comparison::load(&fixture.0, None, "@")
        .unwrap()
        .base()
        .to_owned();
    let owned = fixture.0.join("owned");
    let left = owned.join("left");
    let right = owned.join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("file.txt"), "original\n").unwrap();
    std::fs::write(right.join("file.txt"), "changed\n").unwrap();
    let mut capture = Capture {
        identity: Identity {
            pid: 999999,
            started: 1,
            boot: [0, 0],
        },
        grouped: true,
        directory: false,
        repository: fixture.0.clone(),
        working_head: Some(head.clone()),
        received: 1,
        total: 2,
        files: vec![File {
            before: "file.txt".into(),
            after: "file.txt".into(),
            kind: "modified".into(),
            before_object: None,
            after_object: None,
        }],
        intent: Some(Intent {
            left: head.clone(),
            right: "working tree".into(),
        }),
    };
    let mut arguments = Arguments::parse_from([
        "jaywalk",
        fixture.0.to_str().unwrap(),
        "--diff",
        left.to_str().unwrap(),
        right.to_str().unwrap(),
    ]);
    arguments.difftool = Some(capture.clone());
    let (viewer, cx) = setup_arguments(&fixture, arguments.clone(), cx);
    cx.update(|window, app| viewer.update(app, |v, cx| v.select_revision(&head, window, cx)));
    draw(cx);
    std::fs::write(right.join("second.txt"), "second\n").unwrap();
    capture.received = 2;
    capture.intent = None; // Unknown/index-like endpoints must still restore their supplied aggregate.
    capture.files.push(File {
        before: "second.txt".into(),
        after: "second.txt".into(),
        kind: "added".into(),
        before_object: None,
        after_object: None,
    });
    arguments.difftool = Some(capture);
    cx.update(|window, app| viewer.update(app, |v, cx| v.update_difftool(arguments, window, cx)));
    draw(cx);
    cx.update(|_, app| {
        let session = viewer.read(app).repository.ready().unwrap();
        assert!(matches!(
            session.scope,
            super::ComparisonScope::Revision { .. }
        ));
        assert_eq!(session.review.aggregate.files().len(), 2);
    });
    cx.update(|window, app| viewer.update(app, |v, cx| v.all_review_changes(None, window, cx)));
    draw(cx);
    cx.update(|_, app| {
        let session = viewer.read(app).repository.ready().unwrap();
        assert!(matches!(session.scope, super::ComparisonScope::Aggregate));
        assert!(viewer.read(app).selected_history_commit().is_none());
        assert_eq!(session.comparison.ready().unwrap().files().len(), 2);
    });
}

#[gpui_test::test]
fn application_pages_keep_the_comparison_editors_selection_scroll_and_drawers(
    cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId;
    use jaywalk::settings::DiffView;
    let fixture = Fixture::new();
    let before = (1..=250)
        .map(|line| format!("unchanged {line}\n"))
        .collect::<String>();
    let after = (1..=250)
        .map(|line| {
            format!(
                "{} {line}\n",
                if line % 10 == 0 {
                    "modified"
                } else {
                    "unchanged"
                }
            )
        })
        .collect::<String>();
    fixture.replace(&before, &after);
    crate::account_ui::tests::setup(cx);
    let (viewer, cx) = setup(&fixture, cx);
    for mode in [DiffView::SideBySide, DiffView::Inline, DiffView::Fluid] {
        cx.update(|_, app| viewer.update(app, |viewer, cx| viewer.set_view_mode(mode, cx)));
        draw(cx);
        if mode == DiffView::Fluid {
            cx.update(|_, app| viewer.update(app, |viewer, cx| viewer.scroll_comparison(250., cx)));
            draw(cx);
        }
        let fluid_position = cx.update(|_, app| viewer.read(app).fluid.position);
        let (comparison, before_editor, after_editor, standard, path, drawers) =
            cx.update(|_, app| {
                let viewer = viewer.read(app);
                let code = viewer.code().unwrap();
                (
                    viewer
                        .repository
                        .ready()
                        .unwrap()
                        .comparison
                        .ready()
                        .unwrap()
                        .clone(),
                    code.before.clone(),
                    code.after.clone(),
                    code.standard.as_ref().unwrap().as_ref().unwrap().clone(),
                    viewer.selected_path(),
                    (
                        viewer.layout.saved.history_hidden,
                        viewer.layout.saved.files_hidden,
                    ),
                )
            });
        // Keep a real folded-source selection and a nonzero viewport.
        cx.update(|_, app| {
            standard.update(app, |state, cx| {
                let path = state.files()[0].path().clone();
                state.set_selected_lines(
                    Some(gpui_kit::component::diff::DiffLineRange::new(
                        path,
                        gpui_kit::component::diff::DiffSide::Modified,
                        118,
                        122,
                    )),
                    cx,
                );
                state.restore_viewport_offset(
                    gpui_test::ListOffset {
                        item_ix: 6,
                        offset_in_item: px(2.),
                    },
                    cx,
                );
            })
        });
        draw(cx);
        let (offset, text, context) = cx.update(|_, app| {
            let state = standard.read(app);
            (
                state.viewport_offset(),
                state.selected_text(app),
                state.context_lines(),
            )
        });
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(CommandId::Accounts, window, cx)
            })
        });
        draw(cx);
        assert!(cx.debug_bounds("accounts-page").is_some());
        assert!(cx.debug_bounds("drawer-status-History").is_none());
        let accounts =
            cx.update(|_, app| viewer.read(app).account_page.as_ref().unwrap().entity_id());
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(CommandId::ShowWelcome, window, cx)
            })
        });
        draw(cx);
        assert!(cx.debug_bounds("welcome-page").is_some());
        // Back/Forward travels among pages and reuses account content.
        cx.simulate_keystrokes("secondary-alt-left");
        draw(cx);
        assert!(cx.debug_bounds("accounts-page").is_some());
        assert_eq!(
            cx.update(|_, app| viewer.read(app).account_page.as_ref().unwrap().entity_id()),
            accounts
        );
        cx.simulate_keystrokes("secondary-alt-left");
        draw(cx);
        cx.update(|_, app| {
            let viewer = viewer.read(app);
            assert!(viewer.page == super::pages::Page::Comparison);
            assert_eq!(viewer.selected_path(), path);
            assert!(std::rc::Rc::ptr_eq(
                viewer
                    .repository
                    .ready()
                    .unwrap()
                    .comparison
                    .ready()
                    .unwrap(),
                &comparison
            ));
            let code = viewer.code().unwrap();
            assert_eq!(code.before.entity_id(), before_editor.entity_id());
            assert_eq!(code.after.entity_id(), after_editor.entity_id());
            assert_eq!(standard.read(app).context_lines(), context);
            assert_eq!(standard.read(app).selected_text(app), text);
            assert_eq!(standard.read(app).viewport_offset(), offset);
            assert_eq!(viewer.fluid.position, fluid_position);
            assert_eq!(
                (
                    viewer.layout.saved.history_hidden,
                    viewer.layout.saved.files_hidden
                ),
                drawers
            );
        });
        cx.simulate_keystrokes("secondary-alt-right");
        draw(cx);
        assert!(cx.debug_bounds("accounts-page").is_some());
        let back = cx.debug_bounds("view-back").unwrap();
        cx.simulate_click(back.center(), Default::default());
        draw(cx);
        assert_eq!(
            cx.update(|_, app| standard.read(app).selected_text(app)),
            text
        );
    }
}

#[gpui_test::test]
fn welcome_accounts_navigation_and_palette_stay_in_the_existing_window(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    crate::account_ui::tests::setup(cx);
    let args = Arguments::parse_from(["jaywalk"]).for_welcome();
    let (viewer, cx) = setup_arguments(&fixture, args, cx);
    assert!(cx.debug_bounds("welcome-page").is_some());
    let window_count = cx.update(|_, app| app.windows().len());
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    cx.simulate_input("Accounts");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.debug_bounds("accounts-page").is_some());
    assert_eq!(cx.update(|_, app| app.windows().len()), window_count);
    // Escape from an idle page returns to Welcome rather than closing the app.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.debug_bounds("welcome-page").is_some());
    assert_eq!(cx.update(|_, app| app.windows().len()), window_count);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::GoForward, window, cx)
        })
    });
    draw(cx);
    assert!(cx.debug_bounds("accounts-page").is_some());
}

#[gpui_test::test]
fn leaving_a_loading_comparison_keeps_its_result_and_forward_page(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    crate::account_ui::tests::setup(cx);
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.load_repository(Some("file.txt".into()), window, cx);
            viewer.execute_command(CommandId::Accounts, window, cx);
        })
    });
    draw(cx);
    let editor = cx.update(|_, app| viewer.read(app).code().unwrap().before.entity_id());
    cx.simulate_keystrokes("secondary-alt-left");
    draw(cx);
    cx.update(|_, app| {
        let viewer = viewer.read(app);
        assert!(viewer.page == super::pages::Page::Comparison);
        assert_eq!(viewer.code().unwrap().before.entity_id(), editor);
    });
    cx.simulate_keystrokes("secondary-alt-right");
    draw(cx);
    assert!(cx.debug_bounds("accounts-page").is_some());
}

#[gpui_test::test]
fn pull_request_palette_preserves_the_view_until_selection(test_cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|_, app| {
        crate::hosting_ui::init(app);
        jaywalk::accounts::application::init_with_backend(
            crate::hosting_ui::tests::fake_service(),
            app,
        );
    });
    let original = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(jaywalk::commands::CommandId::OpenPullRequest, window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| matches!(viewer.read(app).page, super::pages::Page::Comparison)));
    assert!(cx.debug_bounds("palette-overlay").is_some());
    let picker = cx.debug_bounds("pr-palette-picker").unwrap();
    let search = cx.debug_bounds("command-search").unwrap();
    let context = cx.debug_bounds("pr-picker-context").unwrap();
    let results = cx.debug_bounds("command-results").unwrap();
    assert_eq!(search.top(), picker.top(), "nothing precedes search");
    assert!(search.bottom() <= context.top());
    assert!(context.bottom() <= results.top());
    assert!(cx.debug_bounds("pr-browser-controls").is_none());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
    assert!(cx.update(|_, app| matches!(viewer.read(app).page, super::pages::Page::Comparison)));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        original
    );

    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(jaywalk::commands::CommandId::ShowCommands, window, cx);
        })
    });
    draw(cx);
    cx.simulate_input("open pull request");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| matches!(viewer.read(app).page, super::pages::Page::Comparison)));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| matches!(viewer.read(app).page, super::pages::Page::Comparison)));
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
    assert!(cx.update(|_, app| viewer.read(app).comparison().unwrap().hosted().is_some()));
    assert!(cx.debug_bounds("pr-details").is_none());
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.navigate_view(false, window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| matches!(viewer.read(app).page, super::pages::Page::Comparison)));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        original
    );
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(jaywalk::commands::CommandId::BrowsePullRequests, window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| matches!(viewer.read(app).page, super::pages::Page::PullRequests)));
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn hosted_status_refresh_preserves_the_comparison_and_back_uses_its_memory_cache(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId;
    use std::sync::atomic::Ordering;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    let (service, counter) = crate::hosting_ui::tests::fake_service_with_status_counter();
    cx.update(|window, app| {
        crate::hosting_ui::init(app);
        jaywalk::accounts::application::init_with_backend(service, app);
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::OpenPullRequest, window, cx)
        });
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let original = cx.update(|_, app| {
        let viewer = viewer.read(app);
        assert!(viewer.is_hosted_review());
        assert!(viewer.pr_status.snapshot.is_some());
        viewer.comparison().unwrap().identity()
    });
    assert!(cx.debug_bounds("pr-status").is_some());
    let initial = counter.load(Ordering::SeqCst);
    assert!(initial > 0);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::RefreshReviewStatus, window, cx)
        })
    });
    draw(cx);
    let refreshed = counter.load(Ordering::SeqCst);
    assert!(refreshed > initial);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        original
    );
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::ShowWelcome, window, cx)
        })
    });
    draw(cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::GoBack, window, cx)
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        original
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        refreshed,
        "returning to a fresh retained snapshot must not refetch status"
    );
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::RefreshReviewStatus, window, cx);
            viewer.execute_command(CommandId::ShowWelcome, window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).page == super::pages::Page::Welcome));
    assert!(cx.update(|_, app| viewer.read(app).pr_status.control.is_none()));
}

#[gpui_test::test]
fn refresh_still_updates_the_hosted_review_while_a_commit_comparison_is_loading(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        crate::hosting_ui::init(app);
        jaywalk::accounts::application::init_with_backend(
            crate::hosting_ui::tests::fake_service(),
            app,
        );
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::OpenPullRequest, window, cx)
        });
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            let super::LoadState::Loaded(session) = &mut viewer.repository else {
                panic!()
            };
            assert!(session.home.hosted().is_some());
            session.comparison = super::LoadState::Loading;
            viewer.execute_command(CommandId::Refresh, window, cx);
        })
    });
    draw(cx);
    assert!(
        cx.update(|_, app| viewer
            .read(app)
            .comparison()
            .is_some_and(|comparison| comparison.hosted().is_some())),
        "Refresh must resolve the published PR even when its current commit is still loading"
    );
}

#[gpui_test::test]
fn hosted_overview_never_renders_the_previous_local_overview_under_its_pr_heading(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::{
        accounts::Control,
        commands::CommandId,
        hosting::{Locator, RepositoryName},
    };
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::Overview, window, cx)
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).capture_overview_identity().is_some()));
    let service = crate::hosting_ui::tests::fake_service();
    service.restore().unwrap();
    let key = service.accounts_snapshot().accounts[0].identity.key.clone();
    let locator = Locator {
        provider: jaywalk::accounts::Provider::Github,
        repository: RepositoryName::parse("team/widgets").unwrap(),
        number: 42,
    };
    let review = service
        .open_review(
            &key,
            &locator,
            Default::default(),
            Default::default(),
            &Control::new(std::time::Duration::from_secs(30)),
        )
        .unwrap();
    cx.update(|window, app| {
        jaywalk::accounts::application::init_with_backend(service, app);
        viewer.update(app, |viewer, cx| {
            viewer.open_hosted_review(review, window, cx)
        });
        // Render before parking: the old overview exists and replacement work is pending.
        window.draw(app).clear(app);
    });
    assert!(cx.debug_bounds("overview-preparing").is_some());
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).is_hosted_review()));
}

#[gpui_test::test]
fn git_whole_branch_return_keeps_unstaged_rename_after_commit_inspection(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let before = (0..40)
        .map(|n| format!("line {n:02}: retained source content\n"))
        .collect::<String>();
    fixture.replace(&before, &before);
    fixture.git(&["switch", "-c", "feature"]);
    let after = before.replace(
        "line 10: retained source content",
        "line 10: committed change",
    );
    std::fs::write(fixture.0.join("file.txt"), &after).unwrap();
    fixture.git(&["add", "file.txt"]);
    fixture.git(&["commit", "-m", "branch change"]);
    std::fs::rename(fixture.0.join("file.txt"), fixture.0.join("renamed.txt")).unwrap();
    let (viewer, cx) = setup(&fixture, cx);
    let commit = cx.update(|_, app| {
        let v = viewer.read(app);
        let c = v.comparison().unwrap();
        assert_eq!(c.workspace_label(), Some("Current work · based on main"));
        assert_eq!(v.selected_path().as_deref(), Some("renamed.txt"));
        assert_eq!(
            c.files()
                .iter()
                .find(|f| f.after_path() == "renamed.txt")
                .unwrap()
                .status(),
            "renamed"
        );
        c.recorded_tip().to_owned()
    });
    cx.update(|window, app| viewer.update(app, |v, cx| v.select_revision(&commit, window, cx)));
    draw(cx);
    cx.update(|_, app| assert_eq!(viewer.read(app).comparison().unwrap().tip(), commit));
    cx.update(|window, app| viewer.update(app, |v, cx| v.all_review_changes(None, window, cx)));
    draw(cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        let c = v.comparison().unwrap();
        assert!(matches!(
            v.repository.ready().unwrap().review.source,
            super::review_context::ReviewSource::Home(super::review_context::HomeSource::Workspace)
        ));
        assert_eq!(c.tip(), "working tree");
        assert_eq!(c.workspace_label(), Some("Current work · based on main"));
        assert!(
            c.files()
                .iter()
                .any(|f| f.after_path() == "renamed.txt" && f.status() == "renamed")
        );
    });
}

#[gpui_test::test]
fn carriage_returns_preserve_source_selection_in_all_diff_modes(cx: &mut TestAppContext) {
    use gpui_kit::component::diff::{DiffLineRange, DiffSide};
    use jaywalk::{diff_model::Algorithm, settings::DiffView};
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    for (before, after) in [
        ("\tone\roldValue\r尾部", "\tone\rnewValue\r尾部"),
        ("one\r\nold\r\n", "one\r\nnew\r\n"),
        ("漢字\rold\nlast\r", "漢字\rnew\nlast\r\n"),
        ("same\n", "same\r\n"),
    ] {
        fixture.replace(before, after);
        for algorithm in [Algorithm::Myers, Algorithm::Patience] {
            cx.update(|window, app| {
                viewer.update(app, |v, cx| {
                    v.settings.diff.algorithm = algorithm;
                    v.refresh(window, cx);
                })
            });
            draw(cx);
            for mode in [DiffView::Fluid, DiffView::SideBySide, DiffView::Inline] {
                cx.update(|_, app| viewer.update(app, |v, cx| v.set_view_mode(mode, cx)));
                draw(cx);
                let (old_editor, new_editor, standard) = cx.update(|_, app| {
                    let code = viewer
                        .read(app)
                        .code()
                        .expect("CR files load without panicking");
                    (
                        code.before.clone(),
                        code.after.clone(),
                        code.standard.as_ref().unwrap().as_ref().unwrap().clone(),
                    )
                });
                cx.update(|_, app| {
                    for (editor, source) in [(&old_editor, before), (&new_editor, after)] {
                        assert_eq!(editor.read(app).value().as_ref(), source);
                        editor.update(app, |editor, cx| {
                            editor.set_selected_range(0..source.len(), cx);
                            assert_eq!(editor.selected_text().to_string(), source);
                        });
                    }
                });
                cx.update(|_, app| {
                    standard.update(app, |state, cx| {
                        let path = state.files()[0].path().clone();
                        for (side, source) in
                            [(DiffSide::Original, before), (DiffSide::Modified, after)]
                        {
                            state.set_selected_lines(
                                Some(DiffLineRange::new(path.clone(), side, 1, usize::MAX)),
                                cx,
                            );
                            assert_eq!(
                                state.selected_text(cx),
                                source,
                                "{mode:?} / {algorithm:?}: source bytes changed"
                            );
                        }
                        state.set_selected_lines(None, cx);
                    })
                });
            }
        }
    }
}

#[gpui_test::test]
fn comparison_options_keep_each_sources_bytes_and_agree_in_every_view(cx: &mut TestAppContext) {
    use gpui_kit::component::diff::{DiffLineRange, DiffSide};
    use jaywalk::{commands::CommandId, settings::DiffView};
    let f = Fixture::new();
    let before = "let café = 1; \r\n\n\t \r\nlast\r\n";
    let after = "letcafé=1;\nlast\n";
    f.replace(before, after);
    let (viewer, cx) = setup(&f, cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    for command in [
        CommandId::WhitespaceAll,
        CommandId::ToggleIgnoreLineEndings,
        CommandId::ToggleIgnoreBlankLines,
    ] {
        cx.update(|w, app| viewer.update(app, |v, cx| v.execute_command(command, w, cx)));
        draw(cx);
    }
    for mode in [DiffView::Fluid, DiffView::SideBySide, DiffView::Inline] {
        cx.update(|_, app| viewer.update(app, |v, cx| v.set_view_mode(mode, cx)));
        draw(cx);
        let standard = cx.update(|_, app| {
            let v = viewer.read(app);
            assert_eq!(v.comparison().unwrap().identity(), identity);
            let code = v.code().unwrap();
            assert!(code.model.blocks.iter().all(|b| !b.changed));
            assert_eq!(code.before.read(app).value().as_ref(), before);
            assert_eq!(code.after.read(app).value().as_ref(), after);
            code.standard.as_ref().unwrap().as_ref().unwrap().clone()
        });
        cx.update(|_, app| {
            standard.update(app, |state, cx| {
                assert_eq!(state.change_navigation_position().1, 0);
                let path = state.files()[0].path().clone();
                for (side, text) in [(DiffSide::Original, before), (DiffSide::Modified, after)] {
                    state.set_selected_lines(
                        Some(DiffLineRange::new(path.clone(), side, 1, usize::MAX)),
                        cx,
                    );
                    assert_eq!(state.selected_text(cx), text);
                }
                state.set_selected_lines(None, cx);
            })
        });
    }
    let old_model = cx.update(|_, app| viewer.read(app).code().unwrap().model.clone());
    for command in [
        CommandId::ToggleShowWhitespace,
        CommandId::ToggleShowLineEndings,
        CommandId::ToggleWhitespaceErrors,
        CommandId::ToggleWordHighlight,
    ] {
        cx.update(|w, app| viewer.update(app, |v, cx| v.execute_command(command, w, cx)));
        draw(cx);
        cx.update(|_, app| {
            assert!(std::rc::Rc::ptr_eq(
                &old_model,
                &viewer.read(app).code().unwrap().model
            ))
        });
    }
    cx.update(|_, app| {
        let v = viewer.read(app);
        assert!(
            v.diff_preferences.show_whitespace
                && v.diff_preferences.show_line_endings
                && v.diff_preferences.highlight_whitespace_errors
        );
        assert!(!v.diff_preferences.word_highlight);
    });
    cx.update(|w, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::WhitespaceExact, w, cx)
        })
    });
    draw(cx);
    cx.update(|_, app| {
        assert!(
            viewer
                .read(app)
                .code()
                .unwrap()
                .model
                .blocks
                .iter()
                .any(|b| b.changed)
        )
    });
}

#[gpui_test::test]
fn diff_config_changes_replace_only_edited_defaults_and_commands_remember_the_ignore_mode(
    cx: &mut TestAppContext,
) {
    use jaywalk::{commands::CommandId, diff_options::Whitespace};
    let f = Fixture::new();
    let (viewer, cx) = setup(&f, cx);
    cx.update(|w, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::WhitespaceTrailing, w, cx);
            v.execute_command(CommandId::ToggleWhitespaceDifferences, w, cx);
            assert_eq!(v.diff_preferences.comparison.whitespace, Whitespace::Exact);
            v.execute_command(CommandId::ToggleWhitespaceDifferences, w, cx);
            assert_eq!(
                v.diff_preferences.comparison.whitespace,
                Whitespace::IgnoreTrailing
            );
            v.execute_command(CommandId::ToggleShowWhitespace, w, cx);
        })
    });
    draw(cx);
    cx.update(|w, app| {
        viewer.update(app, |v, cx| {
            let mut settings = v.settings.clone();
            settings.ui.font_size += 1.;
            v.apply_settings(settings, w, cx);
            assert!(v.diff_preferences.show_whitespace);
            assert_eq!(
                v.diff_preferences.comparison.whitespace,
                Whitespace::IgnoreTrailing
            );
            let mut settings = v.settings.clone();
            settings.diff.whitespace = Whitespace::IgnoreAll;
            settings.diff.show_line_endings = true;
            v.apply_settings(settings, w, cx);
            assert_eq!(
                v.diff_preferences.comparison.whitespace,
                Whitespace::IgnoreAll
            );
            assert!(v.diff_preferences.show_whitespace && v.diff_preferences.show_line_endings);
        })
    });
    draw(cx);
}

#[gpui_test::test]
fn unified_picker_prefixes_help_and_focus_survive_mode_changes(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId as C;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|_, app| {
        crate::hosting_ui::init(app);
        jaywalk::accounts::application::init_with_backend(
            crate::hosting_ui::tests::fake_service(),
            app,
        );
    });
    let original = cx.update(|window, app| {
        let focus = viewer.read(app).code().unwrap().before.focus_handle(app);
        window.focus(&focus, app);
        focus
    });
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::ShowCommands,
    });
    draw(cx);
    for (query, kind) in [
        ("~pro", palette::PaletteKind::Repositories),
        ("?commands", palette::PaletteKind::Help),
        ("@symbol", palette::PaletteKind::FileSymbols),
        ("#symbol", palette::PaletteKind::ProjectSymbols),
        ("file.txt", palette::PaletteKind::Files),
        ("!fix", palette::PaletteKind::PullRequests),
        (">word wrap", palette::PaletteKind::Commands),
    ] {
        cx.update(|window, app| {
            viewer.update(app, |v, cx| v.palette_test_set_query(query, window, cx))
        });
        draw(cx);
        assert_eq!(
            cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
            Some(kind),
            "query {query}"
        );
        assert_eq!(
            cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
            identity
        );
    }
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.palette_test_set_query("?commands", window, cx)
        })
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        ">"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|window, _| original.is_focused(window)));
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn command_usage_is_shared_but_does_not_reorder_an_open_picker(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId as C;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        crate::command_usage::record(C::ToggleWordHighlight, app);
        viewer.update(app, |v, cx| v.execute_command(C::ShowCommands, window, cx));
    });
    draw(cx);
    // Another window can record usage without changing the open picker's ordering.
    cx.update(|_, app| {
        for _ in 0..10 {
            crate::command_usage::record(C::ToggleShowWhitespace, app);
        }
        app.refresh_windows();
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| !viewer.read(app).diff_preferences.word_highlight));
    assert!(cx.update(|_, app| !viewer.read(app).diff_preferences.show_whitespace));
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::ShowCommands,
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).diff_preferences.show_whitespace));
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::ClearCommandUsage,
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| app
            .global::<crate::command_usage::Usage>()
            .history
            .score(C::ToggleShowWhitespace)),
        0
    );
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.settings.picker.adaptive_command_order = false;
            v.execute_command(C::ShowCommands, window, cx);
            v.palette_test_set_query(">Toggle word-level highlighting", window, cx);
        })
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| app
            .global::<crate::command_usage::Usage>()
            .history
            .score(C::ToggleWordHighlight)),
        0
    );
}

#[gpui_test::test]
fn line_picker_uses_focused_source_side_and_reveals_folded_context(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId as C;
    use jaywalk::settings::DiffView;
    let fixture = Fixture::new();
    let source = (0..150)
        .map(|i| format!("source line {i}\n"))
        .collect::<String>();
    fixture.replace(&source, &source.replace("line 80", "modified line 80"));
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        let focus = viewer.read(app).code().unwrap().before.focus_handle(app);
        window.focus(&focus, app);
        viewer.update(app, |v, cx| v.execute_command(C::GoToLine, window, cx));
    });
    draw(cx);
    cx.simulate_input("100");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .code()
            .unwrap()
            .before
            .read(app)
            .cursor_position()
            .line),
        99
    );
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.set_view_mode(DiffView::SideBySide, cx);
            v.collapse_unchanged = true;
            v.apply_standard_context(cx);
            v.execute_command(C::GoToLine, window, cx);
            v.palette_test_set_query(":1", window, cx);
        })
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let position = cx.update(|_, app| {
        viewer
            .read(app)
            .code()
            .unwrap()
            .standard
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .read(app)
            .viewport_position()
            .unwrap()
    });
    assert!(position.line() <= 3);
    assert!(cx.debug_bounds("diff-expand-0-0").is_none());
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            window.focus(&v.focus, cx);
            let state = v
                .code()
                .unwrap()
                .standard
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            state.update(cx, |state, cx| {
                let path = state.files()[0].path().clone();
                state.set_selected_lines(
                    Some(gpui_kit::component::diff::DiffLineRange::new(
                        path,
                        gpui_kit::component::diff::DiffSide::Original,
                        1,
                        1,
                    )),
                    cx,
                );
            });
            v.execute_command(C::GoToLine, window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_line_before()));
}

#[gpui_test::test]
fn welcome_picker_explains_unavailable_modes_without_leaving_welcome(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId as C;
    let fixture = Fixture::new();
    let args = Arguments::parse_from(["jaywalk"]);
    let (viewer, cx) = setup_arguments(&fixture, args, test_cx);
    for command in [C::FindFile, C::GoToLine, C::PickerHelp] {
        cx.dispatch_action(super::commands::ExecuteCommand { command });
        draw(cx);
        assert!(cx.debug_bounds("palette-overlay").is_some());
        assert!(cx.debug_bounds("welcome-page").is_some());
        cx.simulate_keystrokes("escape");
        draw(cx);
    }
    assert!(cx.update(|_, app| viewer.read(app).repository.ready().is_none()));
}

#[gpui_test::test]
fn dismissing_a_cold_branch_picker_rejects_its_late_results(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId as C;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(C::ChooseReview, window, cx);
            assert!(
                v.review_picker_visible(),
                "cold chooser must open before its worker completes"
            );
            v.close_palette(window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        identity
    );
}

#[gpui_test::test]
fn account_picker_confirms_disconnect_and_preserves_the_view(test_cx: &mut TestAppContext) {
    use jaywalk::{accounts::application::Accounts, commands::CommandId as C};
    crate::account_ui::tests::setup(test_cx);
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::DisconnectAccount,
    });
    draw(cx);
    assert!(cx.debug_bounds("account-palette-picker").is_some());
    assert!(cx.debug_bounds("accounts-page").is_none());
    cx.simulate_input("synthetic-user");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| app
            .global::<Accounts>()
            .state
            .read(app)
            .snapshot
            .accounts
            .len()),
        1
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        "synthetic-user"
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("confirm local");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| {
        app.global::<Accounts>()
            .state
            .read(app)
            .snapshot
            .accounts
            .is_empty()
    }));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        identity
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn account_picker_connection_errors_and_mode_switches_stay_in_the_picker(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::{accounts::application::Accounts, commands::CommandId as C};
    crate::account_ui::tests::setup(test_cx);
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::ConnectAccount,
    });
    draw(cx);
    cx.simulate_input("github");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    // The synthetic store has no client registration; no browser or OS store is touched.
    assert!(cx.update(|_, app| {
        app.global::<Accounts>()
            .state
            .read(app)
            .notice
            .as_ref()
            .unwrap()
            .error
    }));
    assert!(cx.debug_bounds("account-palette-picker").is_some());
    assert!(cx.debug_bounds("accounts-page").is_none());
    cx.update(|window, app| viewer.update(app, |v, cx| v.palette_test_set_query("?", window, cx)));
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Help)
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::Accounts,
    });
    draw(cx);
    assert!(cx.debug_bounds("accounts-page").is_some());
}

#[gpui_test::test]
fn manage_account_picker_retains_parent_query_after_connection_check(test_cx: &mut TestAppContext) {
    use jaywalk::{
        accounts::{ConnectionState, application::Accounts},
        commands::CommandId as C,
    };
    crate::account_ui::tests::setup(test_cx);
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::ManageAccounts,
    });
    draw(cx);
    cx.simulate_input("synthetic-user");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("check");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| matches!(
        app.global::<Accounts>().state.read(app).snapshot.accounts[0].state,
        ConnectionState::Connected
    )));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        "synthetic-user"
    );
    cx.update(|window, app| viewer.update(app, |v, cx| v.palette_test_set_query("~", window, cx)));
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Repositories)
    );
}

#[gpui_test::test]
fn dismissing_an_account_picker_does_not_cancel_another_owners_operation(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::{
        accounts::{application::Accounts, controller::AccountAction},
        commands::CommandId as C,
    };
    crate::account_ui::tests::setup(test_cx);
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    let owner = cx.update(|window, app| {
        let state = app.global::<Accounts>().state.clone();
        let owner = state
            .update(app, |state, cx| state.start(AccountAction::Load, cx))
            .unwrap();
        viewer.update(app, |v, cx| {
            v.execute_command(C::ManageAccounts, window, cx);
            v.close_palette(window, cx);
        });
        owner
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| app
            .global::<Accounts>()
            .state
            .read(app)
            .last_finished
            .unwrap()
            .id),
        owner.id
    );
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn manage_accounts_disconnect_returns_to_the_valid_parent_and_keeps_its_query(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::{accounts::application::Accounts, commands::CommandId as C};
    crate::account_ui::tests::setup(test_cx);
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.dispatch_action(super::commands::ExecuteCommand {
        command: C::ManageAccounts,
    });
    draw(cx);
    cx.simulate_input("synthetic-user");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("disconnect");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("confirm local");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| {
        app.global::<Accounts>()
            .state
            .read(app)
            .snapshot
            .accounts
            .is_empty()
    }));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        "synthetic-user"
    );
    assert!(cx.debug_bounds("account-palette-picker").is_some());
    // Home is a valid empty filtered list, and a single Escape dismisses it.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn project_symbols_open_unchanged_source_and_file_symbols_use_the_before_side(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId as C;
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("stable.rs"),
        "// fixture\n\nfn stable_target() {}\n",
    )
    .unwrap();
    std::fs::write(
        fixture.0.join("change.rs"),
        "// before\nfn old_target() {}\n",
    )
    .unwrap();
    fixture.git(&["add", "stable.rs", "change.rs"]);
    fixture.git(&["commit", "-m", "source fixtures"]);
    std::fs::write(
        fixture.0.join("change.rs"),
        "// after\nfn new_target() {}\n",
    )
    .unwrap();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(C::GoToProjectSymbol, window, cx);
            v.palette_test_set_query("#stable_target", window, cx);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| {
        viewer
            .read(app)
            .symbol_picker_description()
            .contains("symbols")
    }));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).selected_path()),
        Some("stable.rs".into())
    );
    // A project jump uses the original source row even in an unchanged file.
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .code()
            .unwrap()
            .standard
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .read(app)
            .selected_lines()
            .unwrap()
            .start()),
        3
    );
    cx.update(|window, app| viewer.update(app, |v, cx| v.select_path("change.rs", window, cx)));
    draw(cx);
    cx.update(|window, app| {
        let focus = viewer.read(app).code().unwrap().before.focus_handle(app);
        window.focus(&focus, app);
        viewer.update(app, |v, cx| {
            v.execute_command(C::GoToFileSymbol, window, cx);
            v.palette_test_set_query("@old_target", window, cx);
        });
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .code()
            .unwrap()
            .before
            .read(app)
            .cursor_position()
            .line),
        1
    );
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn symbol_search_cancellation_cannot_replace_a_new_picker_mode(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId as C;
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("symbols.rs"), "fn target() {}\n").unwrap();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(C::GoToProjectSymbol, window, cx);
            v.palette_test_set_query(">refresh", window, cx);
            // Deliver the query callback before yielding to the index worker.
            v.palette_query_changed(">refresh", window, cx);
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Commands)
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn local_pr_action_uses_the_picker_and_opens_a_full_pinned_tree_headlessly(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::{
        accounts::Control,
        commands::CommandId as C,
        hosting::{Locator, RepositoryName},
    };
    let fixture = Fixture::new();
    fixture.git(&["remote", "add", "origin", "git@github.com:team/widgets.git"]);
    let base = jaywalk::repository::Comparison::load(&fixture.0, Some("HEAD"), "HEAD")
        .unwrap()
        .tip()
        .to_owned();
    fixture.git(&["commit", "--allow-empty", "-m", "review head"]);
    let head = jaywalk::repository::Comparison::load(&fixture.0, Some("HEAD"), "HEAD")
        .unwrap()
        .tip()
        .to_owned();
    let service = crate::hosting_ui::tests::fake_service_with_review_commits(base, head);
    service.restore().unwrap();
    let key = service.accounts_snapshot().accounts[0].identity.key.clone();
    let review = service
        .open_review(
            &key,
            &Locator {
                provider: jaywalk::accounts::Provider::Github,
                repository: RepositoryName::parse("team/widgets").unwrap(),
                number: 42,
            },
            Default::default(),
            Default::default(),
            &Control::new(std::time::Duration::from_secs(30)),
        )
        .unwrap();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        jaywalk::accounts::application::init_with_backend(service, app);
        viewer.update(app, |v, cx| {
            v.open_hosted_review(review, window, cx);
            v.execute_command(C::ReviewPRLocally, window, cx);
        });
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::LocalRepositories)
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| {
        viewer
            .read(app)
            .comparison()
            .unwrap()
            .hosted()
            .unwrap()
            .is_local()
    }));
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.set_tree_scope(jaywalk::settings::TreeScope::All, cx);
            v.select_path("file.txt", window, cx);
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .code()
            .unwrap()
            .after
            .read(app)
            .value()
            .to_string()),
        "original\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "changed\n"
    );
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(C::RestoreReviewCheckout, window, cx)
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).local_actions.recoveries.len()),
        0
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
}

#[gpui_test::test]
fn local_pr_checkout_refusal_preserves_the_view_and_all_dirty_files(test_cx: &mut TestAppContext) {
    use jaywalk::{
        accounts::Control,
        commands::CommandId as C,
        hosting::{Locator, RepositoryName},
    };
    let fixture = Fixture::new();
    fixture.git(&["remote", "add", "origin", "git@github.com:team/widgets.git"]);
    let head = std::fs::read(fixture.0.join(".git/HEAD")).unwrap();
    let index = std::fs::read(fixture.0.join(".git/index")).unwrap();
    let service = crate::hosting_ui::tests::fake_service();
    service.restore().unwrap();
    let key = service.accounts_snapshot().accounts[0].identity.key.clone();
    let review = service
        .open_review(
            &key,
            &Locator {
                provider: jaywalk::accounts::Provider::Github,
                repository: RepositoryName::parse("team/widgets").unwrap(),
                number: 42,
            },
            Default::default(),
            Default::default(),
            &Control::new(std::time::Duration::from_secs(30)),
        )
        .unwrap();
    let identity = review.comparison().identity();
    let (viewer, cx) = setup(&fixture, test_cx);
    cx.update(|window, app| {
        jaywalk::accounts::application::init_with_backend(service, app);
        viewer.update(app, |v, cx| {
            v.settings.local_review.mode = jaywalk::local_review::Mode::CheckoutIfClean;
            v.open_hosted_review(review, window, cx);
            v.execute_command(C::ReviewPRLocally, window, cx);
        });
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        identity
    );
    assert!(cx.update(|_, app| {
        viewer
            .read(app)
            .local_actions
            .message
            .as_ref()
            .unwrap()
            .contains("refused")
    }));
    assert_eq!(std::fs::read(fixture.0.join(".git/HEAD")).unwrap(), head);
    assert_eq!(std::fs::read(fixture.0.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "changed\n"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
}

#[gpui_test::test]
fn account_metadata_is_added_lazily_and_config_order_reloads_across_the_app(
    test_cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let backend = crate::hosting_ui::tests::fake_service();
    test_cx.update(|app| {
        jaywalk::accounts::application::init_with_backend(backend.clone(), app);
    });
    let (viewer, cx) = setup(&fixture, test_cx);
    let path = fixture.0.join("settings.toml");
    assert!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .accounts
            .is_empty()
    );
    assert!(
        !backend.accounts_snapshot().loaded,
        "a local diff must not load credentials"
    );
    jaywalk::settings_reload::SettingsSource::resolve(Some(&path))
        .unwrap()
        .initialize()
        .unwrap();
    backend.restore().unwrap();
    for _ in 0..3 {
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(cx);
    }
    let contents = std::fs::read_to_string(&path).unwrap();
    let config = Settings::parse(&contents).unwrap();
    assert_eq!(config.accounts.len(), 2);
    assert!(
        config
            .accounts
            .iter()
            .all(|account| account.client_id.as_deref() == Some("synthetic-client"))
    );
    // Simulate an editor moving complete account sections in the file.
    let mut sections = contents.split("[[accounts]]");
    let mut edited = sections.next().unwrap().to_owned();
    for section in sections.collect::<Vec<_>>().into_iter().rev() {
        edited.push_str("[[accounts]]");
        edited.push_str(&section.replace("name = \"person2\"", "name = \"Work\""));
    }
    std::fs::write(&path, &edited).unwrap();
    assert_eq!(Settings::parse(&edited).unwrap().accounts[0].id, "2");
    for _ in 0..3 {
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(cx);
    }
    let accounts = backend.accounts_snapshot().accounts;
    assert_eq!(accounts[0].identity.key.subject, "2");
    assert_eq!(accounts[0].label(), "Work");
    assert_eq!(accounts[0].identity.label, "person2");
    assert_eq!(
        cx.update(|_, app| viewer.read(app).settings.accounts[0].id.clone()),
        "2"
    );
}

fn reference_picker_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.git(&["checkout", "-b", "topic"]);
    fixture.git(&["add", "file.txt"]);
    fixture.git(&["commit", "-m", "topic change"]);
    let topic = std::fs::read_to_string(fixture.0.join(".git/refs/heads/topic")).unwrap();
    fixture.git(&["checkout", "main"]);
    fixture.git(&[
        "config",
        "remote.origin.url",
        "/nonexistent/synthetic-remote",
    ]);
    fixture.git(&["update-ref", "refs/remotes/origin/review", topic.trim()]);
    fixture
}

#[gpui_test::test]
fn remote_palette_toggles_keep_query_selection_and_comparison(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = reference_picker_fixture();
    let (viewer, cx) = setup(&fixture, cx);
    let original = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    cx.simulate_input("Remote history sources");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Remotes)
    );
    cx.simulate_input("origin");
    draw(cx);
    for expected in [false, true] {
        cx.simulate_keystrokes("enter");
        draw(cx);
        cx.update(|_, app| {
            let v = viewer.read(app);
            assert_eq!(
                v.palette_test_state(app),
                (
                    Some(palette::PaletteKind::Remotes),
                    "origin".into(),
                    Some(0)
                )
            );
            assert_eq!(v.references.visibility.includes("origin"), expected);
            assert_eq!(v.comparison().unwrap().identity(), original);
            assert_eq!(v.selected_path().as_deref(), Some("file.txt"));
        });
    }
    // Scoped review bindings do nothing in the remote picker.
    cx.simulate_keystrokes("shift-enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Remotes)
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::HideAllRemotes, window, cx)
        })
    });
    draw(cx);
    assert!(!cx.update(|_, app| viewer.read(app).references.visibility.includes("origin")));
}

#[gpui_test::test]
fn branch_palette_opens_hidden_remote_targets_and_reviews_local_branches(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = reference_picker_fixture();
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::HideAllRemotes, window, cx);
            v.execute_command(CommandId::GoToReference, window, cx);
        })
    });
    draw(cx);
    let original = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.simulate_input("origin");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        original
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        assert!(v.palette_test_state(app).0.is_none());
        assert_eq!(
            v.references.inspected.as_ref().unwrap().remote(),
            Some("origin")
        );
        assert!(!v.references.visibility.includes("origin"));
        assert_eq!(
            v.comparison().unwrap().recorded_tip(),
            std::fs::read_to_string(fixture.0.join(".git/refs/heads/topic"))
                .unwrap()
                .trim()
        );
    });
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::GoToReference, window, cx)
        })
    });
    draw(cx);
    cx.simulate_input("topic");
    draw(cx);
    cx.simulate_keystrokes("shift-enter");
    draw(cx);
    if cx.update(|_, app| viewer.read(app).palette_test_state(app).0)
        == Some(palette::PaletteKind::Reviews)
    {
        cx.simulate_input("main");
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
    }
    assert!(cx.update(|_, app| matches!(
        &viewer.read(app).repository.ready().unwrap().review.source,
        super::review_context::ReviewSource::Target(jaywalk::branch_review::Target::Branch { .. })
    )));
    // Neither opening nor reviewing a reference checks out its branch.
    assert_eq!(
        std::fs::read_to_string(fixture.0.join(".git/HEAD"))
            .unwrap()
            .trim(),
        "ref: refs/heads/main"
    );
}

#[gpui_test::test]
fn floating_outlines_preserve_editor_focus_and_panel_geometry(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (before, after) = cx.update(|_, app| {
        let code = viewer.read(app).code().unwrap();
        (code.before.clone(), code.after.clone())
    });
    let dimensions = cx.update(|_, app| {
        (
            before.read(app).input_bounds(),
            after.read(app).input_bounds(),
        )
    });
    let header = cx.debug_bounds("file-header").unwrap();
    let gutter = cx.debug_bounds("fluid-gutter").unwrap();
    cx.simulate_click(dimensions.0.center(), Default::default());
    draw(cx);
    assert!(cx.update(|window, app| before.focus_handle(app).is_focused(window)));
    for enabled in [false, true] {
        cx.update(|_, app| {
            let mut settings = viewer.read(app).settings.clone();
            settings.ui.accent_floating_outlines = enabled;
            crate::appearance::apply(&settings, app);
            viewer.update(app, |v, cx| {
                v.settings = settings;
                cx.notify();
            });
        });
        draw(cx);
        assert_eq!(
            cx.update(|_, app| (
                before.read(app).input_bounds(),
                after.read(app).input_bounds()
            )),
            dimensions
        );
        assert_eq!(cx.debug_bounds("file-header").unwrap(), header);
        assert_eq!(cx.debug_bounds("fluid-gutter").unwrap(), gutter);
        assert!(cx.update(|window, app| before.focus_handle(app).is_focused(window)));
    }
}

fn paged_history_fixture() -> (Fixture, String, String) {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join(".git/info/exclude"),
        "settings.toml\nkeybindings/\n",
    )
    .unwrap();
    let mut oldest = String::new();
    let mut newest = String::new();
    for index in 0..14 {
        std::fs::write(fixture.0.join("file.txt"), format!("revision {index}\n")).unwrap();
        fixture.git(&["add", "file.txt"]);
        let message = if index == 0 {
            "older café needle".into()
        } else if index == 13 {
            "newest unique result".into()
        } else {
            format!("ordinary revision {index}")
        };
        fixture.git(&["commit", "-m", &message]);
        newest = std::fs::read_to_string(fixture.0.join(".git/refs/heads/main"))
            .unwrap()
            .trim()
            .to_owned();
        if index == 0 {
            oldest = newest.clone();
        }
    }
    std::fs::write(fixture.0.join("file.txt"), "working edit\n").unwrap();
    (fixture, oldest, newest)
}
fn setup_paged<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Viewer>, &'a mut VisualTestContext) {
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    setup_arguments_with_settings(
        fixture,
        args,
        "[history]\npage_size=4\nsearch_page_size=2\npage_on_scroll=false\n",
        cx,
    )
}
#[gpui_test::test]
fn opening_history_fills_a_small_configured_batch_without_scroll_paging(cx: &mut TestAppContext) {
    let (fixture, _, _) = paged_history_fixture();
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    let (viewer, cx) = setup_arguments_with_settings(
        &fixture,
        args,
        "[history]\npage_size=12\ninitial_batch_size=3\npage_on_scroll=false\n",
        cx,
    );
    cx.update(|_, app| {
        let v = viewer.read(app);
        let history = v.repository.ready().unwrap().history.as_ref().unwrap();
        assert_eq!(history.revisions.len(), 12);
        assert!(history.has_more());
        assert_eq!(v.history_query.limit(), 12);
        assert!(!v.references.loading_more);
        assert!(!v.settings.history.page_on_scroll);
        let ids = history
            .revisions
            .iter()
            .map(|r| r.commit_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), history.revisions.len());
        assert_eq!(v.selected_path().as_deref(), Some("file.txt"));
    });
}

#[gpui_test::test]
fn initial_history_fill_keeps_the_first_batch_usable_and_respects_cancellation(
    cx: &mut TestAppContext,
) {
    let (fixture, _, _) = paged_history_fixture();
    let (viewer, cx) = setup_paged(&fixture, cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    let first = cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            let first = v
                .repository
                .ready()
                .unwrap()
                .history
                .as_ref()
                .unwrap()
                .clone();
            v.settings.history.page_size = 12;
            v.fill_initial_history(window, cx);
            assert!(v.references.loading_more);
            assert!(std::sync::Arc::ptr_eq(
                &first,
                v.repository.ready().unwrap().history.as_ref().unwrap()
            ));
            assert_eq!(first.revisions.len(), 4);
            assert_eq!(v.comparison().unwrap().identity(), identity);
            first
        })
    });
    draw(cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        let filled = v.repository.ready().unwrap().history.as_ref().unwrap();
        assert_eq!(filled.revisions.len(), 12);
        assert!(!v.references.loading_more);
        assert_eq!(v.comparison().unwrap().identity(), identity);
        assert_eq!(v.selected_path().as_deref(), Some("file.txt"));
        for (old, new) in first.revisions.iter().zip(&filled.revisions) {
            assert!(std::sync::Arc::ptr_eq(old, new));
        }
    });
    // A source/query change cancels filling; its completion must not replace
    // the newly installed graph, even if the worker was already scheduled.
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            if let super::LoadState::Loaded(session) = &mut v.repository {
                session.history = Ok(first.clone());
            }
            v.fill_initial_history(window, cx);
            v.invalidate_history_reload();
            assert!(!v.references.loading_more);
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| {
        std::sync::Arc::ptr_eq(
            &first,
            viewer
                .read(app)
                .repository
                .ready()
                .unwrap()
                .history
                .as_ref()
                .unwrap(),
        )
    }));
}

#[gpui_test::test]
fn history_pages_preserve_inspection_and_retry_then_support_automatic_loading(
    cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId;
    let (fixture, _, _) = paged_history_fixture();
    let (viewer, cx) = setup_paged(&fixture, cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    let initial = cx.update(|_, app| {
        viewer
            .read(app)
            .repository
            .ready()
            .unwrap()
            .history
            .as_ref()
            .unwrap()
            .clone()
    });
    assert_eq!(initial.revisions.len(), 4);
    assert!(initial.has_more());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .repository
            .ready()
            .unwrap()
            .history
            .as_ref()
            .unwrap()
            .revisions
            .len()),
        4
    );
    // A failed page retains the exact old snapshot and offers retry.
    std::fs::rename(
        fixture.0.join(".git/objects"),
        fixture.0.join(".git/saved-objects"),
    )
    .unwrap();
    let row = cx.debug_bounds("history-load-more").unwrap();
    cx.simulate_click(row.center(), Default::default());
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).references.error.is_some()));
    assert!(cx.update(|_, app| {
        std::sync::Arc::ptr_eq(
            viewer
                .read(app)
                .repository
                .ready()
                .unwrap()
                .history
                .as_ref()
                .unwrap(),
            &initial,
        )
    }));
    std::fs::rename(
        fixture.0.join(".git/saved-objects"),
        fixture.0.join(".git/objects"),
    )
    .unwrap();
    let row = cx.debug_bounds("history-load-more").unwrap();
    cx.simulate_click(row.center(), Default::default());
    draw(cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        assert!(v.references.error.is_none());
        assert_eq!(
            v.repository
                .ready()
                .unwrap()
                .history
                .as_ref()
                .unwrap()
                .revisions
                .len(),
            8
        );
        assert_eq!(v.comparison().unwrap().identity(), identity);
        assert_eq!(v.selected_path().as_deref(), Some("file.txt"));
    });
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::LatestHistory, window, cx);
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer
            .read(app)
            .repository
            .ready()
            .unwrap()
            .history
            .as_ref()
            .unwrap()
            .revisions
            .len()),
        4
    );
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            let mut settings = v.settings.clone();
            settings.history.page_on_scroll = true;
            v.apply_settings(settings, window, cx);
        })
    });
    draw(cx);
    assert!(
        cx.update(|_, app| viewer
            .read(app)
            .repository
            .ready()
            .unwrap()
            .history
            .as_ref()
            .unwrap()
            .revisions
            .len())
            > 4
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        identity
    );
}
#[gpui_test::test]
fn slash_history_search_finds_older_commits_and_keeps_repository_scope_after_reveal(
    cx: &mut TestAppContext,
) {
    let (fixture, oldest, newest) = paged_history_fixture();
    let (viewer, cx) = setup_paged(&fixture, cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.simulate_keystrokes("secondary-shift-p");
    draw(cx);
    cx.simulate_input("/needle");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::History)
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        1
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        identity
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        assert_eq!(
            v.selected_history_commit().as_deref(),
            Some(oldest.as_str())
        );
        let history = v.repository.ready().unwrap().history.as_ref().unwrap();
        assert_eq!(history.revisions[0].commit_id, oldest);
        assert!(history.source().unwrap().context().is_some());
    });
    // The graph now shows an older context; / must still search its original source.
    cx.simulate_keystrokes("secondary-shift-p");
    draw(cx);
    cx.simulate_input("/newest unique");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        1
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).selected_history_commit())
            .as_deref(),
        Some(newest.as_str())
    );
    let row = cx.debug_bounds("history-row-0").unwrap();
    assert!(row.size.height > px(0.));
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.execute_command(jaywalk::commands::CommandId::SearchHistory, window, cx)
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        3
    ); // two results plus Load more
    // Cached context results can keep selection by key when the repository
    // results insert an earlier row. Navigate to the explicit Load more row.
    for _ in 0..3 {
        if cx.update(|_, app| viewer.read(app).palette_test_state(app).2) == Some(2) {
            break;
        }
        cx.simulate_keystrokes("down");
        draw(cx);
    }
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).2),
        Some(2)
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        5
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        "/"
    );
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("/after:invalid");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        0
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
}

#[gpui_test::test]
fn welcome_uses_two_columns_without_scrolling_and_stacks_on_small_windows(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|window, app| {
        app.global_mut::<crate::recents::Recents>().repositories = (0..10)
            .map(|index| {
                PathBuf::from(format!(
                    "/a/long/parent/path/with/many/components/repository-{index}"
                ))
            })
            .collect();
        viewer.update(app, |v, cx| {
            v.execute_command(CommandId::ShowWelcome, window, cx)
        });
    });
    draw(cx);
    assert!(cx.debug_bounds("welcome-file-shortcut").is_some());
    let page = cx.debug_bounds("welcome-page").unwrap();
    let content = cx.debug_bounds("welcome-content").unwrap();
    let header = cx.debug_bounds("welcome-header").unwrap();
    let start = cx.debug_bounds("welcome-start").unwrap();
    let recents = cx.debug_bounds("welcome-recents").unwrap();
    assert!(header.bottom() < start.top());
    assert_eq!(start.top(), recents.top());
    assert!(start.right() < recents.left());
    assert!(
        content.bottom() <= page.bottom(),
        "default welcome content should fit: {content:?} in {page:?}"
    );
    assert!(cx.debug_bounds("welcome-recent-9").unwrap().bottom() < page.bottom());
    cx.simulate_resize(size(px(560.), px(800.)));
    draw(cx);
    let page = cx.debug_bounds("welcome-page").unwrap();
    let content = cx.debug_bounds("welcome-content").unwrap();
    let start = cx.debug_bounds("welcome-start").unwrap();
    let recents = cx.debug_bounds("welcome-recents").unwrap();
    assert_eq!(start.left(), recents.left());
    assert!(start.bottom() < recents.top());
    assert!(content.right() <= page.right());
    cx.simulate_resize(size(px(1000.), px(400.)));
    draw(cx);
    assert!(
        cx.debug_bounds("welcome-content").unwrap().bottom()
            > cx.debug_bounds("welcome-page").unwrap().bottom()
    );
}

#[gpui_test::test]
fn opening_a_large_new_file_keeps_its_first_line_visible(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    fixture.git(&["rm", "-f", "file.txt"]);
    fixture.git(&["commit", "-m", "remove fixture file"]);
    let source = (1..=200)
        .map(|line| format!("new line {line}\n"))
        .collect::<String>();
    std::fs::write(fixture.0.join("file.txt"), source).unwrap();
    let (viewer, cx) = setup(&fixture, cx);
    assert!(cx.update(|_, app| viewer.read(app).code().is_some()));
    cx.update(|_, app| {
        let viewer = viewer.read(app);
        let code = viewer.code().unwrap();
        assert_eq!(viewer.fluid.position, 0.);
        assert_eq!(code.after.read(app).scroll_offset().y, px(0.));
        assert_eq!(code.after.read(app).visible_row_range().unwrap().start, 0);
    });
}

#[gpui_test::test]
fn difftool_and_normal_opens_share_remembered_controls_without_replacing_supplied_bytes(
    test_cx: &mut TestAppContext,
) {
    use jaywalk::{
        difftool::{Capture, File, Identity},
        layout_state::{LayoutState, RepositoryLayoutStore},
        settings::DiffView,
    };
    let fixture = Fixture::new();
    let inputs = tempfile::tempdir().unwrap();
    let left = inputs.path().join("left");
    let right = inputs.path().join("right");
    std::fs::create_dir(&left).unwrap();
    std::fs::create_dir(&right).unwrap();
    std::fs::write(left.join("file.txt"), "supplied before\n").unwrap();
    std::fs::write(right.join("file.txt"), "supplied after\n").unwrap();
    let root = fixture.0.canonicalize().unwrap();
    let state_path = root.join("settings.interface.toml");
    RepositoryLayoutStore::new(state_path.clone(), &root)
        .unwrap()
        .save(&LayoutState {
            view: Some(DiffView::Inline),
            wrap_before: Some(false),
            wrap_after: Some(false),
            wrap_inline: Some(false),
            collapse_unchanged: Some(true),
            ..Default::default()
        })
        .unwrap();
    let mut args = Arguments::parse_from([
        "jaywalk",
        inputs.path().to_str().unwrap(),
        "--diff",
        left.to_str().unwrap(),
        right.to_str().unwrap(),
        "--file",
        "file.txt",
    ]);
    args.difftool = Some(Capture {
        identity: Identity {
            pid: 999999,
            started: 1,
            boot: [0, 0],
        },
        grouped: true,
        directory: true,
        repository: fixture.0.join("."),
        working_head: None,
        received: 1,
        total: 1,
        files: vec![File {
            before: "file.txt".into(),
            after: "file.txt".into(),
            kind: "modified".into(),
            before_object: None,
            after_object: None,
        }],
        intent: None,
    });
    let contents = "[local_review]\nfetch_missing=false\n[live]\nenabled=false\n[layout]\nremember_per_repository=true\n";
    let (viewer, cx) = setup_arguments_with_config(&fixture, args, contents, test_cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        assert_eq!(v.view.preferred(), DiffView::Inline);
        assert!(!v.wrapping.inline);
        assert!(v.collapse_unchanged);
        assert_eq!(
            v.code().unwrap().after.read(app).value().as_ref(),
            "supplied after\n"
        );
    });
    std::fs::write(fixture.0.join("file.txt"), "later worktree contents\n").unwrap();
    cx.update(|window, app| {
        viewer.update(app, |v, cx| {
            v.set_view_mode(DiffView::SideBySide, cx);
            v.toggle_wrap(window, cx);
            v.toggle_unchanged_lines(cx);
        })
    });
    draw(cx);
    cx.update(|_, app| {
        let v = viewer.read(app);
        assert_eq!(
            v.code().unwrap().after.read(app).value().as_ref(),
            "supplied after\n"
        );
        let latest = crate::interface_state::latest(&state_path, &root, app)
            .expect("difftool controls save under the producer repository, not temporary inputs");
        assert_eq!(latest.view, Some(DiffView::SideBySide));
        assert_eq!(latest.wrap_before, Some(true));
        assert_eq!(latest.collapse_unchanged, Some(false));
        let normal = v.arguments.for_repository(root.clone());
        let settings = Settings::parse(contents).unwrap();
        let mut layout = SessionLayout::load(&normal, &settings);
        layout.use_latest(&settings, app);
        assert_eq!(layout.saved.view, Some(DiffView::SideBySide));
        assert_eq!(layout.saved.wrap_before, Some(true));
        assert_eq!(layout.saved.collapse_unchanged, Some(false));
        crate::interface_state::flush_for_test(app);
        let reopened = SessionLayout::load(&normal, &settings);
        assert_eq!(reopened.saved.view, Some(DiffView::SideBySide));
        assert_eq!(reopened.saved.wrap_before, Some(true));
        assert_eq!(reopened.saved.collapse_unchanged, Some(false));
    });
}

#[gpui_test::test]
fn setup_is_optional_preserves_the_comparison_and_only_saves_explicit_choices(
    cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let before = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.simulate_resize(size(px(1500.), px(1400.)));
    cx.update(|w, app| viewer.update(app, |v, c| v.execute_command(CommandId::ShowSetup, w, c)));
    draw(cx);
    assert!(cx.debug_bounds("setup-page").is_some());
    let button = cx.debug_bounds("setup-update-0").unwrap();
    cx.simulate_click(button.center(), Default::default());
    draw(cx);
    // Skipping is not consent, even after editing an unsaved checkbox.
    let later = cx.debug_bounds("setup-later").unwrap();
    cx.simulate_click(later.center(), Default::default());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        before
    );
    let path = fixture.0.join("settings.toml");
    assert!(
        !Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .setup
            .completed
    );
    cx.update(|w, app| viewer.update(app, |v, c| v.execute_command(CommandId::ShowSetup, w, c)));
    draw(cx);
    let checks = cx.debug_bounds("setup-update-0").unwrap();
    cx.simulate_click(checks.center(), Default::default());
    draw(cx);
    let save = cx.debug_bounds("setup-save").unwrap();
    cx.simulate_click(save.center(), Default::default());
    draw(cx);
    let saved = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved.setup.completed && saved.updates.automatic_checks);
    assert!(!saved.updates.automatic_downloads && !saved.updates.automatic_installation);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        before
    );
}

#[gpui_test::test]
fn setup_fits_one_normal_viewport_and_stacks_in_narrow_windows(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.simulate_resize(size(px(1500.), px(800.)));
    cx.update(|w, app| viewer.update(app, |v, c| v.execute_command(CommandId::ShowSetup, w, c)));
    draw(cx);
    let page = cx.debug_bounds("setup-page").unwrap();
    let content = cx.debug_bounds("setup-content").unwrap();
    let local = cx.debug_bounds("setup-local").unwrap();
    let updates = cx.debug_bounds("setup-updates").unwrap();
    assert!(
        content.bottom() <= page.bottom(),
        "normal setup should fit: {content:?} in {page:?}"
    );
    assert_eq!(local.top(), updates.top());
    assert!(local.right() < updates.left());
    assert!(cx.debug_bounds("setup-cli-complete").is_some());
    let cli = cx.debug_bounds("setup-cli-action").unwrap();
    assert_eq!(cli.left(), local.left());
    assert!(cli.size.width < local.size.width / 2.);
    assert_eq!(
        cx.debug_bounds("setup-later").unwrap().left(),
        content.left()
    );
    assert_eq!(
        cx.debug_bounds("setup-save").unwrap().right(),
        content.right()
    );

    assert!(cx.debug_bounds("setup-save").unwrap().bottom() <= page.bottom());
    cx.simulate_resize(size(px(560.), px(800.)));
    draw(cx);
    let local = cx.debug_bounds("setup-local").unwrap();
    let updates = cx.debug_bounds("setup-updates").unwrap();
    assert_eq!(local.left(), updates.left());
    assert!(local.bottom() < updates.top());
    let page = cx.debug_bounds("setup-page").unwrap();
    let content = cx.debug_bounds("setup-content").unwrap();
    assert!(content.right() <= page.right());
    cx.simulate_resize(size(px(1000.), px(350.)));
    draw(cx);
    assert!(
        cx.debug_bounds("setup-content").unwrap().bottom()
            > cx.debug_bounds("setup-page").unwrap().bottom()
    );
}

#[gpui_test::test]
fn picker_default_shortcuts_and_large_catalog_cache(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.0.join("catalog")).unwrap();
    for index in 0..500 {
        std::fs::write(fixture.0.join(format!("catalog/item-{index:03}.rs")), "").unwrap();
    }
    fixture.git(&["add", "catalog"]);
    fixture.git(&["commit", "-m", "catalog"]);
    let (viewer, cx) = setup(&fixture, cx);
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Commands)
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        ">"
    );
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("secondary-shift-p");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        ""
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        200
    );
    let searches = cx.update(|_, app| viewer.read(app).palette_test_file_searches());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_file_searches()),
        searches
    );
    cx.simulate_input("item-499");
    draw(cx);
    let searches = cx.update(|_, app| viewer.read(app).palette_test_file_searches());
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        1
    );
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("secondary-shift-p");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_file_searches()),
        searches
    );
    cx.simulate_input("item-499");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).selected_path()),
        Some("catalog/item-499.rs".into())
    );
}

#[gpui_test::test]
fn workspace_command_tracks_configured_bitbucket_accounts(cx: &mut TestAppContext) {
    use jaywalk::accounts::{
        AccountKey, AccountView, ConnectionState, Identity, Provider, application::Accounts,
    };
    crate::account_ui::tests::setup(cx);
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let state = cx.update(|_, app| app.global::<Accounts>().state.clone());
    cx.update(|_, app| {
        state.update(app, |state, cx| {
            state.snapshot.loaded = true;
            state.snapshot.accounts.clear();
            cx.notify();
        })
    });
    cx.simulate_keystrokes("secondary-p");
    cx.simulate_input("workspace");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        0
    );
    cx.update(|_, app| {
        state.update(app, |state, cx| {
            state.snapshot.accounts.push(AccountView {
                name: None,
                generation: 1,
                authentication: jaywalk::accounts::Authentication::Oauth,
                identity: Identity {
                    key: AccountKey {
                        provider: Provider::Bitbucket,
                        subject: "synthetic-bb".into(),
                    },
                    label: "Synthetic".into(),
                },
                workspace: None,
                state: ConnectionState::Connected,
            });
            cx.notify();
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        1
    );
    cx.update(|_, app| {
        state.update(app, |state, cx| {
            state.snapshot.accounts.clear();
            cx.notify();
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        0
    );
}
