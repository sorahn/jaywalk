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
    args: Arguments,
    contents: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Viewer>, &'a mut VisualTestContext) {
    setup_arguments_with_startup(fixture, args, contents, Ok(false), cx)
}
fn setup_arguments_with_startup<'a>(
    fixture: &Fixture,
    mut args: Arguments,
    contents: &str,
    startup: Result<bool, String>,
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
        let view = app.new(|cx| Viewer::new(args, settings, layout, startup, window, cx));
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
        cx.simulate_keystrokes("alt-f5");
        draw(cx);
        let first = cx.update(|_, app| state.read(app).change_navigation_position());
        assert_eq!(first.1, 2);
        assert!(first.0.is_some());
        assert!(cx.debug_bounds("change-counter").is_some());
        let width = cx.debug_bounds("change-counter").unwrap().size.width;
        cx.simulate_keystrokes("alt-f5");
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
        cx.simulate_keystrokes("alt-f5 alt-shift-f5");
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
fn spacing_reloads_without_scaling_fonts_or_losing_the_open_picker(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    let old_font = cx.update(|_, app| gpui::component::Theme::global(app).font_size);
    cx.update(|window, app| {
        let mut settings = viewer.read(app).settings.clone();
        settings.ui.spacing.unit = 6.;
        settings.ui.spacing.interactive = 2.;
        // Match the shared settings service: project global appearance first,
        // then deliver the same settings snapshot to each viewer.
        crate::appearance::apply(&settings, app);
        viewer.update(app, |viewer, cx| {
            viewer.apply_settings(settings, window, cx)
        });
    });
    draw(cx);
    cx.update(|_, app| {
        let theme = gpui::component::Theme::global(app);
        let spacing = theme.host_spacing.unwrap();
        assert_eq!(theme.font_size, old_font);
        assert_eq!(spacing.edge, px(12.));
        assert_eq!(spacing.interactive, px(12.));
        assert_eq!(viewer.read(app).settings.file_indent_width(), 12.);
        assert_eq!(viewer.read(app).comparison().unwrap().identity(), identity);
        assert_eq!(viewer.read(app).palette_test_state(app).1, ">");
    });
    let results = cx.debug_bounds("command-results").unwrap();
    let selected = cx.update(|_, app| viewer.read(app).palette_test_state(app).2);
    cx.simulate_mouse_move(
        point(results.center().x, results.bottom() - px(1.)),
        None,
        Default::default(),
    );
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).2),
        selected
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|window, app| viewer.read(app).focus.is_focused(window)));
}

#[gpui_test::test]
fn palette_bottom_inset_never_selects_a_clipped_row(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    let results = cx.debug_bounds("command-results").unwrap();
    let selected = cx.update(|_, app| viewer.read(app).palette_test_state(app).2);
    cx.simulate_mouse_move(
        point(results.center().x, results.bottom() - px(1.)),
        None,
        Default::default(),
    );
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).2),
        selected,
        "the bottom breathing room must not belong to a hidden row"
    );
    for _ in 0..12 {
        cx.simulate_keystrokes("down");
    }
    draw(cx);
    let selected = cx.update(|_, app| viewer.read(app).palette_test_state(app).2);
    cx.simulate_mouse_move(
        point(results.center().x, results.bottom() + px(1.)),
        None,
        Default::default(),
    );
    cx.simulate_mouse_move(
        point(results.center().x, results.bottom() - px(1.)),
        None,
        Default::default(),
    );
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).2),
        selected,
        "scrolling must not turn the inset into a row hit target"
    );
}

#[gpui_test::test]
fn main_palette_dismisses_reference_browser_and_restores_viewer_focus(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|_, app| {
        viewer.update(app, |viewer, cx| {
            viewer.references.open = true;
            cx.notify();
        })
    });
    draw(cx);
    assert!(cx.debug_bounds("reference-browser").is_some());
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    assert!(!cx.update(|_, app| viewer.read(app).references.open));
    assert!(cx.debug_bounds("reference-browser").is_none());
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::Commands)
    );
    cx.simulate_input("refresh");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).1),
        ">refresh"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).palette_test_state(app).0.is_none()));
    assert!(cx.update(|window, app| viewer.read(app).focus.is_focused(window)));
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
            let caret = cx.debug_bounds("dropdown-trigger-caret").unwrap();
            assert!(caret.size.width >= px(12.));
            assert!(caret.left() >= label.right());
            assert!(caret.right() <= button.right());
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
        let (settings_page, mut popup) = open_accounts_test(&viewer, cx);
        assert!(popup.debug_bounds("accounts-page").is_some());
        assert!(popup.debug_bounds("settings-accounts").is_some());
        assert!(cx.debug_bounds("accounts-page").is_none());
        assert!(cx.debug_bounds("drawer-status-History").is_some());
        let (same_page, _) = open_accounts_test(&viewer, cx);
        assert_eq!(settings_page.entity_id(), same_page.entity_id());
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(CommandId::ShowWelcome, window, cx)
            });
        });
        draw(cx);
        assert!(cx.debug_bounds("welcome-page").is_some());
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
    }
}

#[gpui_test::test]
fn accounts_palette_opens_settings_without_replacing_welcome(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    crate::account_ui::tests::setup(cx);
    let args = Arguments::parse_from(["jaywalk"]).for_welcome();
    let (viewer, cx) = setup_arguments(&fixture, args, cx);
    assert!(cx.debug_bounds("welcome-page").is_some());
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    cx.simulate_input("Accounts");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.debug_bounds("welcome-page").is_some());
    assert_eq!(cx.update(|_, app| app.windows().len()), 2);
    let (_, mut popup) = open_accounts_test(&viewer, cx);
    assert!(popup.debug_bounds("accounts-page").is_some());
    assert!(popup.debug_bounds("settings-save").is_none());
    popup.simulate_keystrokes("escape");
    draw(&mut popup);
    assert!(popup.debug_bounds("settings-page").is_some());
    assert!(popup.debug_bounds("accounts-page").is_none());
    assert!(cx.debug_bounds("welcome-page").is_some());
    let (_, mut popup) = open_accounts_test(&viewer, cx);
    assert!(popup.debug_bounds("accounts-page").is_some());
    assert_eq!(cx.update(|_, app| app.windows().len()), 2);
}

#[gpui_test::test]
fn opening_accounts_keeps_a_loading_comparison_in_place(cx: &mut TestAppContext) {
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
    let (_, mut popup) = open_accounts_test(&viewer, cx);
    assert!(popup.debug_bounds("accounts-page").is_some());
    cx.update(|_, app| {
        let viewer = viewer.read(app);
        assert!(viewer.page == super::pages::Page::Comparison);
        assert_eq!(viewer.code().unwrap().before.entity_id(), editor);
    });
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
    assert!(cx.debug_bounds("accounts-page").is_none());
    let (_, mut popup) = open_accounts_test(&viewer, cx);
    assert!(popup.debug_bounds("accounts-page").is_some());
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
    let path = fixture.0.join("settings.toml");
    let checked = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(checked.updates.automatic_checks);
    assert!(checked.updates.may_check_automatically());
    // The checkbox saves consent immediately; skipping does not undo that choice.
    let later = cx.debug_bounds("setup-later").unwrap();
    cx.simulate_click(later.center(), Default::default());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        before
    );
    let path = fixture.0.join("settings.toml");
    cx.update(|w, app| viewer.update(app, |v, c| v.execute_command(CommandId::ShowSetup, w, c)));
    draw(cx);
    let checks = cx.debug_bounds("setup-update-0").unwrap();
    cx.simulate_click(checks.center(), Default::default());
    draw(cx);
    let save = cx.debug_bounds("setup-save").unwrap();
    cx.simulate_click(save.center(), Default::default());
    draw(cx);
    let saved = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!saved.updates.automatic_checks);
    assert!(!saved.updates.automatic_downloads && !saved.updates.automatic_installation);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        before
    );
}

#[gpui_test::test]
fn older_setup_window_cannot_restore_revoked_update_consent(test_cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let args = Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap()]);
    let mut other_context = test_cx.clone();
    let (first, first_cx) = setup_arguments_with_settings(
        &fixture,
        args,
        "[updates]\nautomatic_checks=true\nautomatic_downloads=true\n",
        test_cx,
    );
    let (arguments, settings) = first_cx.update(|_, app| {
        let viewer = first.read(app);
        (viewer.arguments.clone(), viewer.settings.clone())
    });
    let mut second = None;
    let (_, second_cx) = other_context.add_window_view(|window, app| {
        let layout = SessionLayout::load(&arguments, &settings);
        let view = app.new(|cx| Viewer::new(arguments, settings, layout, Ok(false), window, cx));
        second = Some(view.clone());
        Root::new(view, window, app)
    });
    let second = second.unwrap();
    for (viewer, cx) in [(&first, &mut *first_cx), (&second, &mut *second_cx)] {
        cx.simulate_resize(size(px(1500.), px(1400.)));
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(CommandId::ShowSetup, window, cx);
            })
        });
        draw(cx);
    }
    let checks = first_cx.debug_bounds("setup-update-0").unwrap();
    first_cx.simulate_click(checks.center(), Default::default());
    draw(first_cx);
    let path = fixture.0.join("settings.toml");
    let revoked = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!revoked.updates.automatic_checks);
    second_cx.update(|_, app| {
        second.update(app, |viewer, _| {
            viewer.setup.updates.automatic_checks = true;
            viewer.setup.updates.automatic_downloads = true;
        })
    });
    draw(second_cx);
    let downloads = second_cx.debug_bounds("setup-update-1").unwrap();
    second_cx.simulate_click(downloads.center(), Default::default());
    draw(second_cx);
    let after_checkbox = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!after_checkbox.updates.automatic_checks);
    assert!(!after_checkbox.updates.automatic_downloads);
    second_cx.update(|_, app| assert_eq!(second.read(app).setup.updates, after_checkbox.updates));
    // Completion must preserve the same revocation.
    let save = second_cx.debug_bounds("setup-save").unwrap();
    second_cx.simulate_click(save.center(), Default::default());
    draw(second_cx);
    let saved = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!saved.updates.automatic_checks);
    assert_eq!(saved.updates, revoked.updates);
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

