//! One coherent overview per comparison, built in the background and reused
//! while visiting files. The unified widget owns folding, selection and copy.
use super::*;
use gpui::component::Sizable;
use gpui::component::diff::DiffAnnotation;
use gpui::component::spinner::Spinner;
use gpui::prelude::FluentBuilder as _;
use jaywalk::overview::{CommitDetails, OverviewSeries};
use jaywalk::repository::{ChangedFile, ComparisonIdentity};
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
struct Request {
    source: ComparisonIdentity,
    tab_size: usize,
    series: bool,
    algorithm: jaywalk::diff_model::Algorithm,
    options: jaywalk::diff_options::ComparisonOptions,
}
struct FileSummary {
    file: ChangedFile,
    notice: Option<String>,
}
struct PreparedOverview {
    series: Option<Arc<OverviewSeries>>,
    commit: Option<CommitDetails>,
    warning: Option<String>,
    files: Arc<BTreeMap<String, FileSummary>>,
    documents: Vec<DiffFile>,
}
impl PreparedOverview {
    fn load(
        comparison: &Comparison,
        tab_size: usize,
        whole_branch: bool,
        algorithm: jaywalk::diff_model::Algorithm,
        options: jaywalk::diff_options::ComparisonOptions,
    ) -> Result<Self, String> {
        let mut snapshot = comparison.overview_with_options(algorithm, options);
        let series = if whole_branch {
            match comparison.overview_series() {
                Ok(series) => series.map(Arc::new),
                Err(error) => {
                    snapshot.metadata_error = Some(match snapshot.metadata_error {
                        Some(previous) => format!("{previous}; series: {error}"),
                        None => format!("Series metadata unavailable: {error}"),
                    });
                    None
                }
            }
        } else {
            None
        };
        let mut documents = Vec::with_capacity(snapshot.files.len());
        let mut files = BTreeMap::new();
        for file in snapshot.files {
            let mut parsed = DiffFile::parse(&file.patch).map_err(|error| {
                format!("Could not prepare {}: {error}", file.file.after_path())
            })?;
            if parsed.len() != 1 {
                return Err(format!(
                    "Expected one overview entry for {}",
                    file.file.after_path()
                ));
            }
            let mut document = parsed.remove(0);
            if let Some(rows) = file.alignment {
                document = document.with_alignment(
                    rows.into_iter()
                        .map(|r| gpui::component::diff::DiffLineAlignment {
                            original: r.before,
                            modified: r.after,
                            changed: r.changed,
                        })
                        .collect(),
                )?;
            }
            if document.path().as_ref() != file.file.after_path() {
                return Err(format!(
                    "Overview path did not match {}",
                    file.file.after_path()
                ));
            }
            documents.push(
                document
                    .with_language(file_language(file.file.after_path()))
                    .with_tab_size(tab_size),
            );
            files.insert(
                file.file.after_path().to_owned(),
                FileSummary {
                    file: file.file,
                    notice: file.notice,
                },
            );
        }
        Ok(Self {
            series,
            commit: snapshot.commit,
            warning: snapshot.metadata_error,
            files: Arc::new(files),
            documents,
        })
    }
}
#[derive(Clone)]
struct LoadedOverview {
    request: Request,
    root: std::path::PathBuf,
    series: Option<Arc<OverviewSeries>>,
    operation: String,
    scope: String,
    commit: Option<CommitDetails>,
    files: Arc<BTreeMap<String, FileSummary>>,
    state: Entity<DiffState>,
    additions: usize,
    deletions: usize,
}
#[derive(Default)]
pub(super) struct OverviewState {
    requested: Option<Request>,
    expanded_messages: HashSet<String>,
    current: Option<LoadedOverview>,
    task: Option<Task<()>>,
    generation: u64,
    pub(super) error: Option<String>,
    restore_offset: Option<ListOffset>,
    restore_collapsed: HashSet<String>,
    pending_file: Option<(ComparisonIdentity, String)>,
}
#[derive(Clone)]
pub(super) struct OverviewCheckpoint {
    loaded: LoadedOverview,
    offset: ListOffset,
    horizontal: Pixels,
}
impl Viewer {
    pub(super) fn navigate_overview_file(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        if self.selected_file.is_some() || matches!(self.comparison_state(), LoadState::Loading) {
            return false;
        }
        let Some(comparison) = self.comparison() else {
            return false;
        };
        if !comparison
            .files()
            .iter()
            .any(|file| file.after_path() == path)
        {
            return false;
        }
        let identity = comparison.identity();
        if let Some(loaded) = self
            .overview
            .current
            .as_ref()
            .filter(|loaded| loaded.request.source == identity)
        {
            loaded
                .state
                .update(cx, |state, cx| state.scroll_to_file(path, cx));
        } else {
            self.overview.pending_file = Some((identity, path.to_owned()));
        }
        self.track_overview_file(Some(path.to_owned()), cx);
        true
    }

