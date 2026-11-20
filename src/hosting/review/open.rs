//! Acquisition order shared by normal PR opens. Published endpoints/paths are
//! always resolved first; successful local acquisition replaces only file metadata.
use super::*;
use crate::local_review::{Mode, Workspace, checkout};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenStage {
    Account,
    Manifest,
    Matching,
    Objects,
    FileMetadata,
    Verify,
    Checkout,
    HostedFallback,
    Ready,
}
impl OpenStage {
    pub fn message(self) -> &'static str {
        match self {
            Self::Account => "Checking account…",
            Self::Manifest => "Loading published PR comparison…",
            Self::Matching => "Finding a local repository…",
            Self::Objects => "Acquiring pinned Git objects…",
            Self::FileMetadata => "Loading file metadata…",
            Self::Verify => "Verifying published PR revision…",
            Self::Checkout => "Preparing clean review checkout…",
            Self::HostedFallback => "Local review unavailable; loading hosted metadata…",
            Self::Ready => "Opening review…",
        }
    }
}
pub struct LocalRequest<'a> {
    pub candidates: &'a [PathBuf],
    pub options: crate::local_review::Options,
    pub receipts: &'a Path,
}
pub struct OpenOptions<'a> {
    pub hosting: Options,
    pub limit: TextLimit,
    pub local: Option<LocalRequest<'a>>,
    pub progress: &'a dyn Fn(OpenStage),
}
struct Timings<'a> {
    start: Instant,
    phase_start: Instant,
    phase: OpenStage,
    stages: Vec<(OpenStage, u128)>,
    requests: &'a Cell<usize>,
    progress: &'a dyn Fn(OpenStage),
    result: &'static str,
}
impl Timings<'_> {
    fn enter(&mut self, stage: OpenStage) {
        self.stages
            .push((self.phase, self.phase_start.elapsed().as_millis()));
        self.phase_start = Instant::now();
        self.phase = stage;
        (self.progress)(stage);
    }
}
impl Drop for Timings<'_> {
    fn drop(&mut self) {
        self.stages
            .push((self.phase, self.phase_start.elapsed().as_millis()));
        let stages = self
            .stages
            .iter()
            .map(|(stage, ms)| format!("{stage:?}={ms}ms"))
            .collect::<Vec<_>>()
            .join(" ");
        // No account IDs, repository paths, URLs, commit IDs or tokens in diagnostics.
        eprintln!(
            "PR open: result={} total_ms={} api_requests={} {}",
            self.result,
            self.start.elapsed().as_millis(),
            self.requests.get(),
            stages
        );
    }
}
struct LocalPrepared {
    workspace: Arc<Workspace>,
    snapshot: Option<checkout::Snapshot>,
    data: Data,
}
fn prepare_local(
    manifest: &Manifest,
    local: &LocalRequest<'_>,
    control: &Control,
    timings: &mut Timings<'_>,
) -> Result<LocalPrepared, String> {
    timings.enter(OpenStage::Matching);
    let association =
        crate::hosting::associate(&manifest.metadata.locator, local.candidates, control)
            .map_err(|e| e.to_string())?;
    let known =
        match association.matches.as_slice() {
            [known] if !association.truncated => known,
            _ => return Err(if association.truncated {
                "Repository search is limited to 50 paths; use Review PR locally to choose a match"
            } else if association.matches.is_empty() {
                "No matching known repository"
            } else {
                "Multiple matching repositories; use Review PR locally to choose one"
            }
            .into()),
        };
    let snapshot = if local.options.mode == Mode::CheckoutIfClean {
        Some(checkout::preflight(known, control)?)
    } else {
        None
    };
    timings.enter(OpenStage::Objects);
    let workspace = Workspace::prepare(known, &manifest.metadata, local.options, control)?;
    timings.enter(OpenStage::FileMetadata);
    let data = manifest.local(&workspace, control)?;
    Ok(LocalPrepared {
        workspace,
        snapshot,
        data,
    })
}
pub(crate) fn open(
    access: Access,
    locator: &Locator,
    options: OpenOptions<'_>,
    control: &Control,
) -> Result<Review, Error> {
    open_with_request(access.clone(), locator, options, control, |r| {
        access.request(r, control)
    })
}
pub(super) fn open_with_request(
    access: Access,
    locator: &Locator,
    options: OpenOptions<'_>,
    control: &Control,
    mut transport: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Review, Error> {
    let requests = Cell::new(0);
    let mut request = |r| {
        requests.set(requests.get() + 1);
        transport(r)
    };
    (options.progress)(OpenStage::Manifest);
    let mut timings = Timings {
        start: Instant::now(),
        phase_start: Instant::now(),
        phase: OpenStage::Manifest,
        stages: Vec::new(),
        requests: &requests,
        progress: options.progress,
        result: "failed",
    };
    let manifest = resolve_manifest(locator, options.hosting, control, &mut request)?;
    let mut fallback = None;
    if let Some(local) = options
        .local
        .as_ref()
        .filter(|local| local.options.mode != Mode::Hosted)
    {
        let prepared = prepare_local(&manifest, local, control, &mut timings);
        control.check()?;
        match prepared {
            Ok(prepared) => {
                let contents = Contents::new_local(
                    prepared.data,
                    access.clone(),
                    options.hosting,
                    options.limit,
                    prepared.workspace.clone(),
                );
                match contents.comparison() {
                    Ok(comparison) => {
                        // Verify after object acquisition/metadata, before modifying a checkout.
                        timings.enter(OpenStage::Verify);
                        manifest.verify(control, &mut request)?;
                        let checkout = if let Some(snapshot) = prepared.snapshot {
                            timings.enter(OpenStage::Checkout);
                            checkout::start(&prepared.workspace, local.receipts, &snapshot, control)
                                .map(Some)
                        } else {
                            Ok(None)
                        };
                        control.check()?;
                        match checkout {
                            Ok(checkout) => {
                                let notice = local_notice(local.options.mode, checkout.as_deref());
                                timings.result = "local";
                                timings.enter(OpenStage::Ready);
                                return Ok(Review {
                                    comparison,
                                    checkout,
                                    local_notice: Some(notice),
                                });
                            }
                            Err(error) => fallback = Some(error),
                        }
                    }
                    Err(error) => fallback = Some(error),
                }
            }
            Err(error) => fallback = Some(error),
        }
    }
    control.check()?;
    timings.enter(if fallback.is_some() {
        OpenStage::HostedFallback
    } else {
        OpenStage::FileMetadata
    });
    let data = manifest.hosted(control, &mut request)?;
    timings.enter(OpenStage::Verify);
    manifest.verify(control, &mut request)?;
    let comparison = Contents::new(data, access, options.hosting, options.limit)
        .comparison()
        .map_err(|_| Error::InvalidResponse)?;
    control.check()?;
    timings.result = "hosted";
    timings.enter(OpenStage::Ready);
    Ok(Review {
        comparison,
        checkout: None,
        local_notice: fallback
            .map(|error| format!("Local review unavailable: {error} · Hosted snapshot retained")),
    })
}
