//! Application authentication backend. Blocking operations belong on a worker,
//! never in render/input callbacks. The GUI owns one Service for all windows.
pub mod application;
mod browser;
mod model;
mod provider;
pub mod publisher;
mod service;
mod store;

pub use browser::BrowserAttempt;
pub use model::*;
pub use provider::{HostingProvider, HttpProvider};
pub use service::{Service, SignIn, Snapshot};
pub use store::{CredentialStore, OsCredentialStore};

mod workspaces;
pub use workspaces::{Workspace, WorkspaceListing};

pub mod controller;

pub mod preferences;
