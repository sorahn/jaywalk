//! The aggregate being reviewed is separate from a selected commit or file.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HomeSource {
    Workspace,
    Opened,
}
impl HomeSource {
    pub(super) fn from_arguments(arguments: &Arguments) -> Self {
        if arguments.diff.is_some() || arguments.base.is_some() || arguments.tip != "@" {
            Self::Opened
        } else {
            Self::Workspace
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Current work",
            Self::Opened => "Opened comparison",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReviewSource {
    Home(HomeSource),
    Target(jaywalk::branch_review::Target),
}
impl ReviewSource {
    pub(super) fn label(&self) -> String {
        match self {
            Self::Home(home) => home.label().into(),
            Self::Target(target) => target.label(),
        }
    }
    pub(super) fn follows_branch(&self) -> bool {
        matches!(
            self,
            Self::Target(jaywalk::branch_review::Target::Branch { .. })
        )
    }
}
#[derive(Clone)]
pub(super) struct ReviewContext {
    pub(super) source: ReviewSource,
    pub(super) aggregate: Rc<Comparison>,
}
impl Viewer {
    pub(super) fn home_arguments(&self) -> Arguments {
        arguments_for_home(&self.arguments, self.home_source)
    }
}

use jaywalk::branch_review::{Catalog, RefName, Target, Tip};
use jaywalk::review_requests::ComparisonRequest;

#[derive(Clone)]
pub(super) enum ReviewChoice {
    Home(HomeSource),
    Tip(Tip),
    Base { tip: Tip, base: RefName },
    Target(Target),
}
#[derive(Default)]
pub(super) struct ReviewPicker {
    pub(super) choices: Vec<(String, ReviewChoice)>,
    catalog: Option<std::sync::Arc<Catalog>>,
    epoch: Option<ComparisonRequest>,
    task: Option<Task<()>>,
    pub(super) pending: Option<ComparisonRequest>,
}
enum Intent {
    Choose,
    Infer(String),
    Base(Tip, bool),
    Target(Target),
}
enum Decision {
    Restore(HomeSource),
    Choices(Vec<(String, ReviewChoice)>),
    Target(Target),
}

impl Viewer {
    pub(super) fn all_review_changes(
        &mut self,
        commit: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let selected = commit.or_else(|| match &session.scope {
            ComparisonScope::Revision {
                commit_id,
                working_copy: false,
                ..
            } => Some(commit_id.clone()),
            ComparisonScope::Evolution { anchor, .. } => Some(anchor.clone()),
            _ => None,
        });
        if let Some(commit) = selected {
            self.begin_review_choice(Intent::Infer(commit), window, cx);
        } else if let ReviewSource::Target(target) = &session.review.source {
            self.open_review_target(target.clone(), window, cx);
        } else if let ReviewSource::Home(home) = session.review.source {
            self.load_home_review(home, window, cx);
        }
    }
    pub(super) fn choose_review(
        &mut self,
        tip: Option<Tip>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_review_choice(
            tip.map_or(Intent::Choose, |tip| Intent::Base(tip, false)),
            window,
            cx,
        );
    }
    pub(super) fn choose_review_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let tip = match &session.review.source {
            ReviewSource::Target(Target::Branch { tip, .. }) => tip.clone(),
            _ => Tip::Commit(session.review.aggregate.tip.clone()),
        };
        self.begin_review_choice(Intent::Base(tip, false), window, cx);
    }
    pub(super) fn review_reference(
        &mut self,
        reference: RefName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_review_choice(Intent::Base(Tip::Reference(reference), true), window, cx);
    }
    pub(super) fn open_review_target(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_review_choice(Intent::Target(target), window, cx);
    }
    fn begin_review_choice(&mut self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        if self.arguments.diff.is_some() {
            return;
        }
        let home = session.home.as_ref().clone();
        let context_source = session.review.source.clone();
        let context_base = session.review.aggregate.base.clone();
        let context_tip = session.review.aggregate.tip.clone();
        let epoch = self.requests.comparison();
        self.cancel_live_refresh();
        self.review_picker.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let catalog = Catalog::capture(&home)?;
                    let decision = match intent {
                        Intent::Choose => {
                            let mut choices = vec![
                                (
                                    "Current work".into(),
                                    ReviewChoice::Home(HomeSource::Workspace),
                                ),
                                (
                                    "Opened comparison".into(),
                                    ReviewChoice::Home(HomeSource::Opened),
                                ),
                            ];
                            choices.extend(catalog.references.iter().map(|reference| {
                                (
                                    reference.display_name(),
                                    ReviewChoice::Tip(Tip::Reference(RefName::from_reference(
                                        reference,
                                    ))),
                                )
                            }));
                            if let ReviewSource::Target(target) = &context_source {
                                choices.insert(
                                    0,
                                    (
                                        format!("Current review · {}", target.label()),
                                        ReviewChoice::Target(target.clone()),
                                    ),
                                );
                            }
                            Decision::Choices(choices)
                        }
                        Intent::Base(tip, automatic) => base_decision(&catalog, tip, automatic),
                        Intent::Target(target) => Decision::Target(target),
                        Intent::Infer(commit) => {
                            if catalog.contains(&context_base, &context_tip, &commit)? {
                                match context_source {
                                    ReviewSource::Home(home) => Decision::Restore(home),
                                    ReviewSource::Target(target) => Decision::Target(target),
                                }
                            } else {
                                let candidates = catalog.candidates(&commit)?;
                                if candidates.len() == 1 {
                                    base_decision(&catalog, candidates[0].clone(), true)
                                } else {
                                    let candidates = if candidates.is_empty() {
                                        vec![Tip::Commit(commit)]
                                    } else {
                                        candidates
                                    };
                                    Decision::Choices(
                                        candidates
                                            .into_iter()
                                            .map(|tip| (tip.label(), ReviewChoice::Tip(tip)))
                                            .collect(),
                                    )
                                }
                            }
                        }
                    };
                    Ok::<_, String>((catalog, decision))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != epoch {
                    return;
                }
                match result {
                    Err(error) => {
                        viewer.live.message = Some(format!("Review unchanged: {error}"));
                        cx.notify();
                    }
                    Ok((catalog, decision)) => {
                        let catalog = std::sync::Arc::new(catalog);
                        match decision {
                            Decision::Restore(home) => viewer.load_home_review(home, window, cx),
                            Decision::Target(target) => {
                                viewer.resolve_review(catalog, target, window, cx)
                            }
                            Decision::Choices(choices) => {
                                viewer.show_review_choices(catalog, choices, epoch, window, cx)
                            }
                        }
                    }
                }
            });
        }));
    }
    fn show_review_choices(
        &mut self,
        catalog: std::sync::Arc<Catalog>,
        choices: Vec<(String, ReviewChoice)>,
        epoch: ComparisonRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.review_picker.choices = choices;
        self.review_picker.catalog = Some(catalog);
        self.review_picker.epoch = Some(epoch);
        self.open_palette(palette::PaletteKind::Reviews, window, cx);
    }
    pub(super) fn apply_review_choice(
        &mut self,
        choice: ReviewChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.review_picker.epoch != Some(self.requests.comparison()) {
            return;
        }
        let Some(catalog) = self.review_picker.catalog.clone() else {
            return;
        };
        match choice {
            ReviewChoice::Home(home) => self.load_home_review(home, window, cx),
            ReviewChoice::Target(target) => self.resolve_review(catalog, target, window, cx),
            ReviewChoice::Base { tip, base } => {
                self.resolve_review(catalog, Target::Branch { tip, base }, window, cx)
            }
            ReviewChoice::Tip(tip) => match base_decision(&catalog, tip, true) {
                Decision::Target(target) => self.resolve_review(catalog, target, window, cx),
                Decision::Choices(choices) => self.show_review_choices(
                    catalog,
                    choices,
                    self.requests.comparison(),
                    window,
                    cx,
                ),
                Decision::Restore(_) => {}
            },
        }
    }
    fn resolve_review(
        &mut self,
        catalog: std::sync::Arc<Catalog>,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint_navigation(cx);
        self.remember_reading(cx);
        self.cancel_live_refresh();
        self.invalidate_comparison_request();
        let epoch = self.requests.comparison();
        self.review_picker.pending = Some(epoch);
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    catalog
                        .resolve(&target)
                        .map(|comparison| (comparison, ReviewSource::Target(target)))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != epoch {
                    return;
                }
                viewer.review_picker.pending = None;
                viewer.finish_loading(LoadingKind::Comparison);
                match result {
                    Ok((comparison, source)) => {
                        viewer.install_review(comparison, source, window, cx)
                    }
                    Err(error) => {
                        viewer.live.message = Some(format!("Review unchanged: {error}"));
                        cx.notify();
                    }
                }
            });
        }));
    }
    pub(super) fn load_home_review(
        &mut self,
        home: HomeSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.arguments.diff.is_some() && home == HomeSource::Workspace {
            return;
        }
        let arguments = arguments_for_home(&self.arguments, home);
        self.checkpoint_navigation(cx);
        self.remember_reading(cx);
        self.cancel_live_refresh();
        self.invalidate_comparison_request();
        let epoch = self.requests.comparison();
        self.review_picker.pending = Some(epoch);
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { arguments.comparison() })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != epoch {
                    return;
                }
                viewer.review_picker.pending = None;
                viewer.finish_loading(LoadingKind::Comparison);
                match result {
                    Ok(comparison) => {
                        viewer.install_review(comparison, ReviewSource::Home(home), window, cx)
                    }
                    Err(error) => {
                        viewer.live.message = Some(format!("Review unchanged: {error}"));
                        cx.notify();
                    }
                }
            });
        }));
    }
    fn install_review(
        &mut self,
        comparison: Comparison,
        source: ReviewSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember_reading(cx);
        self.remember_evolution_header();
        self.evolution.clear();
        self.file_evolution.clear();
        self.active_evolution = evolution::EvolutionKind::Change;
        self.resume_live_refresh();
        self.navigation.pinned = matches!(
            source,
            ReviewSource::Target(Target::MergedBranch { .. } | Target::MergeResult { .. })
        );
        let comparison = Rc::new(comparison);
        if let LoadState::Loaded(session) = &mut self.repository {
            if let ReviewSource::Home(home) = source {
                self.home_source = home;
                session.home = comparison.clone();
            }
            session.review = ReviewContext {
                source,
                aggregate: comparison.clone(),
            };
            session.scope = ComparisonScope::Aggregate;
            session.comparison = LoadState::Loaded(comparison);
            session.revision_comparison = None;
        }
        self.rebuild_browser(cx);
        self.follow_open_file(window, cx);
        self.reload_history(None, window, cx);
        cx.notify();
    }
}
fn base_decision(catalog: &Catalog, tip: Tip, automatic: bool) -> Decision {
    if automatic
        && let Some(base) = catalog.default_base()
        && tip != Tip::Reference(base.clone())
    {
        return Decision::Target(Target::Branch {
            tip,
            base: base.clone(),
        });
    }
    Decision::Choices(
        catalog
            .references
            .iter()
            .map(|reference| {
                let base = RefName::from_reference(reference);
                (
                    format!("{} · based on {}", tip.label(), base.display()),
                    ReviewChoice::Base {
                        tip: tip.clone(),
                        base,
                    },
                )
            })
            .collect(),
    )
}
fn arguments_for_home(original: &Arguments, home: HomeSource) -> Arguments {
    let mut arguments = original.clone();
    if home == HomeSource::Workspace {
        arguments.base = None;
        arguments.tip = "@".into();
        arguments.diff = None;
    }
    arguments
}

