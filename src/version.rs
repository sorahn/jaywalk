//! Shared version identity for the CLI, About window, and bundle builder.
pub const PACKAGE: &str = env!("CARGO_PKG_VERSION");
pub const DISPLAY: &str = env!("JAYWALK_DISPLAY_VERSION");
pub const REVISION: &str = env!("JAYWALK_BUILD_REVISION");