#[gpui_test::test]
fn first_setup_skip_returns_to_welcome_and_manual_setup_remains_available(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let args = Arguments::parse_from(["jaywalk"]).for_welcome();
    // Preparation already persisted seen before constructing this first window.
    let (viewer, cx) = setup_arguments_with_startup(
        &fixture,
        args,
        "[setup]\nseen=true\ncompleted=false\n[layout]\nremember_per_repository=false\n[live]\nenabled=false\n",
        Ok(true),
        cx,
    );
    cx.simulate_resize(size(px(1500.), px(1400.)));
    draw(cx);
    let skip = cx.debug_bounds("setup-later").unwrap();
    cx.simulate_click(skip.center(), Default::default());
    draw(cx);
    assert!(cx.update(|_, app| viewer.read(app).is_welcome()));
    let saved = Settings::parse(&std::fs::read_to_string(fixture.0.join("settings.toml")).unwrap())
        .unwrap();
    assert!(fixture.0.join("settings.toml").exists());
    assert!(!saved.updates.may_check_automatically());
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::ShowSetup, window, cx)
        })
    });
    draw(cx);
    assert!(cx.debug_bounds("setup-page").is_some());
}

#[gpui_test::test]
fn shell_theme_reload_preserves_open_diff_entities_focus_and_geometry(cx: &mut TestAppContext) {
    use jaywalk::theme::{Part, Surface};
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (before, after, model) = cx.update(|_, app| {
        let code = viewer.read(app).code().unwrap();
        (code.before.clone(), code.after.clone(), code.model.clone())
    });
    let bounds = cx.update(|_, app| before.read(app).input_bounds());
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
    let header = cx.debug_bounds("file-header").unwrap();
    let settings = Settings::parse(
        r##"
[local_review]
fetch_missing=false
[live]
enabled=false
[layout]
remember_per_repository=false
[layout.placement]
history_side='left'
files_side='left'
[ui]
appearance="dark"
[theme.shell_auto]
seed="#153049"
[theme.overrides]
"shell.sidebar.files.background"="#273543"
"##,
    )
    .unwrap();
    cx.update(|window, app| {
        crate::appearance::apply(&settings, app);
        viewer.update(app, |v, cx| v.apply_settings(settings, window, cx));
        let code = viewer.read(app).code().unwrap();
        assert_eq!(code.before.entity_id(), before.entity_id());
        assert_eq!(code.after.entity_id(), after.entity_id());
        assert!(std::rc::Rc::ptr_eq(&code.model, &model));
        assert_eq!(
            crate::appearance::surface(Surface::Files, Part::Background, app),
            gpui::Hsla::from(gpui::rgb(0x273543))
        );
        assert_ne!(
            crate::appearance::surface(Surface::History, Part::Background, app),
            crate::appearance::surface(Surface::Files, Part::Background, app)
        );
        assert_eq!(
            gpui::component::Theme::global(app)
                .highlight_theme
                .style
                .editor_background,
            Some(crate::appearance::surface(
                Surface::Viewer,
                Part::Background,
                app
            ))
        );
    });
    draw(cx);
    assert_eq!(cx.debug_bounds("file-header").unwrap(), header);
    assert_eq!(cx.update(|_, app| before.read(app).input_bounds()), bounds);
    assert!(cx.update(|window, app| before.focus_handle(app).is_focused(window)));
}

#[gpui_test::test]
fn branch_checkout_picker_switches_a_remote_branch_and_keeps_the_review_pinned(
    cx: &mut TestAppContext,
) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".gitignore"), "/settings.toml\n").unwrap();
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "clean fixture"]);
    fixture.git(&["switch", "-c", "local-topic"]);
    std::fs::write(fixture.0.join("file.txt"), "branch topic\n").unwrap();
    fixture.git(&["add", "file.txt"]);
    fixture.git(&["commit", "-m", "topic"]);
    fixture.git(&[
        "remote",
        "add",
        "origin",
        "https://example.invalid/repository.git",
    ]);
    fixture.git(&["update-ref", "refs/remotes/origin/remote-topic", "HEAD"]);
    fixture.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/remote-topic",
    ]);
    fixture.git(&["switch", "main"]);
    // Keep a selectable remote beyond the old 200-row prototype cutoff.
    let tip = std::process::Command::new("git")
        .current_dir(&fixture.0)
        .args(["rev-parse", "local-topic"])
        .output()
        .unwrap();
    let tip = String::from_utf8(tip.stdout).unwrap();
    let mut updates = std::process::Command::new("git")
        .current_dir(&fixture.0)
        .args(["update-ref", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let mut input = updates.stdin.take().unwrap();
        for index in 0..700 {
            writeln!(
                input,
                "update refs/remotes/origin/branch-{index:04} {}",
                tip.trim()
            )
            .unwrap();
        }
    }
    assert!(updates.wait().unwrap().success());
    let (viewer, cx) = setup(&fixture, cx);
    let before = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::CheckoutBranch, window, cx)
        })
    });
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_state(app).0),
        Some(palette::PaletteKind::CheckoutBranches)
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        703
    );
    cx.update(|_, app| {
        assert!(
            viewer.read(app).live.notice.is_none(),
            "completed branch listing must clear loading feedback"
        )
    });
    cx.simulate_input("origin/remote-topic");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).palette_test_count(app)),
        1
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    let branch = std::process::Command::new("git")
        .current_dir(&fixture.0)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(branch.stdout).unwrap().trim(),
        "remote-topic"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("file.txt")).unwrap(),
        "branch topic\n"
    );
    cx.update(|_, app| {
        let viewer = viewer.read(app);
        assert_eq!(viewer.comparison().unwrap().identity(), before);
        assert!(viewer.navigation.pinned);
        assert!(viewer.palette_test_state(app).0.is_none());
    });
}

#[gpui_test::test]
fn checkout_this_branch_uses_the_review_aggregate_while_inspecting_another_commit(
    cx: &mut TestAppContext,
) {
    use jaywalk::{
        branch_review::{RefName, Target, Tip},
        commands::CommandId,
    };
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".gitignore"), "/settings.toml\n").unwrap();
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "clean fixture"]);
    fixture.git(&["switch", "-c", "topic"]);
    std::fs::write(fixture.0.join("file.txt"), "topic\n").unwrap();
    fixture.git(&["add", "file.txt"]);
    fixture.git(&["commit", "-m", "topic"]);
    fixture.git(&["switch", "main"]);
    let (viewer, cx) = setup(&fixture, cx);
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.open_review_target(
                Target::Branch {
                    tip: Tip::Reference(RefName {
                        name: "topic".into(),
                        remote: None,
                    }),
                    base: RefName {
                        name: "main".into(),
                        remote: None,
                    },
                },
                window,
                cx,
            )
        })
    });
    draw(cx);
    let base = cx.update(|_, app| {
        viewer
            .read(app)
            .repository
            .ready()
            .unwrap()
            .review
            .aggregate
            .base()
            .to_owned()
    });
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| viewer.select_revision(&base, window, cx))
    });
    draw(cx);
    let before = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::CheckoutThisBranch, window, cx)
        })
    });
    draw(cx);
    let branch = std::process::Command::new("git")
        .current_dir(&fixture.0)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(branch.stdout).unwrap().trim(), "topic");
    cx.update(|_, app| {
        let viewer = viewer.read(app);
        assert_eq!(viewer.comparison().unwrap().identity(), before);
        assert!(matches!(
            viewer.repository.ready().unwrap().scope,
            super::ComparisonScope::Revision { .. }
        ));
    });
}

#[gpui_test::test]
fn cancelling_branch_checkout_preparation_restores_the_previous_pin(cx: &mut TestAppContext) {
    use jaywalk::{
        accounts::Control, commands::CommandId, local_review::checkout::branches::Catalog,
    };
    use std::{sync::Arc, time::Duration};
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".gitignore"), "/settings.toml\n").unwrap();
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "clean fixture"]);
    fixture.git(&["branch", "topic"]);
    let catalog =
        Arc::new(Catalog::capture(&fixture.0, &Control::new(Duration::from_secs(30))).unwrap());
    let branch = catalog
        .branches
        .iter()
        .find(|branch| branch.label() == "topic")
        .unwrap()
        .clone();
    let (viewer, cx) = setup(&fixture, cx);
    let before = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    for pinned in [false, true] {
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(CommandId::CheckoutBranch, window, cx);
            });
        });
        draw(cx);
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.navigation.pinned = pinned;
                viewer.local_actions.branch.catalog = Some(catalog.clone());
                viewer.checkout_selected_branch(branch.clone(), window, cx);
                assert!(viewer.navigation.pinned);
                // Escape before the queued preparation worker starts.
                viewer.close_palette(window, cx);
                assert_eq!(viewer.navigation.pinned, pinned);
            });
        });
        draw(cx);
        assert_eq!(
            cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
            before
        );
        let head = std::process::Command::new("git")
            .current_dir(&fixture.0)
            .args(["branch", "--show-current"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), "main");
    }
}

fn save_settings(cx: &mut VisualTestContext) {
    let save = cx
        .debug_bounds("settings-save")
        .expect("Settings has a Save button");
    cx.simulate_click(save.center(), Default::default());
    draw(cx);
}

#[gpui_test::test]
fn settings_search_edit_reset_and_close_preserve_the_review(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let before = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    let (page, mut settings_cx) = open_settings_test(&viewer, cx, false);
    let main_cx = cx;
    let cx = &mut settings_cx;
    draw(cx);
    assert!(cx.debug_bounds("settings-page").is_some());
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("diff.word_highlight");
    draw(cx);
    let status = cx.update(|_, app| page.read(app).test_status(app));
    let row = cx
        .debug_bounds("setting-diff.word_highlight")
        .unwrap_or_else(|| panic!("{status}"));
    assert!(cx.debug_bounds("reset-diff.word_highlight").is_none());
    let header_height = cx.debug_bounds("settings-header").unwrap().size.height;
    cx.simulate_click(row.center(), Default::default());
    draw(cx);
    assert_eq!(
        cx.debug_bounds("settings-header").unwrap().size.height,
        header_height
    );
    let path = fixture.0.join("settings.toml");
    assert!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .diff
            .word_highlight,
        "a toggle must not write until Save"
    );
    main_cx.update(|_, app| assert!(!viewer.read(app).settings.diff.word_highlight));
    save_settings(cx);
    let saved = Settings::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(!saved.diff.word_highlight);
    assert!(cx.debug_bounds("reset-diff.word_highlight").is_some());
    let reset = cx.debug_bounds("reset-diff.word_highlight").unwrap();
    cx.simulate_click(reset.center(), Default::default());
    draw(cx);
    save_settings(cx);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(Settings::parse(&text).unwrap().diff.word_highlight);
    assert!(cx.debug_bounds("reset-diff.word_highlight").is_none());
    let raw: toml::Value = toml::from_str(&text).unwrap();
    assert!(
        raw.get("diff")
            .and_then(|table| table.get("word_highlight"))
            .is_none()
    );
    let bounds = cx.debug_bounds("settings-modified").unwrap();
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("setting-diff.word_highlight").is_none());
    cx.update(|window, _| window.remove_window());
    draw(main_cx);
    assert_eq!(
        main_cx.update(|_, app| viewer.read(app).comparison().unwrap().identity()),
        before
    );
    assert!(main_cx.debug_bounds("settings-page").is_none());
}