#[cfg(test)]
mod tests {
    use super::HomeSource;
    use crate::cli::Arguments;
    use clap::Parser;
    #[test]
    fn workspace_return_does_not_overwrite_the_original_opened_range() {
        let original =
            Arguments::try_parse_from(["jw", ".", "--base", "main", "--tip", "feature"]).unwrap();
        let workspace = super::arguments_for_home(&original, HomeSource::Workspace);
        assert!(workspace.base.is_none());
        assert_eq!(workspace.tip, "@");
        let opened = super::arguments_for_home(&original, HomeSource::Opened);
        assert_eq!(opened.base.as_deref(), Some("main"));
        assert_eq!(opened.tip, "feature");
        assert_eq!(original.base, opened.base);
        assert_eq!(original.tip, opened.tip);
    }
    #[test]
    fn explicit_ranges_and_external_inputs_remain_distinct_from_current_work() {
        for arguments in [
            vec!["jw", "."],
            vec!["jw", ".", "--base", "main", "--tip", "feature"],
            vec!["jw", "--diff", "before", "after"],
        ] {
            let parsed = Arguments::try_parse_from(&arguments).unwrap();
            assert_eq!(
                HomeSource::from_arguments(&parsed),
                if arguments.len() == 2 {
                    HomeSource::Workspace
                } else {
                    HomeSource::Opened
                }
            );
        }
    }
}
