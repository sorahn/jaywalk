//! One coherent overview per comparison, built in the background and reused
//! while visiting files. The unified widget owns folding, selection and copy.
use super::*;
use gpui::component::Sizable;
use gpui::component::diff::DiffAnnotation;
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
    fn load(comparison: &Comparison, tab_size: usize, whole_branch: bool) -> Result<Self, String> {
        let mut snapshot = comparison.overview();
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
            let mut parsed = DiffFile::parse(&file.patch)
                .map_err(|error| format!("Could not prepare {}: {error}", file.file.after_path))?;
            if parsed.len() != 1 {
                return Err(format!(
                    "Expected one overview entry for {}",
                    file.file.after_path
                ));
            }
            let document = parsed.remove(0);
            if document.path().as_ref() != file.file.after_path {
                return Err(format!(
                    "Overview path did not match {}",
                    file.file.after_path
                ));
            }
            documents.push(
                document
                    .with_language(file_language(&file.file.after_path))
                    .with_tab_size(tab_size),
            );
            files.insert(
                file.file.after_path.clone(),
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
}
#[derive(Clone)]
pub(super) struct OverviewCheckpoint {
    loaded: LoadedOverview,
    offset: ListOffset,
    horizontal: Pixels,
}
impl Viewer {
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
            series: matches!(
                self.repository.ready().map(|session| &session.scope),
                Some(ComparisonScope::Aggregate)
            ),
        };
        if self.overview.requested.as_ref() == Some(&request) {
            return;
        }
        let comparison = comparison.clone();
        let comparison_operation = comparison.operation.clone();
        let comparison_root = comparison.root.clone();
        let scope = self.scope_label();
        self.overview.requested = Some(request.clone());
        self.overview.generation += 1;
        let generation = self.overview.generation;
        self.overview.error = None;
        self.begin_loading(LoadingKind::Overview, window, cx);
        self.overview.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    PreparedOverview::load(&comparison, request.tab_size, request.series)
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
                        viewer.overview.error = prepared
                            .warning
                            .map(|error| format!("Commit metadata unavailable: {error}"));
                    }
                }
                cx.notify();
            });
        }));
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
            .bg(theme.background);
        if let LoadState::Failed(error) = self.comparison_state() {
            return surface
                .child(
                    div()
                        .p_6()
                        .child(feedback::error_message("overview-error", &error)),
                )
                .into_any_element();
        }
        let Some(loaded) = &self.overview.current else {
            let text = if self.overview.error.is_some() {
                "Overview unavailable"
            } else if self.loading_visible(LoadingKind::Comparison)
                || self.loading_visible(LoadingKind::Overview)
            {
                "Loading overview…"
            } else {
                ""
            };
            return surface.child(div().p_6().child(text)).into_any_element();
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
        let expanded = move || {
            let state = state.clone();
            let mut header = div()
                .id("overview-message")
                .v_flex()
                .gap_2()
                .p_4()
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
                        .gap_2()
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
            if let Some(series) = &series {
                header = header.child(render_series(
                    series,
                    &expanded_messages,
                    series_viewer.clone(),
                    muted,
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
        let empty_header = loaded.files.is_empty().then(&expanded);
        let menu_owner = cx.entity().downgrade();
        let menu_focus = self.focus.clone();
        let source = loaded.request.source.clone();
        let root = loaded.root.clone();
        let repository = self.arguments.diff.is_none();
        let diff = Diff::new(&loaded.state)
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
            .render_preamble(move |_, _| expanded())
            .sticky_headers(true)
            .code_font(theme.mono_font_family.clone(), px(self.code_font_size))
            .font_features(typography::features(self.settings.editor.ligatures))
            .line_number(self.line_numbers)
            .soft_wrap(self.wrapping.inline)
            .annotations(annotations)
            .render_annotation(move |annotation, _, _| {
                div().p_3().whitespace_normal().child(
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
                            if file.file.status == "renamed" || file.file.status == "copied" {
                                format!("{} from {}", file.file.status, file.file.before_path)
                            } else {
                                file.file.status.clone()
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
                            .p_6()
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
        let style = Style {
            size: size(relative(1.).into(), window.line_height().into()),
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
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
        let caption = if self.state.read(cx).preamble_scrolled() {
            format!("{} · {}", self.label, self.subject)
        } else {
            self.label.clone()
        };
        let mut content = div()
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
) -> AnyElement {
    let mut list = div().v_flex().gap_2();
    list = list.child(div().text_xl().child("Commits in this comparison"));
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
            .gap_2()
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
                    .pl_4()
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