#[gpui_test::test]
fn settings_numeric_drafts_validate_before_saving_and_defaults_are_read_only(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut settings_cx) = open_settings_test(&viewer, cx, false);
    let cx = &mut settings_cx;
    draw(cx);
    let bounds = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(bounds.center(), Default::default());
    cx.simulate_input("ui.font_size");
    draw(cx);
    assert!(
        cx.debug_bounds(if cfg!(target_os = "macos") {
            "kbd:cmd-s"
        } else {
            "kbd:ctrl-s"
        })
        .is_some(),
        "Save must display its configured shortcut"
    );
    let status = cx.update(|_, app| page.read(app).test_status(app));
    let bounds = cx
        .debug_bounds("setting-ui.font_size")
        .unwrap_or_else(|| panic!("{status}"));
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
    let input_focus = cx.update(|window, app| window.focused(app).unwrap());
    let header = cx.debug_bounds("settings-header").unwrap();
    cx.simulate_click(
        point(header.left() + px(20.), header.top() + px(20.)),
        Default::default(),
    );
    draw(cx);
    assert!(!cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("-10");
    cx.simulate_keystrokes("enter");
    draw(cx);
    let path = fixture.0.join("settings.toml");
    assert_eq!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .font_size,
        16.
    );
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("18");
    cx.simulate_keystrokes("secondary-s");
    draw(cx);
    assert_eq!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .font_size,
        18.
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    cx.update(|_, app| page.update(app, |page, cx| page.show_defaults(true, cx)));
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    assert!(cx.debug_bounds("settings-page").is_some());
}

#[gpui_test::test]
fn settings_documents_publish_across_windows_and_reject_old_results(test_cx: &mut TestAppContext) {
    use jaywalk::{preferences::Patch, settings_reload::SettingsSource};
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, test_cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let path = cx.update(|_, app| viewer.read(app).arguments.config.clone().unwrap());
    let source = SettingsSource::resolve(Some(&path)).unwrap();
    let old = source.read_snapshot().unwrap();
    let newer =
        jaywalk::preferences::patch(&source, Patch::set("ui.appearance", "light".into())).unwrap();
    cx.update(|_, app| {
        crate::settings_service::publish_snapshot(source.clone(), newer.clone(), app).unwrap();
        crate::settings_service::publish_snapshot(source.clone(), old, app).unwrap();
    });
    draw(cx);
    draw(&mut popup);
    popup.update(|_, app| {
        assert!(page.read(app).test_status(app).contains("loaded=true"));
    });
    cx.update(|_, app| {
        assert_eq!(
            viewer.read(app).settings.ui.appearance,
            jaywalk::settings::Appearance::Light,
            "config error: {:?}",
            viewer.read(app).config.error
        );
    });
    let settings = cx.update(|_, app| viewer.read(app).settings.clone());
    // An explicit default is a document change even if its effective settings
    // are equal; subscribers still need its provenance for Reset/Set labels.
    let explicit =
        jaywalk::preferences::patch(&source, Patch::set("diff.word_highlight", true.into()))
            .unwrap();
    cx.update(|_, app| {
        let state = crate::settings_service::publish_snapshot(source, explicit, app).unwrap();
        assert!(
            state
                .read(app)
                .snapshot
                .as_ref()
                .unwrap()
                .explicit("diff.word_highlight")
                .is_some()
        );
        assert_eq!(
            state.read(app).current.diff.word_highlight,
            settings.diff.word_highlight
        );
    });
}

#[gpui_test::test]
fn visible_commands_follow_remapped_chords_and_unbound_profiles(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let settings = cx.update(|_, app| viewer.read(app).settings.clone());
    for (binding, expected) in [("['secondary-k secondary-o']", true), ("[]", false)] {
        let profile = jaywalk::keybinding_profiles::Profile::parse(&format!(
            "[keybindings]\nopen_repository={binding}"
        ))
        .unwrap();
        let remapped = settings.clone().with_keybindings(profile).unwrap();
        cx.update(|window, app| {
            super::commands::install_bindings(&remapped, app).unwrap();
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(CommandId::ShowSetup, window, cx)
            });
        });
        draw(cx);
        assert_eq!(cx.debug_bounds("kbd:cmd-k cmd-o").is_some(), expected);
        // The default shortcut must disappear after a remap or an unbind.
        assert!(cx.debug_bounds("kbd:cmd-o").is_none());
    }
}

#[gpui_test::test]
fn settings_remain_usable_in_a_narrow_window(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut settings_cx) = open_settings_test(&viewer, cx, false);
    let cx = &mut settings_cx;
    cx.simulate_resize(size(px(560.), px(600.)));
    draw(cx);
    let page = cx.debug_bounds("settings-page").unwrap();
    let search = cx.debug_bounds("settings-search").unwrap();
    assert!(search.right() <= page.right());
    assert!(search.left() >= page.left());
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.font_size");
    draw(cx);
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    assert!(field.right() <= page.right());
    assert!(field.bottom() <= page.bottom());
}

fn open_accounts_test(
    viewer: &Entity<Viewer>,
    cx: &mut VisualTestContext,
) -> (
    Entity<crate::preferences_ui::SettingsPage>,
    VisualTestContext,
) {
    let (handle, page) = cx.update(|_, app| {
        let viewer = viewer.read(app);
        crate::preferences_window::show_accounts(
            viewer.arguments.clone(),
            viewer.settings.clone(),
            jaywalk::commands::CommandId::Accounts,
            app,
        )
        .unwrap()
    });
    let mut popup = VisualTestContext::from_window(handle, &cx.cx);
    popup.simulate_resize(size(px(1100.), px(800.)));
    draw(&mut popup);
    (page, popup)
}

fn open_settings_test(
    viewer: &Entity<Viewer>,
    cx: &mut VisualTestContext,
    defaults: bool,
) -> (
    Entity<crate::preferences_ui::SettingsPage>,
    VisualTestContext,
) {
    let (handle, page) = cx.update(|_, app| {
        let viewer = viewer.read(app);
        crate::preferences_window::show(
            viewer.arguments.clone(),
            viewer.settings.clone(),
            defaults,
            app,
        )
        .unwrap()
    });
    let mut popup = VisualTestContext::from_window(handle, &cx.cx);
    popup.simulate_resize(size(px(1100.), px(800.)));
    draw(&mut popup);
    (page, popup)
}

#[gpui_test::test]
fn settings_commands_open_a_single_separate_window_for_the_source(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
    for command in [CommandId::ShowSettings, CommandId::ShowDefaultSettings] {
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                viewer.execute_command(command, window, cx)
            })
        });
        draw(cx);
        assert!(cx.debug_bounds("settings-page").is_none());
        cx.update(|_, app| {
            assert_eq!(app.windows().len(), 2);
            assert_eq!(viewer.read(app).comparison().unwrap().identity(), identity);
        });
    }
}

#[gpui_test::test]
fn settings_navigation_selects_one_destination_and_preserves_explicit_search_scope(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(bounds.center(), Default::default());
        draw(cx);
    }
    let status = |cx: &mut VisualTestContext| cx.update(|_, app| page.read(app).test_status(app));
    click(cx, "settings-modified");
    assert!(
        status(cx).contains("category=None changed=true"),
        "{}",
        status(cx)
    );
    click(cx, "settings-modified");
    assert!(
        status(cx).contains("category=None changed=true"),
        "{}",
        status(cx)
    );
    click(cx, "settings-all");
    assert!(
        status(cx).contains("category=None changed=false"),
        "{}",
        status(cx)
    );
    click(cx, "settings-modified");
    click(cx, "settings-category-0");
    assert!(
        status(cx).contains("category=Some(General) changed=false"),
        "{}",
        status(cx)
    );
    click(cx, "settings-search");
    cx.simulate_input("wrap");
    draw(cx);
    click(cx, "settings-category-3");
    click(cx, "settings-search");
    cx.simulate_keystrokes("secondary-a backspace");
    draw(cx);
    assert!(
        status(cx).contains("category=Some(Diff) changed=false"),
        "{}",
        status(cx)
    );
    click(cx, "settings-search");
    cx.simulate_input("wrap");
    draw(cx);
    click(cx, "settings-modified");
    click(cx, "settings-search");
    cx.simulate_keystrokes("secondary-a backspace");
    draw(cx);
    assert!(
        status(cx).contains("category=None changed=true"),
        "{}",
        status(cx)
    );
    click(cx, "settings-all");
    assert!(
        status(cx).contains("category=None changed=false"),
        "{}",
        status(cx)
    );
    cx.simulate_resize(size(px(560.), px(600.)));
    draw(cx);
    click(cx, "settings-modified-small");
    click(cx, "settings-modified-small");
    assert!(
        status(cx).contains("category=None changed=true"),
        "{}",
        status(cx)
    );
    click(cx, "settings-category-menu");
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert!(
        status(cx).contains("category=None changed=false"),
        "{}",
        status(cx)
    );
}

