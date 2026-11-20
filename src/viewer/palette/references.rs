//! Keyboard access to the same captured references used by the graph popovers.
use super::*;
use jaywalk::{branch_review::RefName, history::Reference, repository::ComparisonIdentity};

impl Viewer {
    pub(super) fn reference_choices(&self, kind: PaletteKind) -> Vec<(String, Choice)> {
        let Some(session) = self
            .repository
            .ready()
            .filter(|_| self.page == pages::Page::Comparison && !self.is_hosted_review())
        else {
            return vec![];
        };
        let Ok(history) = &session.history else {
            return vec![];
        };
        let source = Rc::new(session.home.identity());
        if kind == PaletteKind::References {
            return history
                .references
                .references
                .iter()
                .map(|reference| {
                    (
                        reference.display_name(),
                        Choice::Reference {
                            reference: reference.clone(),
                            source: source.clone(),
                        },
                    )
                })
                .collect();
        }
        let mut choices: Vec<_> = history
            .references
            .remotes
            .iter()
            .map(|remote| {
                (
                    remote.clone(),
                    Choice::Remote {
                        remote: Some(remote.clone()),
                        visible: !self.references.visibility.includes(remote),
                        source: source.clone(),
                    },
                )
            })
            .collect();
        if !choices.is_empty() {
            choices.extend(
                [(true, "Show all remotes"), (false, "Hide all remotes")].map(
                    |(visible, label)| {
                        (
                            label.into(),
                            Choice::Remote {
                                remote: None,
                                visible,
                                source: source.clone(),
                            },
                        )
                    },
                ),
            );
        }
        choices
    }

    pub(super) fn reference_choice_status(&self, reference: &Reference) -> String {
        match reference.remote() {
            Some(remote) => format!(
                "Remote: {remote}{}",
                if self.references.visibility.includes(remote) {
                    ""
                } else {
                    " · hidden from graph"
                }
            ),
            None => "Local".into(),
        }
    }

    fn reference_source_is_current(&self, source: &ComparisonIdentity) -> bool {
        self.page == pages::Page::Comparison
            && !self.is_hosted_review()
            && self
                .repository
                .ready()
                .is_some_and(|session| session.home.identity() == *source)
    }

    pub(super) fn apply_remote_choice(
        &mut self,
        remote: Option<String>,
        visible: bool,
        source: &ComparisonIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.reference_source_is_current(source) {
            return;
        }
        if let Some(name) = &remote {
            let exists = self
                .repository
                .ready()
                .and_then(|session| session.history.as_ref().ok())
                .is_some_and(|history| history.references.remotes.contains(name));
            if !exists {
                return;
            }
        }
        let visible = remote
            .as_ref()
            .map_or(visible, |name| !self.references.visibility.includes(name));
        self.set_remote_visibility(remote, visible, window, cx);
        cx.notify();
    }

    pub(super) fn apply_reference_choice(
        &mut self,
        reference: Reference,
        source: &ComparisonIdentity,
        review: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.reference_source_is_current(source) {
            return;
        }
        if review {
            self.review_reference(RefName::from_reference(&reference), window, cx);
        } else {
            let target = reference.target.clone();
            self.references.inspected = Some(reference);
            self.open_reference_target(target, true, window, cx);
        }
    }
}
