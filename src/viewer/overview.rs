//! One coherent overview per comparison, built in the background and reused
//! while visiting files. The unified widget owns folding, selection and copy.
use super::*;
use gpui::component::Sizable;
use gpui::component::diff::DiffAnnotation;
use jaywalk::overview::CommitDetails;
use jaywalk::repository::{ChangedFile, ComparisonIdentity};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
struct Request {
    source: ComparisonIdentity,
    tab_size: usize,
}
struct FileSummary {
    file: ChangedFile,
    notice: Option<String>,
}
struct PreparedOverview {
    commit: Option<CommitDetails>,
    warning: Option<String>,
    files: Arc<BTreeMap<String, FileSummary>>,
    documents: Vec<DiffFile>,
}
impl PreparedOverview {
    fn load(comparison: &Comparison, tab_size: usize) -> Result<Self, String> {
        let snapshot = comparison.overview();
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
            commit: snapshot.commit,
            warning: snapshot.metadata_error,
            files: Arc::new(files),
            documents,
        })
    }
}
struct LoadedOverview {
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
    current: Option<LoadedOverview>,
    task: Option<Task<()>>,
    generation: u64,
    pub(super) error: Option<String>,
}
impl Viewer {
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
        };
        if self.overview.requested.as_ref() == Some(&request) {
            return;
        }
        let comparison = comparison.clone();
        let scope = self.scope_label();
        self.overview.requested = Some(request.clone());
        self.overview.generation += 1;
        let generation = self.overview.generation;
        self.overview.error = None;
        self.begin_loading(LoadingKind::Overview, window, cx);
        self.overview.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { PreparedOverview::load(&comparison, request.tab_size) },
                )
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
                        viewer.overview.current = Some(LoadedOverview {
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
        let surface = div().v_flex().size_full().bg(theme.background);
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
        let (title, body) = loaded
            .commit
            .as_ref()
            .map_or(("Changeset overview", ""), |commit| {
                let (title, body) = commit
                    .message
                    .split_once('\n')
                    .unwrap_or((&commit.message, ""));
                (
                    if title.trim().is_empty() {
                        if commit.working_copy {
                            "Working copy"
                        } else {
                            "No description"
                        }
                    } else {
                        title
                    },
                    body.trim_matches('\n'),
                )
            });
        let metadata = loaded
            .commit
            .as_ref()
            .map(|commit| format!("{} · {}", commit.author, commit.timestamp));
        let state = loaded.state.clone();
        let context = self.settings.overview.context_lines;
        let full = loaded.state.read(cx).context_lines().is_none();
        let mut header = div()
            .id("overview-message")
            .v_flex()
            .gap_2()
            .p_4()
            .max_h(px(self.settings.overview.message_max_height))
            .flex_shrink_0()
            .overflow_y_scroll()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().text_sm().truncate().child(format!(
                        "{} · {} files · +{} −{}",
                        loaded.scope,
                        loaded.files.len(),
                        loaded.additions,
                        loaded.deletions
                    )))
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
            )
            .child(div().text_xl().whitespace_normal().child(title.to_owned()));
        if !body.is_empty() {
            header = header.child(div().whitespace_normal().child(body.to_owned()));
        }
        if let Some(metadata) = metadata {
            header = header.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(metadata),
            );
        }
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
        let diff = Diff::new(&loaded.state)
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
            .child(header)
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
