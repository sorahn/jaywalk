use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
    Bitbucket,
}
impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Bitbucket => "bitbucket",
        }
    }
    pub(crate) fn redirect(self) -> &'static str {
        match self {
            Self::Github => "http://127.0.0.1:43762/callback",
            Self::Bitbucket => "http://127.0.0.1:43761/callback",
        }
    }
}

/// Subject is the provider's immutable account ID, never its mutable display name.
/// The first adapters support github.com and bitbucket.org only.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct AccountKey {
    pub provider: Provider,
    pub subject: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub key: AccountKey,
    pub label: String,
}
impl Identity {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.key.subject.is_empty()
            || self.label.is_empty()
            || self.key.subject.len() > 256
            || self.label.len() > 256
            || self
                .key
                .subject
                .chars()
                .chain(self.label.chars())
                .any(char::is_control)
        {
            return Err(Error::InvalidResponse);
        }
        Ok(())
    }
}

/// Explicit exposure; formatting is redacted even in assertion failures.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);
impl Secret {
    pub fn new(value: String) -> Result<Self, Error> {
        if value.is_empty() || value.len() > 16384 || value.chars().any(char::is_control) {
            return Err(Error::InvalidResponse);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Developer/publisher credentials remain separate from individual account grants.
/// Both current browser adapters require client authentication. Provisioning for
/// distributed builds is deliberately not decided by this type.
#[derive(Clone, Serialize, Deserialize)]
pub struct ClientRegistration {
    pub provider: Provider,
    pub client_id: String,
    pub secret: Secret,
}
impl ClientRegistration {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.client_id.trim().is_empty()
            || self.client_id.len() > 256
            || self.client_id.chars().any(char::is_control)
        {
            return Err(Error::Registration);
        }
        Secret::new(self.secret.expose().into()).map(|_| ())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access: Secret,
    pub refresh: Option<Secret>,
    /// Absolute expiry survives application restarts. None means provider-issued
    /// non-expiring; missing/invalid expiry is not silently converted to None.
    pub expires_at: Option<u64>,
}
impl Tokens {
    pub(crate) fn usable(&self, now: u64) -> bool {
        self.expires_at
            .is_none_or(|expires| expires > now.saturating_add(30))
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        Secret::new(self.access.expose().into())?;
        if let Some(refresh) = &self.refresh {
            Secret::new(refresh.expose().into())?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RefreshState {
    Ready,
    Pending,
    Blocked,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub identity: Identity,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<super::Workspace>,
    pub tokens: Tokens,
    pub(crate) refresh_state: RefreshState,
}
impl StoredSession {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.identity.validate()?;
        if self.client_id.trim().is_empty() || self.client_id.len() > 256 {
            return Err(Error::Registration);
        }
        self.tokens.validate()?;
        if let Some(workspace) = &self.workspace {
            workspace.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    TimedOut,
    Denied,
    CallbackBusy,
    BrowserUnavailable,
    StoreUnavailable,
    StoreInvalid,
    Persistence,
    Network,
    RateLimited,
    InvalidResponse,
    Registration,
    NeedsSignIn,
    RefreshUncertain,
    IdentityMismatch,
    NotConnected,
    Busy,
    Unsupported,
    NoAccess,
    NotFound,
    LocalRepository,
    DiscoveryStorage,
    ContentLimit,
    Superseded,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "Sign-in cancelled",
            Self::TimedOut => "Request timed out; try again",
            Self::Denied => "Authorization was declined",
            Self::CallbackBusy => {
                "The sign-in callback port is in use; finish the other sign-in first"
            }
            Self::BrowserUnavailable => "Could not open the browser; try Open browser again",
            Self::StoreUnavailable => {
                "Credential store is unavailable or locked; unlock it and try again"
            }
            Self::StoreInvalid => {
                "Stored account data is invalid; recover the credential store before retrying"
            }
            Self::Persistence => {
                "New credentials could not be saved; unlock the credential store before retrying"
            }
            Self::Network => "Could not reach the provider; the saved account has been retained",
            Self::RateLimited => "Provider rate limit reached; try again later",
            Self::InvalidResponse => "Provider returned an unsupported response",
            Self::Registration => "The provider app registration needs developer setup",
            Self::NeedsSignIn => "Authorization expired or was revoked; reconnect this account",
            Self::RefreshUncertain => {
                "A refresh may have been interrupted; reconnect instead of replaying the old token"
            }
            Self::IdentityMismatch => {
                "The signed-in account differs from the account being reconnected"
            }
            Self::NotConnected => "This account is not connected",
            Self::NoAccess => "This account or app does not have access to that request",
            Self::NotFound => "Pull request not found or unavailable to this account",
            Self::ContentLimit => "Review data exceeds the configured content/file limit",
            Self::DiscoveryStorage => "Could not read or save recent pull requests",
            Self::LocalRepository => "Could not read local repository remotes",
            Self::Unsupported => "This provider does not support this action",
            Self::Busy => "Another operation for this account or provider is already running",
            Self::Superseded => "This request belongs to an older account session",
        })
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Stored,
    Checking,
    Connected,
    Refreshing,
    NeedsSignIn,
    Unavailable(Error),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountView {
    pub generation: u64,
    pub identity: Identity,
    pub workspace: Option<super::Workspace>,
    pub state: ConnectionState,
}

/// Cancellation is local to an operation/waiter, never a process-wide switch.
#[derive(Clone)]
pub struct Control {
    state: Arc<AtomicU8>,
    deadline: Instant,
}
impl Control {
    pub fn new(timeout: Duration) -> Self {
        Self {
            state: Arc::new(AtomicU8::new(0)),
            deadline: Instant::now()
                .checked_add(timeout)
                .unwrap_or_else(Instant::now),
        }
    }
    /// Returns false once a credential write has crossed its commit point.
    /// This touches only an atomic and is safe in UI callbacks/Drop handlers.
    pub fn cancel(&self) -> bool {
        match self
            .state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(state) => state == 1,
        }
    }
    pub fn is_committing(&self) -> bool {
        self.state.load(Ordering::Acquire) == 2
    }
    pub(crate) fn commit(&self) -> Result<(), Error> {
        self.check()?;
        self.state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| Error::Cancelled)
    }
    pub fn check(&self) -> Result<(), Error> {
        match self.state.load(Ordering::Acquire) {
            1 => Err(Error::Cancelled),
            2 => Ok(()),
            _ if Instant::now() >= self.deadline => Err(Error::TimedOut),
            _ => Ok(()),
        }
    }
    pub(crate) fn remaining(&self) -> Result<Duration, Error> {
        self.check()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