#[gpui_test::test]
fn settings_number_buttons_save_steps_and_fonts_can_return_to_default(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    let (viewer, cx) = setup_arguments_with_settings(
        &fixture,
        args,
        "[editor]\nfont_family='Fixture font not installed'\n",
        cx,
    );
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.font_size");
    draw(cx);
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    let plus = point(field.right() - px(10.), field.center().y);
    cx.simulate_click(plus, Default::default());
    draw(cx);
    let path = fixture.0.join("settings.toml");
    assert_eq!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .font_size,
        16.
    );
    save_settings(cx);
    assert_eq!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .font_size,
        16.5
    );
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    cx.simulate_click(
        point(field.left() + px(10.), field.center().y),
        Default::default(),
    );
    draw(cx);
    save_settings(cx);
    assert_eq!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .font_size,
        16.
    );
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("editor.font_family");
    draw(cx);
    let font = cx.debug_bounds("setting-editor.font_family").unwrap();
    let caret = cx.debug_bounds("dropdown-trigger-caret").unwrap();
    assert!(caret.size.width >= px(12.));
    assert!(caret.left() >= font.left() && caret.right() <= font.right());
    cx.simulate_click(font.center(), Default::default());
    draw(cx);
    cx.simulate_input("Fixture");
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    // Cancel a searched Select without closing Settings, then reopen the same
    // shared Button trigger and use the keyboard to reset its value.
    let font = cx.debug_bounds("setting-editor.font_family").unwrap();
    cx.simulate_click(font.center(), Default::default());
    draw(cx);
    cx.simulate_keystrokes("up enter");
    draw(cx);
    save_settings(cx);
    assert!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .editor
            .font_family
            .is_none()
    );
    let title = cx.debug_bounds("select-trigger-title").unwrap();
    let expected = cx.update(|window, app| {
        let text: gpui_test::SharedString = "Use default".into();
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
            .shape_line(text, window.rem_size() * 0.875, &[run], None)
            .width
    });
    assert!(
        title.size.width >= expected,
        "default label clipped: {title:?}, expected {expected:?}"
    );
}

#[cfg(debug_assertions)]
#[gpui_test::test]
fn settings_inspector_can_pick_an_individual_control(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.appearance");
    draw(cx);
    cx.update(|window, app| window.toggle_inspector(app));
    draw(cx);
    assert!(cx.update(|window, app| window.is_inspector_picking(app)));
    let field = cx.debug_bounds("setting-ui.appearance").unwrap();
    cx.simulate_mouse_move(field.center(), None, Default::default());
    draw(cx);
    cx.update(|window, _| {
        let highlight = window
            .painted_quads()
            .into_iter()
            .find(|quad| {
                quad.background == gpui_test::solid_background(gpui_test::rgba(0x61afef4d))
            })
            .expect("inspector should highlight the hovered control");
        let scale = window.scale_factor();
        assert!(
            highlight.bounds.size.width.as_f32() / scale <= f32::from(field.size.width) + 1.,
            "inspector selected an overlay instead of the control: {highlight:?}, field {field:?}"
        );
    });
    cx.simulate_click(field.center(), Default::default());
    draw(cx);
    assert!(
        !cx.update(|window, app| window.is_inspector_picking(app)),
        "inspector did not find the dropdown hit target at {field:?}"
    );
}

#[gpui_test::test]
fn settings_color_fields_preserve_the_other_appearance_and_toggle_shared_values(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("diff.colors.added_background");
    draw(cx);
    let dark = cx.update(|_, app| gpui::component::Theme::global(app).is_dark());
    let active = if dark { "dark" } else { "light" };
    let other = if dark { "light" } else { "dark" };
    let original: toml::Value = toml::from_str(jaywalk::settings::DEFAULT_CONFIG).unwrap();
    let other_color = original["diff"]["colors"]["added_background"][other].clone();
    let field = cx
        .debug_bounds("setting-diff.colors.added_background")
        .unwrap();
    assert!(
        field.size.width < px(180.),
        "pair must use a compact single-color field"
    );
    cx.simulate_click(
        point(field.right() - px(25.), field.center().y),
        Default::default(),
    );
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("#123456");
    cx.simulate_keystrokes("enter");
    draw(cx);
    save_settings(cx);
    let read = || {
        toml::from_str::<toml::Value>(
            &std::fs::read_to_string(fixture.0.join("settings.toml")).unwrap(),
        )
        .unwrap()
    };
    let saved = read();
    assert_eq!(
        saved["diff"]["colors"]["added_background"][active].as_str(),
        Some("#123456")
    );
    assert_eq!(
        saved["diff"]["colors"]["added_background"][other],
        other_color
    );
    let checkbox = cx
        .debug_bounds("setting-separate-diff.colors.added_background")
        .unwrap();
    cx.simulate_click(checkbox.center(), Default::default());
    draw(cx);
    save_settings(cx);
    assert_eq!(
        read()["diff"]["colors"]["added_background"].as_str(),
        Some("#123456")
    );
    let checkbox = cx
        .debug_bounds("setting-separate-diff.colors.added_background")
        .unwrap();
    cx.simulate_click(checkbox.center(), Default::default());
    draw(cx);
    save_settings(cx);
    let saved = read();
    for mode in ["light", "dark"] {
        assert_eq!(
            saved["diff"]["colors"]["added_background"][mode].as_str(),
            Some("#123456")
        );
    }
}

#[gpui_test::test]
fn settings_color_browsing_and_appearance_changes_do_not_save_overrides(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    for (key, selector) in [
        (
            "diff.colors.added_background",
            "setting-diff.colors.added_background",
        ),
        (
            "diff.colors.removed_background",
            "setting-diff.colors.removed_background",
        ),
    ] {
        let search = cx.debug_bounds("settings-search").unwrap();
        cx.simulate_click(search.center(), Default::default());
        cx.simulate_keystrokes("secondary-a");
        cx.simulate_input(key);
        draw(cx);
        for appearance in ["dark", "light", "dark"] {
            cx.update(|_, app| {
                crate::appearance::apply(
                    &Settings::parse(&format!("[ui]\nappearance='{appearance}'")).unwrap(),
                    app,
                );
            });
            draw(cx);
            let field = cx.debug_bounds(selector).unwrap();
            cx.simulate_click(
                point(field.right() - px(25.), field.center().y),
                Default::default(),
            );
            cx.simulate_keystrokes("enter");
            draw(cx);
            let header = cx.debug_bounds("settings-header").unwrap();
            cx.simulate_click(header.center(), Default::default());
            draw(cx);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        }
    }
}

#[gpui_test::test]
fn settings_save_discard_and_close_prompts_cover_active_text_and_invalid_drafts(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let main = cx;
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.font_size");
    draw(cx);
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("19");
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    cx.update(|window, app| page.update(app, |page, cx| page.request_close(false, window, cx)));
    draw(cx);
    assert!(cx.cx.has_pending_prompt());
    cx.cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).has_changes()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    assert!(!cx.update(|_, app| page.read(app).has_changes()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("-1");
    draw(cx);
    cx.update(|window, app| page.update(app, |page, cx| page.request_close(false, window, cx)));
    draw(cx);
    cx.cx.simulate_prompt_answer("Save");
    draw(cx);
    assert!(cx.debug_bounds("settings-page").is_some());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("20");
    draw(cx);
    cx.update(|window, app| page.update(app, |page, cx| page.request_close(false, window, cx)));
    draw(cx);
    cx.cx.simulate_prompt_answer("Save");
    draw(main);
    assert_eq!(
        Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .font_size,
        20.
    );
    assert_eq!(
        main.update(|_, app| app.windows().len()),
        1,
        "successful Save closes Settings"
    );
    let (page, mut popup) = open_settings_test(&viewer, main, false);
    let cx = &mut popup;
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("diff.word_highlight");
    draw(cx);
    let toggle = cx.debug_bounds("setting-diff.word_highlight").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    let saved = std::fs::read_to_string(&path).unwrap();
    cx.update(|window, app| page.update(app, |page, cx| page.request_close(false, window, cx)));
    draw(cx);
    cx.cx.simulate_prompt_answer("Discard");
    draw(main);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    assert_eq!(main.update(|_, app| app.windows().len()), 1);
}

#[gpui_test::test]
fn settings_drafts_keep_update_consent_off_and_conflicts_preserve_all_external_values(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("updates.automatic_checks");
    draw(cx);
    let toggle = cx.debug_bounds("setting-updates.automatic_checks").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    cx.update(|_, app| assert!(!viewer.read(app).settings.updates.automatic_checks));
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("diff.word_highlight");
    draw(cx);
    let toggle = cx.debug_bounds("setting-diff.word_highlight").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    let source = jaywalk::settings_reload::SettingsSource::resolve(Some(&path)).unwrap();
    let external = jaywalk::preferences::patch(
        &source,
        jaywalk::preferences::Patch::set("diff.word_highlight", true.into()),
    )
    .unwrap();
    let text = external.text.clone();
    cx.update(|_, app| {
        crate::settings_service::publish_snapshot(source, external, app).unwrap();
    });
    draw(cx);
    save_settings(cx);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "conflict must reject the entire batch, including consent"
    );
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("reset-diff.word_highlight").is_none());
}

#[gpui_test::test]
fn settings_quit_requests_use_the_same_unsaved_guard(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("diff.word_highlight");
    draw(cx);
    let toggle = cx.debug_bounds("setting-diff.word_highlight").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    cx.update(|_, app| crate::preferences_window::request_quit(app));
    draw(cx);
    assert!(cx.cx.has_pending_prompt());
    cx.cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).has_changes()));
    assert!(cx.debug_bounds("settings-page").is_some());
    assert!(
        Settings::parse(&std::fs::read_to_string(fixture.0.join("settings.toml")).unwrap())
            .unwrap()
            .diff
            .word_highlight
    );
}

