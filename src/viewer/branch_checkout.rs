//! Persistent checkout stays inside the palette and preserves the captured review.
use super::*;
use jaywalk::{
    branch_review::{RefName, Target, Tip},
    local_review::checkout::branches::{self, Branch, Catalog},
};
use std::{path::PathBuf, sync::Arc};
#[derive(Clone)]
enum Intent {
    Named(RefName, String),
    Published(Arc<jaywalk::hosting::review::Metadata>),
    Current(String),
}
enum Prepared {
    Branches(Arc<Catalog>),
    CheckedOut(String),
}
#[derive(Default)]
pub(super) struct State {
    pub(super) repositories: Vec<PathBuf>,
    pub(super) catalog: Option<Arc<Catalog>>,
    intent: Option<Intent>,
    source: Option<jaywalk::repository::ComparisonIdentity>,
}
impl Viewer {
    fn checkout_intent(&self) -> Result<Intent, String> {
        let session = self
            .repository
            .ready()
            .ok_or("Open a branch review first")?;
        match &session.review.source {
            review_context::ReviewSource::Hosted(metadata) => {
                Ok(Intent::Published(metadata.clone()))
            }
            review_context::ReviewSource::Target(Target::Branch {
                tip: Tip::Reference(reference),
                ..
            }) => Ok(Intent::Named(
                reference.clone(),
                session.review.aggregate.tip().into(),
            )),
            review_context::ReviewSource::Home(review_context::HomeSource::Workspace)
                if session.review.aggregate.has_git_context() =>
            {
                Ok(Intent::Current(
                    session.review.aggregate.recorded_tip().into(),
                ))
            }
            _ => Err("This comparison has no named branch; use Checkout branch…".into()),
        }
    }
    pub(super) fn choose_branch_checkout(
        &mut self,
        this_branch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let intent = if this_branch {
            match self.checkout_intent() {
                Ok(intent) => Some(intent),
                Err(error) => {
                    self.report_open_error(error, cx);
                    return;
                }
            }
        } else {
            None
        };
        let published = self
            .repository
            .ready()
            .and_then(|session| match &session.review.source {
                review_context::ReviewSource::Hosted(metadata) => Some(metadata.clone()),
                _ => None,
            });
        let source = self.local_source();
        self.open_palette(palette::PaletteKind::CheckoutBranches, window, cx);
        self.local_actions.branch = State {
            intent,
            source: source.clone(),
            ..Default::default()
        };
        if let Some(metadata) = published {
            let candidates = crate::windows::repository_candidates(None, cx);
            let (serial, control) = self.begin_local_action(
                "Finding repositories for checkout… · Escape cancels",
                window,
                cx,
            );
            self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
                let result = cx
                    .background_spawn(async move {
                        jaywalk::hosting::associate(&metadata.locator, &candidates, &control)
                            .map_err(|error| error.to_string())
                    })
                    .await;
                let _ = viewer.update_in(cx, |viewer, window, cx| {
                    if viewer.local_actions.serial != serial || viewer.local_source() != source {
                        return;
                    }
                    viewer.local_actions.control = None;
                    viewer.set_palette_loading(false, window, cx);
                    match result {
                        Ok(found) if found.matches.len() == 1 => {
                            viewer.choose_checkout_repository(found.matches[0].clone(), window, cx)
                        }
                        Ok(found) => {
                            let empty = found.matches.is_empty();
                            viewer.local_actions.branch.repositories = found.matches;
                            viewer.local_actions.message = Some(if empty {
                                "No matching known repository; open the local repository first"
                                    .into()
                            } else {
                                "Choose a matching known repository · Checkout is permanent".into()
                            });
                        }
                        Err(error) => viewer.local_actions.message = Some(error),
                    }
                    cx.notify();
                });
            }));
        } else if let Some(root) = self.repository_directory() {
            self.choose_checkout_repository(root, window, cx);
        } else {
            self.local_actions.branch.repositories =
                crate::windows::repository_candidates(None, cx);
            self.local_actions.message =
                Some("Choose a known repository · Checkout is permanent".into());
            cx.notify();
        }
    }
    pub(super) fn choose_checkout_repository(
        &mut self,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let expected = self.local_actions.branch.source.clone();
        if self.local_source() != expected {
            return;
        }
        self.local_actions.branch.repositories.clear();
        let intent = self.local_actions.branch.intent.clone();
        let receipts = match self.receipts_directory() {
            Ok(path) => path,
            Err(error) => {
                self.local_actions.message = Some(error);
                return;
            }
        };
        let message = if intent.is_some() {
            "Preparing review branch checkout… · Escape cancels before switching"
        } else {
            "Loading local and remote branches… · Escape cancels"
        };
        let (serial, control) = self.begin_local_action(message, window, cx);
        let fetch_missing = self.settings.local_review.fetch_missing;
        let pins_review = matches!(intent, Some(Intent::Named(_, _) | Intent::Published(_)));
        let previous_pin = pins_review.then_some(self.navigation.pinned);
        if pins_review {
            self.cancel_live_refresh();
            self.navigation.pinned = true;
        }
        self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let result = cx
                .background_spawn(async move {
                    if let Some(Intent::Published(metadata)) = intent {
                        return branches::checkout_published(
                            &root,
                            &metadata,
                            fetch_missing,
                            &receipts,
                            &control,
                        )
                        .map(Prepared::CheckedOut);
                    }
                    let catalog = Catalog::capture(&root, &control)?;
                    let branch = match intent {
                        Some(Intent::Named(reference, commit)) => {
                            Some(catalog.reviewed(&reference, &commit)?)
                        }
                        Some(Intent::Current(commit)) => Some(catalog.current_branch_at(&commit)?),
                        _ => None,
                    };
                    match branch {
                        Some(branch) => branches::checkout(&catalog, &branch, &receipts, &control)
                            .map(Prepared::CheckedOut),
                        None => Ok(Prepared::Branches(Arc::new(catalog))),
                    }
                })
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                viewer.finish_branch_checkout(serial, expected, previous_pin, result, window, cx);
            });
        }));
    }
    fn finish_branch_checkout(
        &mut self,
        serial: u64,
        expected: Option<jaywalk::repository::ComparisonIdentity>,
        previous_pin: Option<bool>,
        result: Result<Prepared, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.local_actions.serial != serial || self.local_source() != expected {
            return;
        }
        self.local_actions.control = None;
        self.set_palette_loading(false, window, cx);
        match result {
            Ok(Prepared::Branches(catalog)) => {
                self.local_actions.branch.catalog = Some(catalog);
                self.local_actions.message = Some(
                    "Local branches first · Enter checks out · Checkout stays after closing".into(),
                );
            }
            Ok(Prepared::CheckedOut(message)) => {
                self.close_palette(window, cx);
                self.live.notice = Some(live::LiveNotice::Diagnostic(message));
            }
            Err(error) => {
                if let Some(pinned) = previous_pin {
                    self.navigation.pinned = pinned;
                }
                self.local_actions.message =
                    Some(format!("Checkout refused: {error} · Escape closes"));
            }
        }
        cx.notify();
    }
    pub(super) fn checkout_selected_branch(
        &mut self,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.local_source() != self.local_actions.branch.source {
            return;
        }
        let Some(catalog) = self.local_actions.branch.catalog.clone() else {
            return;
        };
        let expected = self.local_source();
        let receipts = match self.receipts_directory() {
            Ok(path) => path,
            Err(error) => {
                self.local_actions.message = Some(error);
                return;
            }
        };
        let previous_pin = (!branch.current()).then_some(self.navigation.pinned);
        if previous_pin.is_some() {
            self.cancel_live_refresh();
            self.navigation.pinned = true;
        }
        let (serial, control) =
            self.begin_local_action("Checking out branch… · Review stays pinned", window, cx);
        self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let result = cx
                .background_spawn(async move {
                    branches::checkout(&catalog, &branch, &receipts, &control)
                        .map(Prepared::CheckedOut)
                })
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                viewer.finish_branch_checkout(serial, expected, previous_pin, result, window, cx);
            });
        }));
    }
}