    pub(super) fn capture_overview_identity(&self) -> Option<ComparisonIdentity> {
        self.overview
            .current
            .as_ref()
            .map(|loaded| loaded.request.source.clone())
    }
    pub(super) fn capture_overview(&self, cx: &App) -> Option<OverviewCheckpoint> {
        let loaded = self.overview.current.as_ref()?;
        if loaded.request.source != self.comparison()?.identity() {
            return None;
        }
        let state = loaded.state.read(cx);
        Some(OverviewCheckpoint {
            loaded: loaded.clone(),
            offset: state.viewport_offset(),
            horizontal: state.horizontal_offset(),
        })
    }
    pub(super) fn restore_overview(
        &mut self,
        checkpoint: Option<OverviewCheckpoint>,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_overview();
        self.finish_loading(LoadingKind::Overview);
        self.overview.restore_offset = None;
        self.overview.restore_collapsed.clear();
        if let Some(checkpoint) = checkpoint {
            checkpoint.loaded.state.update(cx, |state, cx| {
                state.restore_viewport_offset(checkpoint.offset, cx);
                state.scroll_horizontal(checkpoint.horizontal - state.horizontal_offset(), cx);
            });
            if checkpoint.loaded.request.tab_size != self.settings.editor.tab_size {
                self.overview.restore_offset = Some(checkpoint.offset);
            }
            self.overview.requested = Some(checkpoint.loaded.request.clone());
            self.overview.current = Some(checkpoint.loaded);
        }
    }
    pub(super) fn overview_revision(
        &self,
        id: &str,
    ) -> Option<(jaywalk::history::Revision, Option<String>)> {
        let loaded = self.overview.current.as_ref()?;
        let revision = loaded
            .series
            .as_ref()?
            .commits
            .iter()
            .find_map(|commit| {
                commit
                    .revision
                    .as_ref()
                    .filter(|revision| revision.commit_id == id)
            })?
            .clone();
        Some((
            revision,
            (!loaded.operation.is_empty()).then(|| loaded.operation.clone()),
        ))
    }
    pub(super) fn overview_comparison_label(&self, label: String) -> AnyElement {
        match &self.overview.current {
            Some(loaded) if self.selected_file.is_none() && !loaded.files.is_empty() => {
                ComparisonCaption {
                    state: loaded.state.clone(),
                    label,
                    preamble_scrolled: false,
                    subject: loaded.series.as_ref().map_or_else(
                        || message_parts(loaded.commit.as_ref()).0.to_owned(),
                        |series| series.caption(),
                    ),
                    content: None,
                }
                .into_any_element()
            }
            _ => label.into_any_element(),
        }
    }
    pub(super) fn navigate_overview_change(&mut self, direction: i32, cx: &mut Context<Self>) {
        if let Some(loaded) = &self.overview.current {
            loaded.state.update(cx, |state, cx| {
                if direction < 0 {
                    state.previous_change(cx);
                } else {
                    state.next_change(cx);
                }
            });
        }
    }
    pub(super) fn preserve_overview_reading(&mut self, cx: &App) {
        if self.selected_file.is_none()
            && let Some(loaded) = &self.overview.current
        {
            let state = loaded.state.read(cx);
            self.overview.restore_offset = Some(state.viewport_offset());
            self.overview.restore_collapsed = state
                .files()
                .iter()
                .filter(|file| state.is_file_collapsed(file.path()))
                .map(|file| file.path().to_string())
                .collect();
        }
    }