#[gpui_test::test]
fn settings_accounts_sidebar_preserves_query_draft_and_account_form(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    crate::account_ui::tests::setup(cx);
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("diff.word_highlight");
    draw(cx);
    let toggle = cx.debug_bounds("setting-diff.word_highlight").unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    let accounts = cx.debug_bounds("settings-accounts").unwrap();
    cx.simulate_click(accounts.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("accounts-page").is_some());
    assert!(cx.debug_bounds("settings-save").is_none());
    assert!(cx.debug_bounds("settings-defaults").is_none());
    assert!(cx.debug_bounds("accounts-leave").is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(cx.update(|_, app| page.read(app).has_changes()));
    let paste = cx.debug_bounds("accounts-paste-token").unwrap();
    cx.simulate_click(paste.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("accounts-token-input").is_some());
    let all = cx.debug_bounds("settings-all").unwrap();
    cx.simulate_click(all.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("accounts-page").is_none());
    assert!(cx.debug_bounds("setting-diff.word_highlight").is_some());
    assert!(cx.update(|_, app| {
        page.read(app)
            .test_status(app)
            .contains("diff.word_highlight")
    }));
    assert!(cx.update(|_, app| page.read(app).has_changes()));
    let accounts = cx.debug_bounds("settings-accounts").unwrap();
    cx.simulate_click(accounts.center(), Default::default());
    draw(cx);
    assert!(
        cx.debug_bounds("accounts-token-input").is_some(),
        "switching pages must retain the account form"
    );
    let all = cx.debug_bounds("settings-all").unwrap();
    cx.simulate_click(all.center(), Default::default());
    draw(cx);
    save_settings(cx);
    assert!(
        !Settings::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .diff
            .word_highlight
    );
}

#[gpui_test::test]
fn settings_live_typing_and_discard_restore_saved_values_without_writing(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let main = cx;
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let bytes = std::fs::read(&path).unwrap();
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.font_size");
    draw(cx);
    let original = main.update(|_, app| viewer.read(app).settings.ui.font_size);
    let field = cx.debug_bounds("setting-ui.font_size").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("19");
    draw(cx);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.ui.font_size),
        19.
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("-1");
    draw(cx);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.ui.font_size),
        19.,
        "invalid drafts retain last valid preview"
    );
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    draw(main);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.ui.font_size),
        original
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[gpui_test::test]
fn settings_keyboard_remaps_preview_discard_validate_and_save(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let main = cx;
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let source = jaywalk::settings_reload::SettingsSource::resolve(Some(&path)).unwrap();
    let before = source.read_snapshot().unwrap();
    let profile =
        jaywalk::keybinding_profiles::Draft::load(&path, &before.settings.keyboard.profile)
            .unwrap();
    let sidebar = cx.debug_bounds("settings-keyboard").unwrap();
    cx.simulate_click(sidebar.center(), Default::default());
    draw(cx);
    let query = cx.debug_bounds("keyboard-search").unwrap();
    cx.simulate_click(query.center(), Default::default());
    cx.simulate_input("find_file");
    draw(cx);
    let field = cx.debug_bounds("keyboard-find_file-0").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("alt-shift-j");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.keybindings
            [&jaywalk::commands::CommandId::FindFile]
            .clone()),
        ["alt-shift-j"]
    );
    assert_eq!(
        std::fs::read_to_string(&profile.path).unwrap(),
        profile.original
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before.text);
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    draw(main);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.keybindings
            [&jaywalk::commands::CommandId::FindFile]
            .clone()),
        before.settings.keybindings[&jaywalk::commands::CommandId::FindFile]
    );
    let field = cx.debug_bounds("keyboard-find_file-0").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("secondary-b");
    cx.simulate_keystrokes("enter");
    draw(cx);
    save_settings(cx);
    assert_eq!(
        std::fs::read_to_string(&profile.path).unwrap(),
        profile.original,
        "conflicting shortcut must not save"
    );
    let field = cx.debug_bounds("keyboard-find_file-0").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("alt-shift-j");
    save_settings(cx);
    draw(main);
    assert_eq!(
        source.read_snapshot().unwrap().settings.keybindings
            [&jaywalk::commands::CommandId::FindFile],
        ["alt-shift-j"]
    );
}

#[gpui_test::test]
fn settings_profile_selection_is_a_draft_and_survives_navigation(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let main = cx;
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let source = jaywalk::settings_reload::SettingsSource::resolve(Some(&path)).unwrap();
    let before = source.read_snapshot().unwrap();
    let alternate = fixture.0.join("keybindings/alternate.toml");
    std::fs::write(
        &alternate,
        "# second profile\n[keybindings]\nfind_file=['alt-shift-j']\n",
    )
    .unwrap();
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            page.show_keyboard(window, cx);
            page.load_keyboard_profile("keybindings/alternate.toml".into(), window, cx);
        })
    });
    draw(cx);
    draw(main);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.keybindings
            [&jaywalk::commands::CommandId::FindFile]
            .clone()),
        ["alt-shift-j"]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before.text);
    let all = cx.debug_bounds("settings-all").unwrap();
    cx.simulate_click(all.center(), Default::default());
    draw(cx);
    let keyboard = cx.debug_bounds("settings-keyboard").unwrap();
    cx.simulate_click(keyboard.center(), Default::default());
    draw(cx);
    save_settings(cx);
    assert_eq!(
        source.read_snapshot().unwrap().settings.keyboard.profile,
        "keybindings/alternate.toml"
    );
    assert_eq!(
        std::fs::read_to_string(&alternate).unwrap(),
        "# second profile\n[keybindings]\nfind_file=['alt-shift-j']\n",
        "selection must not rewrite the profile"
    );
}

#[gpui_test::test]
fn settings_keyboard_primary_alternate_fields_preserve_drafts(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let main = cx;
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let source = jaywalk::settings_reload::SettingsSource::resolve(Some(&path)).unwrap();
    let before = source.read_snapshot().unwrap();
    let profile =
        jaywalk::keybinding_profiles::Draft::load(&path, &before.settings.keyboard.profile)
            .unwrap();
    let sidebar = cx.debug_bounds("settings-keyboard").unwrap();
    cx.simulate_click(sidebar.center(), Default::default());
    draw(cx);
    let query = cx.debug_bounds("keyboard-search").unwrap();
    cx.simulate_click(query.center(), Default::default());
    cx.simulate_input("find_file");
    draw(cx);
    assert!(cx.debug_bounds("kbd:shift").is_some());
    assert!(cx.debug_bounds("kbd:p").is_some());
    let field = cx.debug_bounds("keyboard-find_file-0").unwrap();
    cx.simulate_click(field.center(), Default::default());
    draw(cx);
    assert!(
        cx.debug_bounds("kbd:p").is_none(),
        "editing shows the original notation instead of keycaps"
    );
    let first = cx.debug_bounds("keyboard-find_file-0").unwrap();
    let second = cx.debug_bounds("keyboard-find_file-1").unwrap();
    assert_eq!(first.origin.y, second.origin.y, "slots share a row");
    assert_eq!(
        first.size.width, second.size.width,
        "empty slots keep the same width"
    );
    assert!(first.size.width <= px(150.), "shortcut inputs stay compact");
    let heading = cx.debug_bounds("keyboard-primary-heading").unwrap();
    assert_eq!(
        heading.origin.x, first.origin.x,
        "single column headings align with inputs"
    );
    assert!(second.origin.x > first.right());
    assert!(cx.debug_bounds("shortcut-add-find_file").is_none());
    save_settings(cx);
    assert_eq!(
        std::fs::read_to_string(&profile.path).unwrap(),
        profile.original,
        "untouched alternate slots create no override"
    );
    cx.simulate_click(second.center(), Default::default());
    cx.simulate_input("alt-shift-j");
    cx.simulate_keystrokes("enter");
    draw(cx);
    draw(main);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.keybindings[&CommandId::FindFile].len()),
        2
    );
    let remove = cx.debug_bounds("shortcut-remove-find_file-1").unwrap();
    cx.simulate_click(remove.center(), Default::default());
    draw(cx);
    let cleared = cx.debug_bounds("keyboard-find_file-1").unwrap();
    assert_eq!(cleared, second, "clearing preserves slot geometry");
    let query = cx.debug_bounds("keyboard-search").unwrap();
    cx.simulate_click(query.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("show_commands");
    draw(cx);
    let remove = cx.debug_bounds("shortcut-remove-show_commands-1").unwrap();
    cx.simulate_click(remove.center(), Default::default());
    draw(cx);
    draw(main);
    assert_eq!(
        main.update(|_, app| viewer.read(app).settings.keybindings[&CommandId::ShowCommands].len()),
        1
    );
    let remove = cx.debug_bounds("shortcut-remove-show_commands-0").unwrap();
    cx.simulate_click(remove.center(), Default::default());
    draw(cx);
    draw(main);
    assert!(main.update(|_, app| {
        viewer.read(app).settings.keybindings[&CommandId::ShowCommands].is_empty()
    }));
    assert_eq!(
        std::fs::read_to_string(&profile.path).unwrap(),
        profile.original,
        "removing shortcuts is still a draft"
    );
    save_settings(cx);
    assert!(
        source.read_snapshot().unwrap().settings.keybindings[&CommandId::ShowCommands].is_empty()
    );
}

