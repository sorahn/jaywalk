//! Read-only hosted review discovery. A locator is a navigable name, not proof
//! of immutable published comparison endpoints or locally available objects.
mod cache;
mod client;
pub(crate) use cache::Cache;
pub use cache::Fetched;
mod local;
mod model;
pub(crate) use client::{GITHUB_QUERY, Request, discover};
pub use local::{Association, RemoteRepository, associate, repository_remotes};
pub use model::*;

pub mod recent;

pub mod snapshots;

pub mod review;

pub mod status;
