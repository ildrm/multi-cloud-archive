//! Native application services. Secrets stay in the OS keyring, account-scoped.
#![forbid(unsafe_code)]
mod auth;
mod cloud;
mod service;
pub use service::*;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Engine(#[from] cloudweave_engine::Error),
    #[error("Local catalog operation failed")]
    Catalog(#[from] rusqlite::Error),
    #[error("Filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Secure credential storage is unavailable; no plain-token fallback is used")]
    Credential,
    #[error("Authorization failed, was cancelled, or timed out; reconnect the account")]
    Authorization,
    #[error("Provider returned a different account identity; reconnect the original account")]
    IdentityMismatch,
    #[error(
        "The account is not connected on this installation; connect the same provider identity first"
    )]
    AccountMissing,
    #[error("Cloud request failed ({0}); check connectivity and try again")]
    Network(&'static str),
    #[error(
        "Provider returned HTTP {0}; reconnect for 401, check permissions for 403, or retry later for 429/5xx"
    )]
    Http(u16),
    #[error("Invalid or unsupported provider response or configuration")]
    Invalid,
    #[error("Another CloudWeave instance is using this data directory")]
    AlreadyRunning,
    #[error("Choose the local archive storage folder explicitly before restoring a local snapshot")]
    LocalRootRequired,
}