#[gpui_test::test]
fn settings_keyboard_preserves_existing_extra_alternatives(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let bindings = ["alt-shift-j", "alt-shift-k", "alt-shift-l"].map(String::from);
    std::fs::create_dir_all(fixture.0.join("keybindings")).unwrap();
    std::fs::write(
        fixture.0.join("keybindings/default.toml"),
        "[keybindings]\nfind_file=['alt-shift-j','alt-shift-k','alt-shift-l']\n",
    )
    .unwrap();
    let (viewer, cx) = setup(&fixture, cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let path = fixture.0.join("settings.toml");
    let source = jaywalk::settings_reload::SettingsSource::resolve(Some(&path)).unwrap();
    let saved = source.read_snapshot().unwrap();
    let sidebar = cx.debug_bounds("settings-keyboard").unwrap();
    cx.simulate_click(sidebar.center(), Default::default());
    draw(cx);
    let query = cx.debug_bounds("keyboard-search").unwrap();
    cx.simulate_click(query.center(), Default::default());
    cx.simulate_input("find_file");
    draw(cx);
    let first = cx.debug_bounds("keyboard-find_file-0").unwrap();
    let extra = cx.debug_bounds("keyboard-find_file-2").unwrap();
    assert!(extra.origin.y > first.bottom());
    let remove = cx.debug_bounds("shortcut-remove-find_file-0").unwrap();
    cx.simulate_click(remove.center(), Default::default());
    draw(cx);
    save_settings(cx);
    let saved =
        jaywalk::keybinding_profiles::Draft::load(&path, &saved.settings.keyboard.profile).unwrap();
    assert_eq!(
        saved.profile.keybindings[&CommandId::FindFile],
        bindings[1..]
    );
}

#[gpui_test::test]
fn welcome_settings_and_palette_links_follow_the_current_keymap(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup_arguments(&fixture, Arguments::parse_from(["jaywalk"]), cx);
    assert!(cx.debug_bounds("shortcut-show_commands").is_some());
    assert!(cx.debug_bounds("shortcut-show_settings").is_some());
    assert!(cx.debug_bounds("shortcut-open_recent").is_some());
    cx.simulate_keystrokes("secondary-p");
    draw(cx);
    assert!(
        cx.debug_bounds("palette-overlay").is_some(),
        "the advertised command-palette shortcut works from Welcome"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    let palette = cx.debug_bounds("welcome-commands").unwrap();
    cx.simulate_click(palette.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("palette-overlay").is_some());
    assert!(cx.debug_bounds("welcome-page").is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    let settings = cx.update(|_, app| viewer.read(app).settings.clone());
    let profile = jaywalk::keybinding_profiles::Profile::parse(
        "[keybindings]\nshow_commands=['alt-shift-j']\nshow_settings=['alt-shift-,']\n",
    )
    .unwrap();
    let remapped = settings.with_keybindings(profile).unwrap();
    cx.update(|window, app| {
        super::commands::install_bindings(&remapped, app).unwrap();
        viewer.update(app, |viewer, cx| {
            viewer.receive_settings(remapped, None, window, cx)
        });
    });
    draw(cx);
    assert!(cx.debug_bounds("kbd:alt-shift-j").is_some());
    assert!(cx.debug_bounds("kbd:alt-shift-,").is_some());
    let settings = cx.debug_bounds("welcome-picker-show_settings").unwrap();
    cx.simulate_click(settings.center(), Default::default());
    draw(cx);
    assert_eq!(cx.update(|_, app| app.windows().len()), 2);
    assert!(
        cx.debug_bounds("welcome-page").is_some(),
        "opening Settings preserves Welcome"
    );
    let (_, mut popup) = open_settings_test(&viewer, cx, false);
    assert!(popup.debug_bounds("settings-page").is_some());
    assert_eq!(
        cx.update(|_, app| app.windows().len()),
        2,
        "the link opens the retained Settings window"
    );
    assert!(cx.update(|_, app| viewer.read(app).is_welcome()));
}

#[gpui_test::test]
#[ignore = "manual keybindings scroll profiling"]
fn settings_keyboard_scroll_profile(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let sidebar = cx.debug_bounds("settings-keyboard").unwrap();
    let started = std::time::Instant::now();
    cx.simulate_click(sidebar.center(), Default::default());
    draw(cx);
    let opening = started.elapsed();
    let mounted = cx.update(|_, app| page.read(app).keyboard_input_count());
    let position = cx.debug_bounds("keyboard-content").unwrap().center();
    let mut frames = Vec::new();
    for index in 0..60 {
        let started = std::time::Instant::now();
        cx.simulate_event(gpui_test::ScrollEvent {
            position,
            delta: gpui_test::ScrollDelta::Pixels(point(
                px(0.),
                px(if index < 30 { -60. } else { 60. }),
            )),
            ..Default::default()
        });
        draw(cx);
        frames.push(started.elapsed().as_secs_f64() * 1000.);
    }
    frames.sort_by(f64::total_cmp);
    eprintln!(
        "keyboard profile: open={:.1}ms mounted={mounted}/{} scroll+3frames median={:.2}ms p95={:.2}ms",
        opening.as_secs_f64() * 1000.,
        jaywalk::commands::CommandId::ALL.len(),
        frames[30],
        frames[57]
    );
}

#[gpui_test::test]
fn settings_keyboard_virtual_scroll_preserves_pending_edits(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let sidebar = cx.debug_bounds("settings-keyboard").unwrap();
    cx.simulate_click(sidebar.center(), Default::default());
    draw(cx);
    let initialized = cx.update(|_, app| page.read(app).keyboard_input_count());
    assert!(
        initialized < CommandId::ALL.len() / 3,
        "opening creates only viewport inputs: {initialized}"
    );
    assert!(cx.debug_bounds("keyboard-about-0").is_some());
    assert!(
        cx.debug_bounds("keyboard-word_wrap-0").is_none(),
        "offscreen rows are not constructed"
    );
    let first = cx.debug_bounds("keyboard-about-0").unwrap();
    let other = cx.debug_bounds("keyboard-go_back-0").unwrap();
    assert_eq!(
        first.origin.x, other.origin.x,
        "virtual rows use the viewport width to align their controls"
    );
    let field = cx.debug_bounds("keyboard-about-0").unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_input("alt-shift-j");
    let position = cx.debug_bounds("keyboard-content").unwrap().center();
    cx.simulate_event(gpui_test::ScrollEvent {
        position,
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-5000.))),
        ..Default::default()
    });
    draw(cx);
    assert!(
        cx.debug_bounds("keyboard-about-0").is_some(),
        "the focused editor stays mounted outside the viewport"
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    save_settings(cx);
    let path = fixture.0.join("settings.toml");
    let saved = jaywalk::settings_reload::SettingsSource::resolve(Some(&path))
        .unwrap()
        .read_snapshot()
        .unwrap();
    assert_eq!(
        saved.settings.keybindings[&CommandId::About],
        ["alt-shift-j"]
    );
    cx.simulate_event(gpui_test::ScrollEvent {
        position,
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(5000.))),
        ..Default::default()
    });
    draw(cx);
    assert!(
        cx.debug_bounds("keyboard-about-0").is_some(),
        "the edited row can be revisited"
    );
}

#[gpui_test::test]
fn setup_settings_link_preserves_onboarding_and_update_consent(cx: &mut TestAppContext) {
    use jaywalk::commands::CommandId;
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    cx.simulate_resize(size(px(1500.), px(1400.)));
    cx.update(|window, app| {
        viewer.update(app, |viewer, cx| {
            viewer.execute_command(CommandId::ShowSetup, window, cx)
        })
    });
    draw(cx);
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let link = cx.debug_bounds("setup-show_settings").unwrap();
    cx.simulate_click(link.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("setup-page").is_some());
    assert_eq!(cx.update(|_, app| app.windows().len()), 2);
    let (_, mut popup) = open_settings_test(&viewer, cx, false);
    assert!(popup.debug_bounds("settings-page").is_some());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(!cx.update(|_, app| viewer.read(app).settings.updates.automatic_checks));
}

#[gpui_test::test]
fn settings_search_restores_the_browsing_row_and_handles_large_fonts(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let all = cx.debug_bounds("settings-all").unwrap();
    cx.simulate_click(all.center(), Default::default());
    draw(cx);
    let position = cx.debug_bounds("settings-page").unwrap().center();
    let top = cx.update(|_, app| page.read(app).test_first_setting_key().unwrap().to_owned());
    cx.simulate_event(gpui_test::ScrollEvent {
        position,
        delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-1200.))),
        ..Default::default()
    });
    draw(cx);
    let before = cx.update(|_, app| page.read(app).test_first_setting_key().unwrap().to_owned());
    assert_ne!(top, before);
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("word wrap");
    draw(cx);
    cx.simulate_keystrokes("secondary-a backspace");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| page.read(app).test_first_setting_key().unwrap().to_owned()),
        before
    );
    let source =
        jaywalk::settings_reload::SettingsSource::resolve(Some(&fixture.0.join("settings.toml")))
            .unwrap();
    let larger = jaywalk::preferences::patch(
        &source,
        jaywalk::preferences::Patch::set("ui.font_size", 24.0.into()),
    )
    .unwrap();
    cx.update(|_, app| {
        crate::settings_service::publish_snapshot(source, larger, app).unwrap();
    });
    draw(cx);
    let modified = cx.debug_bounds("settings-modified").unwrap();
    let count = cx.debug_bounds("settings-modified-count").unwrap();
    assert!(
        count.right() <= modified.right(),
        "enlarged text must not hide the modified count"
    );
    cx.simulate_resize(size(px(560.), px(800.)));
    draw(cx);
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("diff.word_highlight");
    draw(cx);
    let bounds = cx.debug_bounds("settings-page").unwrap();
    let control = cx.debug_bounds("setting-diff.word_highlight").unwrap();
    assert!(control.right() <= bounds.right() && control.left() >= bounds.left());
}