    pub(super) fn invalidate_overview(&mut self) {
        self.overview.requested = None;
        self.overview.generation += 1;
        self.overview.task = None;
        self.overview.error = None;
    }
    pub(super) fn sync_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_file.is_some() {
            return;
        }
        let Some(comparison) = self.comparison() else {
            return;
        };
        let request = Request {
            source: comparison.identity(),
            tab_size: self.settings.editor.tab_size,
            algorithm: self.settings.diff.algorithm,
            options: self.diff_preferences.comparison,
            series: matches!(
                self.repository.ready().map(|session| &session.scope),
                Some(ComparisonScope::Aggregate)
            ),
        };
        if self.overview.requested.as_ref() == Some(&request) {
            return;
        }
        let comparison = comparison.clone();
        let comparison_operation = comparison.operation().to_owned();
        let comparison_root = comparison.root().to_path_buf();
        let scope = self.scope_label();
        self.overview.requested = Some(request.clone());
        self.overview.generation += 1;
        let generation = self.overview.generation;
        self.overview.error = None;
        self.begin_loading(LoadingKind::Overview, window, cx);
        self.overview.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    PreparedOverview::load(
                        &comparison,
                        request.tab_size,
                        request.series,
                        request.algorithm,
                        request.options,
                    )
                })
                .await;
            let _ = this.update(cx, |viewer, cx| {
                if viewer.overview.generation != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Overview);
                if viewer
                    .comparison()
                    .is_none_or(|comparison| comparison.identity() != request.source)
                    || viewer.settings.editor.tab_size != request.tab_size
                    || viewer.diff_preferences.comparison != request.options
                    || viewer.settings.diff.algorithm != request.algorithm
                    || request.series
                        != matches!(
                            viewer.repository.ready().map(|session| &session.scope),
                            Some(ComparisonScope::Aggregate)
                        )
                {
                    viewer.overview.requested = None;
                    cx.notify();
                    return;
                }
                match result {
                    Err(error) => {
                        viewer.overview.error = Some(format!("Overview not updated: {error}"))
                    }
                    Ok(prepared) => {
                        let additions = prepared.documents.iter().map(DiffFile::additions).sum();
                        let deletions = prepared.documents.iter().map(DiffFile::deletions).sum();
                        let context = viewer.settings.overview.context_lines;
                        let expansion = viewer.settings.overview.expansion_lines;
                        let state = cx.new(|cx| {
                            DiffState::new(prepared.documents, cx)
                                .with_mode(DiffMode::Unified)
                                .with_context_lines(Some(context))
                                .with_expansion_lines(expansion)
                        });
                        let collapsed = std::mem::take(&mut viewer.overview.restore_collapsed);
                        state.update(cx, |state, cx| {
                            for path in collapsed {
                                state.set_file_collapsed(&path, true, cx);
                            }
                        });
                        if let Some(offset) = viewer.overview.restore_offset.take() {
                            state.update(cx, |state, cx| state.restore_viewport_offset(offset, cx));
                        }
                        viewer.overview.current = Some(LoadedOverview {
                            request: request.clone(),
                            root: comparison_root,
                            series: prepared.series,
                            operation: comparison_operation,
                            scope,
                            commit: prepared.commit,
                            files: prepared.files,
                            state,
                            additions,
                            deletions,
                        });
                        if let Some((identity, path)) = viewer.overview.pending_file.take()
                            && identity == request.source
                        {
                            viewer.navigate_overview_file(&path, cx);
                        }
                        viewer.overview.error = prepared
                            .warning
                            .map(|error| format!("Commit metadata unavailable: {error}"));
                    }
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn remeasure_overview_header(&self, cx: &mut Context<Self>) {
        if let Some(loaded) = &self.overview.current {
            loaded
                .state
                .update(cx, |state, cx| state.remeasure_preamble(cx));
        }
    }
    pub(super) fn apply_overview_settings(&mut self, previous: &Settings, cx: &mut Context<Self>) {
        if let Some(loaded) = &self.overview.current {
            loaded.state.update(cx, |state, cx| {
                if previous.overview.context_lines != self.settings.overview.context_lines {
                    state.set_context_lines(Some(self.settings.overview.context_lines), cx);
                }
                if previous.overview.expansion_lines != self.settings.overview.expansion_lines {
                    state.set_expansion_lines(self.settings.overview.expansion_lines, cx);
                }
            });
        }
    }
    pub(super) fn render_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let surface = div()
            .key_context("JaywalkDiff")
            .v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(crate::appearance::surface(
                jaywalk::theme::Surface::Viewer,
                jaywalk::theme::Part::Foreground,
                cx,
            ));
        if let LoadState::Failed(error) = self.comparison_state() {
            return surface
                .child(
                    div()
                        .p(crate::spacing::space(6.0, cx))
                        .child(feedback::error_message("overview-error", &error, cx)),
                )
                .into_any_element();
        }
        let loaded = self.overview.current.as_ref().filter(|loaded| {
            // A newly selected hosted comparison must not inherit another
            // review's body/counts while its own full-text overview is prepared.
            !self.is_hosted_review()
                || self
                    .comparison()
                    .is_some_and(|comparison| comparison.identity() == loaded.request.source)
        });
        let Some(loaded) = loaded else {
            let loading = self.overview.error.is_none()
                && (self.loading_visible(LoadingKind::Comparison)
                    || self.loading_visible(LoadingKind::Overview));
            let text = if self.overview.error.is_some() {
                "Overview unavailable"
            } else if loading {
                "Loading overview…"
            } else {
                ""
            };
            return surface
                .child(
                    div()
                        .debug_selector(|| "overview-preparing".into())
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(crate::spacing::space(6.0, cx))
                        .text_size(px(self.settings.ui.font_size))
                        .text_color(theme.muted_foreground)
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap(crate::spacing::edge(cx))
                                .children(loading.then(|| {
                                    Spinner::new()
                                        .with_size(px(self.settings.ui.font_size))
                                        .color(theme.muted_foreground)
                                }))
                                .child(text),
                        ),
                )
                .into_any_element();
        };
        let (title, body) = message_parts(loaded.commit.as_ref());
        let metadata = loaded
            .commit
            .as_ref()
            .map(|commit| format!("{} · {}", commit.author, commit.timestamp));
        let state = loaded.state.clone();
        let context = self.settings.overview.context_lines;
        let full = loaded.state.read(cx).context_lines().is_none();
        let standalone = loaded.files.is_empty();
        let max_height = self.settings.overview.message_max_height;
        let title = title.to_owned();
        let body = body.to_owned();
        let summary = format!(
            "{} · {} files · +{} −{}",
            loaded.scope,
            loaded.files.len(),
            loaded.additions,
            loaded.deletions
        );
        let border = theme.border;
        let muted = theme.muted_foreground;
        let series = loaded.series.clone();
        let expanded_messages = self.overview.expanded_messages.clone();
        let series_viewer = cx.entity().downgrade();
        let hosted = self
            .repository
            .ready()
            .and_then(|s| s.review.aggregate.hosted())
            .map(|contents| contents.data.metadata.clone());
        let status_viewer = cx.entity().downgrade();
        let status_key = self
            .repository
            .ready()
            .map(|s| s.review.aggregate.identity());
        let status = self.pr_status.presentation(status_key.as_ref());
        let expanded = move |cx: &App| {
            let state = state.clone();
            let mut header = div()
                .id("overview-message")
                .v_flex()
                .gap(crate::spacing::edge(cx))
                .p(crate::spacing::space(4.0, cx))
                .when(standalone, |header| {
                    header.max_h(px(max_height)).overflow_y_scroll()
                })
                .flex_shrink_0()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(crate::spacing::edge(cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .truncate()
                                .child(summary.clone()),
                        )
                        .child(
                            Button::new("overview-context")
                                .ghost()
                                .small()
                                .label(if full {
                                    "Fold unchanged lines"
                                } else {
                                    "Show full files"
                                })
                                .on_click(move |_, _, cx| {
                                    state.update(cx, |state, cx| {
                                        state.set_context_lines(
                                            if full { Some(context) } else { None },
                                            cx,
                                        )
                                    })
                                }),
                        ),
                );
            if let Some(metadata) = &hosted {
                let locator = metadata.locator.clone();
                header =
                    header
                        .child(div().text_sm().text_color(muted).whitespace_normal().child(
                            format!(
                                "{} · by {} · source {} → target {} · merge base {}{}",
                                if metadata.draft == Some(true) {
                                    "DRAFT"
                                } else {
                                    &metadata.state
                                },
                                metadata.author,
                                metadata
                                    .source_branch
                                    .as_deref()
                                    .unwrap_or(&metadata.source.commit[..12]),
                                metadata
                                    .target_branch
                                    .as_deref()
                                    .unwrap_or(&metadata.target.commit[..12]),
                                &metadata.base.commit[..12],
                                if metadata.history_partial {
                                    " · partial history"
                                } else {
                                    ""
                                }
                            ),
                        ))
                        .child(
                            Button::new("hosted-provider-browser")
                                .ghost()
                                .small()
                                .label("Open PR in browser")
                                .on_click(move |_, _, cx| cx.open_url(&locator.url())),
                        );
                header = header.child(pr_status::render(&status, status_viewer.clone(), cx));
                if !metadata.body.is_empty() {
                    header = header.child(
                        div()
                            .text_sm()
                            .whitespace_normal()
                            .child(metadata.body.clone()),
                    );
                }
            }
            if let Some(series) = &series {
                header = header.child(render_series(
                    series,
                    &expanded_messages,
                    series_viewer.clone(),
                    muted,
                    cx,
                ));
            } else {
                header = header.child(div().text_xl().whitespace_normal().child(title.to_owned()));
                if !body.is_empty() {
                    header = header.child(div().whitespace_normal().child(body.to_owned()));
                }
                if let Some(metadata) = &metadata {
                    header =
                        header.child(div().text_sm().text_color(muted).child(metadata.clone()));
                }
            }
            header.into_any_element()
        };
        let viewer = cx.entity().downgrade();
        let files = loaded.files.clone();
        let annotations = files
            .iter()
            .filter(|(_, file)| file.notice.is_some())
            .map(|(path, _)| {
                DiffAnnotation::file(SharedString::from(format!("notice:{path}")), path.clone())
            })
            .collect::<Vec<_>>();
        let notices = files.clone();
        let empty_header = loaded.files.is_empty().then(|| expanded(cx));
        let menu_owner = cx.entity().downgrade();
        let menu_focus = self.focus.clone();
        let source = loaded.request.source.clone();
        let root = loaded.root.clone();
        let repository = self.arguments.diff.is_none()
            && self.comparison().is_some_and(|c| c.hosted().is_none());
        let diff = Diff::new(&loaded.state)
            .display_options(self.diff_render_options())
            .colors(crate::appearance::diff_colors(cx))
            .header_context_menu(move |file, menu, _, _| {
                let target = context_menu::FileTarget::new(
                    source.clone(),
                    &root,
                    file.path().to_string(),
                    false,
                    repository,
                );
                let menu = context_menu::file_menu(
                    menu.action_context(menu_focus.clone()),
                    target,
                    menu_owner.clone(),
                );
                context_menu::overview_commands(menu.separator())
            })
            .border_0()
            .render_preamble(move |_, cx| expanded(cx))
            .sticky_headers(true)
            .code_font(theme.mono_font_family.clone(), px(self.code_font_size))
            .font_features(typography::features(self.settings.editor.ligatures))
            .line_number(self.line_numbers)
            .soft_wrap(self.wrapping.inline)
            .annotations(annotations)
            .render_annotation(move |annotation, _, cx| {
                div().p(crate::spacing::edge(cx)).whitespace_normal().child(
                    notices
                        .get(annotation.path().as_ref())
                        .and_then(|file| file.notice.clone())
                        .unwrap_or_default(),
                )
            })
            .render_header_title_suffix(move |document, _, _| {
                div().text_sm().child(
                    files
                        .get(document.path().as_ref())
                        .map(|file| {
                            if file.file.status() == "renamed" || file.file.status() == "copied" {
                                format!("{} from {}", file.file.status(), file.file.before_path())
                            } else {
                                file.file.status().to_owned()
                            }
                        })
                        .unwrap_or_default(),
                )
            })
            .render_header_suffix(move |file, _, _| {
                let viewer = viewer.clone();
                let path = file.path().to_string();
                Button::new(SharedString::from(format!("overview-open:{path}")))
                    .ghost()
                    .small()
                    .label("Open file")
                    .on_click(move |_, window, cx| {
                        let _ = viewer.update(cx, |viewer, cx| {
                            window.focus(&viewer.focus, cx);
                            viewer.select_path(&path, window, cx);
                        });
                    })
            })
            .size_full();
        let state = loaded.state.clone();
        let tracking_viewer = cx.entity().downgrade();
        let tracking = canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                let path = state.read(cx).viewport_file().map(ToString::to_string);
                let Some(viewer) = tracking_viewer.upgrade() else {
                    return;
                };
                if viewer.read(cx).selected_file.is_some()
                    || viewer.read(cx).browser.reading == path
                {
                    return;
                }
                let state_id = state.entity_id();
                let viewer = tracking_viewer.clone();
                window.defer(cx, move |_, cx| {
                    let _ = viewer.update(cx, |viewer, cx| {
                        if viewer.selected_file.is_none()
                            && viewer
                                .overview
                                .current
                                .as_ref()
                                .is_some_and(|loaded| loaded.state.entity_id() == state_id)
                        {
                            viewer.track_overview_file(path, cx);
                        }
                    });
                });
            },
        )
        .absolute()
        .size_full();
        surface
            .children(empty_header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(if loaded.files.is_empty() {
                        div()
                            .p(crate::spacing::space(6.0, cx))
                            .child(if self.is_interdiff() {
                                "No patch changes between these versions."
                            } else {
                                "No changed files."
                            })
                            .into_any_element()
                    } else {
                        diff.into_any_element()
                    })
                    .child(tracking),
            )
            .into_any_element()
    }
}

fn message_parts(commit: Option<&CommitDetails>) -> (&str, &str) {
    let Some(commit) = commit else {
        return ("Changeset overview", "");
    };
    let (title, body) = commit
        .message
        .split_once('\n')
        .unwrap_or((&commit.message, ""));
    let title = if title.trim().is_empty() {
        if commit.working_copy {
            "Working copy"
        } else {
            "No description"
        }
    } else {
        title
    };
    (title, body.trim_matches('\n'))
}

/// The diff prepaints before the comparison bar. Choose its caption here,
/// using the scroll position that will actually be painted in this frame.
struct ComparisonCaption {
    state: Entity<DiffState>,
    label: String,
    subject: String,
    preamble_scrolled: bool,
    content: Option<AnyElement>,
}
impl Element for ComparisonCaption {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("overview-comparison-caption".into())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // Measure the caption that fits the current scroll state, so the
        // selector grows only when the collapsed message is actually shown.
        self.preamble_scrolled = self.state.read(cx).preamble_scrolled();
        let caption = if self.preamble_scrolled {
            format!("{} · {}", self.label, self.subject)
        } else {
            self.label.clone()
        };
        let mut content = div().min_w_0().truncate().child(caption).into_any_element();
        let layout = content.request_layout(window, cx);
        self.content = Some(content);
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let scrolled = self.state.read(cx).preamble_scrolled();
        if scrolled != self.preamble_scrolled {
            // The body publishes this frame's geometry after header layout.
            // Paint its caption now and remeasure its intrinsic width next frame.
            window.request_animation_frame();
        }
        let caption = if scrolled {
            format!("{} · {}", self.label, self.subject)
        } else {
            self.label.clone()
        };
        let mut content = div()
            .debug_selector(|| "comparison-caption".into())
            .w(bounds.size.width)
            .truncate()
            .child(caption)
            .into_any_element();
        content.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        content.prepaint_at(bounds.origin, window, cx);
        self.content = Some(content);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(content) = &mut self.content {
            content.paint(window, cx);
        }
    }
}
impl IntoElement for ComparisonCaption {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn render_series(
    series: &OverviewSeries,
    expanded_messages: &HashSet<String>,
    owner: WeakEntity<Viewer>,
    muted: Hsla,
    cx: &App,
) -> AnyElement {
    let mut list = div()
        .v_flex()
        .gap(crate::spacing::interactive(cx))
        .line_height(relative(1.35));
    list = list.child(
        div()
            .mb(crate::spacing::interactive(cx))
            .text_xl()
            .child("Commits in this comparison"),
    );
    if series.commits.is_empty() {
        list = list.child(div().text_color(muted).child("No commits in this range."));
    }
    for commit in &series.commits {
        let (subject, body) = message_parts(Some(&commit.details));
        let subject = if commit.details.working_copy {
            "Working copy changes"
        } else {
            subject
        };
        let id = commit.details.id.clone();
        let open = expanded_messages.contains(&id);
        let viewer = owner.clone();
        let subject_element = div()
            .id(SharedString::from(format!("series-subject:{id}")))
            .flex_1()
            .min_w_0()
            .whitespace_normal()
            .child(subject.to_owned())
            .when(commit.revision.is_some(), |element| {
                element.cursor_pointer().on_click(move |_, window, cx| {
                    let _ = viewer.update(cx, |viewer, cx| {
                        window.focus(&viewer.focus, cx);
                        viewer.select_revision(&id, window, cx);
                    });
                })
            });
        let mut row = div()
            .h_flex()
            .items_start()
            .gap(crate::spacing::edge(cx))
            .child("•")
            .child(subject_element);
        if !body.is_empty() {
            let id = commit.details.id.clone();
            let viewer = owner.clone();
            row = row.child(
                Button::new(SharedString::from(format!("series-body:{id}")))
                    .ghost()
                    .small()
                    .compact()
                    .icon(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .tooltip(if open {
                        "Hide commit message"
                    } else {
                        "Show commit message"
                    })
                    .on_click(move |_, _, cx| {
                        let _ = viewer.update(cx, |viewer, cx| {
                            if !viewer.overview.expanded_messages.remove(&id) {
                                viewer.overview.expanded_messages.insert(id.clone());
                            }
                            if let Some(loaded) = &viewer.overview.current {
                                loaded
                                    .state
                                    .update(cx, |state, cx| state.remeasure_preamble(cx));
                            }
                            cx.notify();
                        });
                    }),
            );
        }
        list = list.child(row);
        if open && !body.is_empty() {
            list = list.child(
                div()
                    .pl(crate::spacing::space(4.0, cx))
                    .text_sm()
                    .text_color(muted)
                    .whitespace_normal()
                    .child(body.to_owned()),
            );
        }
    }
    list.into_any_element()
}

impl Viewer {
    pub(super) fn overview_action(
        &mut self,
        command: jaywalk::commands::CommandId,
        cx: &mut Context<Self>,
    ) {
        use jaywalk::commands::CommandId;
        let Some(loaded) = &self.overview.current else {
            return;
        };
        // Do not alter retained overview state while a newer comparison is pending.
        if !self
            .comparison()
            .is_some_and(|comparison| comparison.identity() == loaded.request.source)
        {
            return;
        }
        loaded.state.update(cx, |state, cx| match command {
            CommandId::OverviewContext => {
                state.set_context_lines(Some(self.settings.overview.context_lines), cx)
            }
            CommandId::OverviewFull => state.set_context_lines(None, cx),
            CommandId::CollapseOverview | CommandId::ExpandOverview => {
                let paths: Vec<_> = state
                    .files()
                    .iter()
                    .map(|file| file.path().clone())
                    .collect();
                for path in paths {
                    state.set_file_collapsed(&path, command == CommandId::CollapseOverview, cx);
                }
            }
            _ => {}
        });
        cx.notify();
    }
}

#[cfg(test)]
mod option_tests {
    use super::super::interaction_tests::{Fixture, draw, setup};
    use gpui_test::TestAppContext;
    #[gpui_test::test]
    fn file_panel_navigates_overview_and_double_click_opens(cx: &mut TestAppContext) {
        use gpui_test::{MouseButton, MouseDownEvent, MouseUpEvent};
        use jaywalk::{commands::CommandId, settings::FileView};
        let fixture = Fixture::new();
        let lines: String = (0..100).map(|i| format!("line {i}\n")).collect();
        fixture.replace(&lines, &lines.replace("line 50", "changed 50"));
        std::fs::write(fixture.0.join("z-last.txt"), &lines).unwrap();
        let (viewer, cx) = setup(&fixture, cx);
        cx.update(|window, app| {
            viewer.update(app, |v, cx| {
                v.execute_command(CommandId::Overview, window, cx)
            })
        });
        draw(cx);
        for mode in [FileView::List, FileView::Tree] {
            cx.update(|_, app| viewer.update(app, |v, cx| v.set_file_mode(mode, cx)));
            draw(cx);
            let selector = match mode {
                FileView::List => "browser-file:z-last.txt",
                FileView::Tree => "browser-tree:file:z-last.txt",
            };
            let position = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(position, Default::default());
            draw(cx);
            cx.update(|_, app| {
                let v = viewer.read(app);
                assert!(v.selected_file.is_none());
                assert_eq!(v.browser.reading.as_deref(), Some("z-last.txt"));
                assert_eq!(
                    v.overview
                        .current
                        .as_ref()
                        .unwrap()
                        .state
                        .read(app)
                        .viewport_file()
                        .map(|p| p.as_ref()),
                    Some("z-last.txt")
                );
            });
            // Scroll away while the tree retains its selected item. Notifications
            // must not pull the overview back to that selection.
            cx.update(|_, app| {
                viewer.update(app, |v, cx| {
                    v.navigate_overview_file("file.txt", cx);
                })
            });
            draw(cx);
            assert_eq!(
                cx.update(|_, app| viewer.read(app).browser.reading.clone())
                    .as_deref(),
                Some("file.txt")
            );
            let position = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(position, Default::default());
            draw(cx);
            assert_eq!(
                cx.update(|_, app| viewer.read(app).browser.reading.clone())
                    .as_deref(),
                Some("z-last.txt")
            );
            cx.simulate_event(MouseDownEvent {
                position,
                button: MouseButton::Left,
                click_count: 2,
                modifiers: Default::default(),
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position,
                button: MouseButton::Left,
                click_count: 2,
                modifiers: Default::default(),
            });
            draw(cx);
            assert_eq!(
                cx.update(|_, app| viewer.read(app).selected_path())
                    .as_deref(),
                Some("z-last.txt")
            );
            // Once a file is open, a single click still opens another file.
            let selector = match mode {
                FileView::List => "browser-file:file.txt",
                FileView::Tree => "browser-tree:file:file.txt",
            };
            let position = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(position, Default::default());
            draw(cx);
            assert_eq!(
                cx.update(|_, app| viewer.read(app).selected_path())
                    .as_deref(),
                Some("file.txt")
            );
            cx.update(|window, app| {
                viewer.update(app, |v, cx| {
                    v.execute_command(CommandId::Overview, window, cx)
                })
            });
            draw(cx);
        }
    }

    #[gpui_test::test]
    fn overview_filters_keep_files_and_exact_source_selection(cx: &mut TestAppContext) {
        use gpui_kit::component::diff::{DiffLineRange, DiffSide};
        use jaywalk::commands::CommandId;
        let f = Fixture::new();
        let before = "let value = 1; \r\n\n";
        let after = "letvalue=1;\n";
        f.replace(before, after);
        std::fs::write(
            f.0.join(".git/info/exclude"),
            "settings.toml\nkeybindings/\n",
        )
        .unwrap();
        let (viewer, cx) = setup(&f, cx);
        let identity = cx.update(|_, app| viewer.read(app).comparison().unwrap().identity());
        for command in [
            CommandId::WhitespaceAll,
            CommandId::ToggleIgnoreLineEndings,
            CommandId::ToggleIgnoreBlankLines,
            CommandId::Overview,
        ] {
            cx.update(|w, app| viewer.update(app, |v, cx| v.execute_command(command, w, cx)));
            draw(cx);
        }
        let state = cx.update(|_, app| {
            let v = viewer.read(app);
            let loaded = v.overview.current.as_ref().unwrap();
            assert_eq!(loaded.request.source, identity);
            assert_eq!(loaded.files.len(), 1);
            assert_eq!((loaded.additions, loaded.deletions), (0, 0));
            assert!(
                loaded.files["file.txt"]
                    .notice
                    .as_ref()
                    .unwrap()
                    .contains("ignored differences")
            );
            loaded.state.clone()
        });
        cx.update(|_, app| {
            state.update(app, |s, cx| {
                for (side, text) in [(DiffSide::Original, before), (DiffSide::Modified, after)] {
                    s.set_selected_lines(
                        Some(DiffLineRange::new("file.txt", side, 1, usize::MAX)),
                        cx,
                    );
                    assert_eq!(s.selected_text(cx), text);
                }
            })
        });
        cx.update(|w, app| {
            viewer.update(app, |v, cx| {
                v.execute_command(CommandId::WhitespaceExact, w, cx)
            })
        });
        draw(cx);
        cx.update(|_, app| {
            let v = viewer.read(app);
            let loaded = v.overview.current.as_ref().unwrap();
            assert_eq!(loaded.request.source, identity);
            assert!(loaded.additions + loaded.deletions > 0);
        });
    }
}