#[gpui_test::test]
fn settings_advanced_groups_reveal_search_and_changes_without_saving_preferences(
    cx: &mut TestAppContext,
) {
    use jaywalk::preferences::{Patch, groups::Group};
    let fixture = Fixture::new();
    let (viewer, main) = setup(&fixture, cx);
    let path = fixture.0.join("settings.toml");
    let source = jaywalk::settings_reload::SettingsSource::resolve(Some(&path)).unwrap();
    let changed = jaywalk::preferences::patch(
        &source,
        Patch::set("theme.shell_auto.hue_shift", 0.5.into()),
    )
    .unwrap();
    main.update(|_, app| {
        crate::settings_service::publish_snapshot(source.clone(), changed, app).unwrap();
    });
    let (page, mut popup) = open_settings_test(&viewer, main, false);
    let cx = &mut popup;
    let category = cx.debug_bounds("settings-category-1").unwrap();
    cx.simulate_click(category.center(), Default::default());
    draw(cx);
    assert!(
        cx.debug_bounds("setting-theme.shell_auto.layer_strength")
            .is_none()
    );

    let order = cx.update(|_, app| page.read(app).test_row_labels());
    let position_of = |key: &str| order.iter().position(|row| row == key).unwrap();
    assert!(position_of("theme.shell_auto.seed.dark") < position_of("group:theme_generation"));
    assert!(position_of("group:theme_generation") < position_of("section:Interface text"));
    assert!(position_of("group:detailed_colors") < position_of("section:Interface text"));
    let body = cx.debug_bounds("settings-content").unwrap();
    for _ in 0..20 {
        if cx
            .debug_bounds("settings-group-theme_generation")
            .is_some_and(|bounds| {
                bounds.center().y >= body.top() && bounds.center().y < body.bottom()
            })
        {
            break;
        }
        cx.simulate_event(gpui_test::ScrollEvent {
            position: body.center(),
            delta: gpui_test::ScrollDelta::Pixels(point(px(0.), px(-150.))),
            ..Default::default()
        });
        draw(cx);
    }
    assert!(
        cx.debug_bounds("settings-group-theme_generation-changed")
            .is_some()
    );
    let before = std::fs::read(&path).unwrap();
    let group = cx.debug_bounds("settings-group-theme_generation").unwrap();
    cx.simulate_click(group.center(), Default::default());
    draw(cx);
    assert!(
        cx.debug_bounds("setting-theme.shell_auto.layer_strength")
            .is_some()
    );
    let field = cx
        .debug_bounds("setting-theme.shell_auto.layer_strength")
        .unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("0.8");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    cx.update(|_, app| crate::interface_state::flush_for_test(app));
    assert!(
        jaywalk::preferences::groups::read(&path)
            .unwrap()
            .contains(&Group::ThemeGeneration)
    );
    let group = cx.debug_bounds("settings-group-theme_generation").unwrap();
    cx.simulate_click(group.center(), Default::default());
    draw(cx);
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("theme.shell_auto.layer_strength");
    draw(cx);
    assert!(
        cx.debug_bounds("setting-theme.shell_auto.layer_strength")
            .is_some()
    );
    assert!(
        cx.debug_bounds("settings-row-theme.shell_auto.layer_strength")
            .unwrap()
            .size
            .height
            < px(70.),
        "short settings no longer inherit oversized fixed row heights"
    );
    assert!(cx.debug_bounds("settings-group-theme_generation").is_none());
    cx.simulate_keystrokes("secondary-a backspace");
    draw(cx);
    let modified = cx.debug_bounds("settings-modified").unwrap();
    cx.simulate_click(modified.center(), Default::default());
    draw(cx);
    assert!(
        cx.debug_bounds("setting-theme.shell_auto.hue_shift")
            .is_some()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    cx.update(|_, app| crate::interface_state::flush_for_test(app));
    assert!(
        !jaywalk::preferences::groups::read(&path)
            .unwrap()
            .contains(&Group::ThemeGeneration)
    );
}

#[gpui_test::test]
fn keyboard_header_actions_align_and_help_stays_below_in_the_content_pane(cx: &mut TestAppContext) {
    fn check_help(cx: &mut VisualTestContext) {
        let header = cx.debug_bounds("settings-header").unwrap();
        let title = cx.debug_bounds("keyboard-heading").unwrap();
        let help = cx.debug_bounds("keyboard-help-trigger").unwrap();
        let profile = cx.debug_bounds("keyboard-profile").unwrap();
        let edit = cx.debug_bounds("keyboard-edit-toml").unwrap();
        assert!(help.left() >= title.right(), "help follows the title");
        assert!(edit.right() <= header.right());
        assert!(profile.right() <= edit.left());
        cx.simulate_mouse_move(help.center(), None, Default::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        draw(cx);
        let tooltip = cx
            .debug_bounds("keyboard-help-content")
            .expect("help tooltip opened");
        assert!(
            tooltip.left() >= header.left() + px(8.),
            "tooltip stays off the sidebar: {tooltip:?} vs {header:?}"
        );
        assert!(tooltip.right() <= header.right());
        assert!(
            tooltip.top() >= help.bottom(),
            "tooltip opens below the trigger"
        );
        cx.simulate_mouse_move(point(px(0.), px(0.)), None, Default::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(cx);
    }
    let fixture = Fixture::new();
    crate::account_ui::tests::setup(cx);
    let (viewer, cx) = setup(&fixture, cx);
    let (_, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let main_edit = cx.debug_bounds("settings-edit-toml").unwrap();
    let main_title = cx.debug_bounds("settings-header-title").unwrap();
    let accounts = cx.debug_bounds("settings-accounts").unwrap();
    cx.simulate_click(accounts.center(), Default::default());
    draw(cx);
    let accounts_title = cx.debug_bounds("settings-header-title").unwrap();
    assert_eq!(accounts_title.top(), main_title.top());
    assert_eq!(accounts_title.size.height, main_title.size.height);
    let keyboard = cx.debug_bounds("settings-keyboard").unwrap();
    cx.simulate_click(keyboard.center(), Default::default());
    draw(cx);
    let edit = cx.debug_bounds("keyboard-edit-toml").unwrap();
    assert_eq!(
        edit.right(),
        main_edit.right(),
        "Edit TOML shares the main Settings action position"
    );
    assert_eq!(edit.top(), main_edit.top());
    assert_eq!(
        cx.debug_bounds("keyboard-heading").unwrap().top(),
        main_title.top()
    );
    check_help(cx);
    let source =
        jaywalk::settings_reload::SettingsSource::resolve(Some(&fixture.0.join("settings.toml")))
            .unwrap();
    let small = jaywalk::preferences::patch(
        &source,
        jaywalk::preferences::Patch::set("ui.font_size", 8.0.into()),
    )
    .unwrap();
    cx.update(|_, app| {
        crate::settings_service::publish_snapshot(source, small, app).unwrap();
    });
    draw(cx);
    check_help(cx);
    cx.simulate_resize(size(px(560.), px(700.)));
    draw(cx);
    check_help(cx);
}

#[gpui_test::test]
fn settings_color_picker_previews_rgb_preserves_the_other_mode_and_discards_without_writes(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let key = "diff.colors.added_background";
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input(key);
    draw(cx);
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let dark = cx.update(|_, app| gpui::component::Theme::global(app).is_dark());
    let mode = if dark { "dark" } else { "light" };
    let other = if dark { "light" } else { "dark" };
    let picker = cx.update(|_, app| page.read(app).test_color_picker(key));
    let trigger = cx
        .debug_bounds("setting-swatch-diff.colors.added_background")
        .unwrap();
    cx.simulate_click(trigger.center(), Default::default());
    draw(cx);
    cx.update(|_, app| {
        assert!(picker.read(app).is_open());
        assert!(!picker.read(app).allows_alpha());
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.update(|_, app| assert!(!picker.read(app).is_open()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    cx.update(|window, app| {
        picker.update(app, |state, cx| {
            assert!(state.preview_hex("#12345680", window, cx));
            assert_eq!(state.preview().unwrap().a, 1.);
        });
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    cx.update(|window, app| {
        picker.update(app, |state, cx| {
            state.select_color(gpui::rgb(0x123456).into(), window, cx)
        })
    });
    draw(cx);
    let settings = cx.update(|_, app| viewer.read(app).settings.clone());
    assert_eq!(
        settings.diff.colors.added_background.resolve(dark).value(),
        0x123456
    );
    let defaults = Settings::default();
    assert_eq!(
        settings.diff.colors.added_background.resolve(!dark),
        defaults.diff.colors.added_background.resolve(!dark)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    cx.update(|_, app| {
        assert_eq!(
            viewer.read(app).settings.diff.colors.added_background,
            defaults.diff.colors.added_background
        )
    });
    // Picker hex entry with alpha is normalized to RGB before reaching settings.
    cx.update(|window, app| {
        picker.update(app, |state, cx| {
            state.commit_hex("#65432180", window, cx).unwrap();
        })
    });
    draw(cx);
    save_settings(cx);
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        saved["diff"]["colors"]["added_background"][mode].as_str(),
        Some("#654321")
    );
    let original: toml::Value = toml::from_str(jaywalk::settings::DEFAULT_CONFIG).unwrap();
    assert_eq!(
        saved["diff"]["colors"]["added_background"][other],
        original["diff"]["colors"]["added_background"][other]
    );
}

#[gpui_test::test]
fn settings_accent_picker_keeps_system_until_a_color_is_selected(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.accent_color");
    draw(cx);
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let trigger = cx.debug_bounds("setting-swatch-ui.accent_color").unwrap();
    cx.simulate_click(trigger.center(), Default::default());
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(cx.update(|_, app| matches!(
        viewer.read(app).settings.ui.accent_color,
        jaywalk::settings::AccentColorSetting::System(_)
    )));
    let picker = cx.update(|_, app| page.read(app).test_color_picker("ui.accent_color"));
    cx.update(|window, app| {
        picker.update(app, |state, cx| {
            state.select_color(gpui::rgb(0x123456).into(), window, cx)
        })
    });
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    save_settings(cx);
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["ui"]["accent_color"].as_str(), Some("#123456"));
}

#[gpui_test::test]
fn settings_toml_only_tuning_uses_documented_search_rows_and_remains_in_changed(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    let (viewer, cx) =
        setup_arguments_with_settings(&fixture, args, "[ui]\nloading_delay_ms=450\n", cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    let all = cx.debug_bounds("settings-all").unwrap();
    cx.simulate_click(all.center(), Default::default());
    draw(cx);
    let rows = cx.update(|_, app| page.read(app).test_row_labels());
    assert!(!rows.iter().any(|key| key == "ui.loading_delay_ms"));
    assert!(!rows.iter().any(|key| key == "group:reload_tuning"));
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_input("ui.loading_delay_ms");
    draw(cx);
    assert!(
        cx.debug_bounds("setting-ui.loading_delay_ms").is_none(),
        "no numeric widget for TOML-only tuning"
    );
    assert!(
        cx.debug_bounds("setting-edit-toml-ui.loading_delay_ms")
            .is_some()
    );
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_keystrokes("backspace");
    draw(cx);
    let changed = cx.debug_bounds("settings-modified").unwrap();
    cx.simulate_click(changed.center(), Default::default());
    draw(cx);
    assert!(
        cx.debug_bounds("setting-edit-toml-ui.loading_delay_ms")
            .is_some()
    );
    let path = fixture.0.join("settings.toml");
    assert_eq!(
        jaywalk::settings::Config::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .ui
            .loading_delay_ms,
        450
    );
}

fn search_setting(cx: &mut VisualTestContext, key: &str) {
    let search = cx.debug_bounds("settings-search").unwrap();
    cx.simulate_click(search.center(), Default::default());
    cx.simulate_keystrokes("secondary-a backspace");
    cx.simulate_input(key);
    draw(cx);
}

#[gpui_test::test]
fn settings_editor_commands_preserve_literal_arguments_and_save_the_active_field(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    search_setting(cx, "settings.editor");
    let mode = cx.debug_bounds("setting-editor-mode").unwrap();
    cx.simulate_click(mode.center(), Default::default());
    cx.simulate_keystrokes("down down down enter");
    draw(cx);
    let program = cx
        .debug_bounds("editor-program")
        .expect("Command opens the executable input");
    cx.simulate_click(program.center(), Default::default());
    cx.simulate_input("editor with spaces");
    draw(cx);
    let add = cx
        .debug_bounds("structured-add-settings.editor-0")
        .expect("Command offers arguments");
    assert!(add.size.width < px(220.), "Add argument must fit its label");
    cx.simulate_click(add.center(), Default::default());
    draw(cx);
    let arg = cx
        .debug_bounds("structured-field-settings.editor-0")
        .unwrap();
    cx.simulate_click(arg.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("$(literal); keep as one argument");
    draw(cx);
    let path = fixture.0.join("settings.toml");
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("editor with spaces")
    );
    assert!(
        cx.update(|_, app| viewer.read(app).settings.settings.editor.is_none()),
        "editor changes wait for Save"
    );
    save_settings(cx);
    let saved = jaywalk::settings::Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved.settings.editor,
        Some(jaywalk::settings::SettingsEditor::Command(vec![
            "editor with spaces".into(),
            "$(literal); keep as one argument".into()
        ]))
    );
    // Removing an argument is a normal draft and Discard restores it.
    let remove = cx
        .debug_bounds("structured-remove-settings.editor-1")
        .unwrap();
    cx.simulate_click(remove.center(), Default::default());
    draw(cx);
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| page
            .read(app)
            .test_structured_fields("settings.editor")
            .len()),
        2
    );
}

#[gpui_test::test]
fn settings_profile_folders_use_the_native_chooser_keep_order_and_reject_late_results(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (viewer, cx) = setup(&fixture, cx);
    let (page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    search_setting(cx, "keyboard.additional_directories");
    let toggle = cx
        .debug_bounds("setting-structured-keyboard.additional_directories")
        .unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    for (browse, path) in [
        (
            "structured-browse-keyboard.additional_directories-0",
            "relative first",
        ),
        (
            "structured-browse-keyboard.additional_directories-1",
            "relative second",
        ),
    ] {
        let add = cx
            .debug_bounds("structured-add-keyboard.additional_directories-0")
            .unwrap();
        cx.simulate_click(add.center(), Default::default());
        draw(cx);
        let browse = cx.debug_bounds(browse).unwrap();
        cx.simulate_click(browse.center(), Default::default());
        draw(cx);
        cx.cx.simulate_path_prompt_response(|options| {
            assert!(options.directories && !options.files && !options.multiple);
            Some(vec![path.into()])
        });
        draw(cx);
    }
    let handle = cx
        .debug_bounds("structured-handle-keyboard.additional_directories-1")
        .unwrap();
    let target = cx
        .debug_bounds("structured-row-keyboard.additional_directories-0")
        .unwrap();
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        handle.center() + point(px(8.), px(-8.)),
        Some(MouseButton::Left),
        Default::default(),
    );
    draw(cx);
    cx.simulate_mouse_move(target.center(), Some(MouseButton::Left), Default::default());
    draw(cx);
    cx.simulate_mouse_up(target.center(), MouseButton::Left, Default::default());
    draw(cx);
    let handle = cx
        .debug_bounds("structured-handle-keyboard.additional_directories-0")
        .unwrap();
    cx.simulate_click(handle.center(), Default::default());
    cx.simulate_keystrokes("alt-down");
    draw(cx);
    assert_eq!(
        cx.update(|_, app| page
            .read(app)
            .test_structured_fields("keyboard.additional_directories")[0]
            .1
            .read(app)
            .value()
            .to_string()),
        "relative first"
    );
    cx.simulate_keystrokes("alt-up");
    draw(cx);
    save_settings(cx);
    let path = fixture.0.join("settings.toml");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        jaywalk::settings::Config::parse(&saved)
            .unwrap()
            .keyboard
            .additional_directories,
        [
            PathBuf::from("relative second"),
            PathBuf::from("relative first")
        ]
    );
    let field = cx
        .debug_bounds("structured-field-keyboard.additional_directories-0")
        .unwrap();
    cx.simulate_click(field.center(), Default::default());
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input("temporary unsaved folder");
    draw(cx);
    let browse = cx
        .debug_bounds("structured-browse-keyboard.additional_directories-0")
        .unwrap();
    cx.simulate_click(browse.center(), Default::default());
    draw(cx);
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    cx.cx
        .simulate_path_prompt_response(|_| Some(vec!["late folder".into()]));
    draw(cx);
    assert_eq!(std::fs::read_to_string(path).unwrap(), saved);
    assert!(!cx.update(|_, app| page.read(app).has_changes()));
}

#[gpui_test::test]
fn settings_remote_rules_include_repository_and_saved_names_and_default_removes_one_override(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    fixture.git(&[
        "remote",
        "add",
        "origin",
        "https://example.invalid/repository.git",
    ]);
    let git_config = std::fs::read(fixture.0.join(".git/config")).unwrap();
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    let (viewer, cx) = setup_arguments_with_settings(
        &fixture,
        args,
        "[history.remotes.overrides]\n'backup.eu'=true # saved remote note\n",
        cx,
    );
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    search_setting(cx, "history.remotes.overrides");
    let toggle = cx
        .debug_bounds("setting-structured-history.remotes.overrides")
        .unwrap();
    cx.simulate_click(toggle.center(), Default::default());
    draw(cx);
    assert!(cx.debug_bounds("remote-rule-backup.eu").is_some());
    let origin = cx
        .debug_bounds("remote-rule-origin")
        .expect("read configured remote names");
    cx.simulate_click(origin.center(), Default::default());
    cx.simulate_keystrokes("down down down enter");
    draw(cx);
    save_settings(cx);
    let path = fixture.0.join("settings.toml");
    let saved = std::fs::read_to_string(&path).unwrap();
    let config = jaywalk::settings::Config::parse(&saved).unwrap();
    assert_eq!(config.history.remotes.overrides.get("origin"), Some(&false));
    assert!(config.history.remotes.overrides["backup.eu"]);
    assert!(saved.contains("# saved remote note"));
    let origin = cx.debug_bounds("remote-rule-origin").unwrap();
    cx.simulate_click(origin.center(), Default::default());
    cx.simulate_keystrokes("down enter");
    draw(cx);
    save_settings(cx);
    let config = jaywalk::settings::Config::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(!config.history.remotes.overrides.contains_key("origin"));
    assert!(config.history.remotes.overrides["backup.eu"]);
    assert_eq!(
        std::fs::read(fixture.0.join(".git/config")).unwrap(),
        git_config,
        "suggestions must not mutate repository configuration"
    );
}

#[gpui_test::test]
fn settings_application_picker_stages_a_native_selection_and_can_restore_system_default(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    let (viewer, cx) =
        setup_arguments_with_settings(&fixture, args, "[settings]\neditor='Zed'\n", cx);
    let (_page, mut popup) = open_settings_test(&viewer, cx, false);
    let cx = &mut popup;
    search_setting(cx, "settings.editor");
    assert!(
        cx.debug_bounds("editor-program").is_some(),
        "existing application settings expose their input immediately"
    );
    let path = fixture.0.join("settings.toml");
    let before = std::fs::read_to_string(&path).unwrap();
    let browse = cx.debug_bounds("editor-browse").unwrap();
    cx.simulate_click(browse.center(), Default::default());
    draw(cx);
    cx.cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec!["/Applications/Example Editor.app".into()])
    });
    draw(cx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    save_settings(cx);
    assert_eq!(
        jaywalk::settings::Config::parse(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
            .settings
            .editor,
        Some(jaywalk::settings::SettingsEditor::App(
            "/Applications/Example Editor.app".into()
        ))
    );
    let mode = cx.debug_bounds("setting-editor-mode").unwrap();
    cx.simulate_click(mode.center(), Default::default());
    cx.simulate_keystrokes("down enter");
    draw(cx);
    save_settings(cx);
    assert!(
        jaywalk::settings::Config::parse(&std::fs::read_to_string(path).unwrap())
            .unwrap()
            .settings
            .editor
            .is_none()
    );
}

#[gpui_test::test]
fn settings_shell_source_previews_discards_and_saves_without_rewriting_custom_seeds(
    cx: &mut TestAppContext,
) {
    use jaywalk::theme::{Part, Source, Surface};
    let fixture = Fixture::new();
    let args =
        Arguments::parse_from(["jaywalk", fixture.0.to_str().unwrap(), "--file", "file.txt"]);
    let input = r##"[ui]
appearance="dark"
[theme.shell_auto]
seed={light="#575757",dark="#153049"}
[theme.overrides]
"editor.background"="#1d2021"
"editor.foreground"="#ebdbb2"
"##;
    let (viewer, main) = setup_arguments_with_settings(&fixture, args, input, cx);
    let identity = main.update(|_, app| viewer.read(app).code().unwrap().before.entity_id());
    let original =
        main.update(|_, app| crate::appearance::surface(Surface::History, Part::Background, app));
    let path = fixture.0.join("settings.toml");
    let saved = std::fs::read_to_string(&path).unwrap();
    let seed = Settings::parse(&saved).unwrap().theme.shell_auto.seed;
    let (page, mut popup) = open_settings_test(&viewer, main, false);
    let cx = &mut popup;
    search_setting(cx, "theme.shell_auto.source");
    let select = |cx: &mut VisualTestContext| {
        let field = cx.debug_bounds("setting-theme.shell_auto.source").unwrap();
        cx.simulate_click(field.center(), Default::default());
        cx.simulate_keystrokes("down down enter");
        draw(cx);
    };
    select(cx);
    assert!(cx.update(|_, app| page.read(app).has_changes()));
    assert_eq!(
        cx.update(|_, app| viewer.read(app).settings.theme.shell_auto.source),
        Source::CodeTheme
    );
    assert_ne!(
        cx.update(|_, app| crate::appearance::surface(Surface::History, Part::Background, app)),
        original
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    let discard = cx.debug_bounds("settings-discard").unwrap();
    cx.simulate_click(discard.center(), Default::default());
    draw(cx);
    assert_eq!(
        cx.update(|_, app| crate::appearance::surface(Surface::History, Part::Background, app)),
        original
    );
    assert_eq!(
        cx.update(|_, app| viewer.read(app).settings.theme.shell_auto.source),
        Source::Custom
    );
    select(cx);
    save_settings(cx);
    let actual = Settings::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(actual.theme.shell_auto.source, Source::CodeTheme);
    assert_eq!(actual.theme.shell_auto.seed, seed);
    assert!(!actual.updates.automatic_checks);
    assert_eq!(
        cx.update(|_, app| viewer.read(app).code().unwrap().before.entity_id()),
        identity
    );
}
