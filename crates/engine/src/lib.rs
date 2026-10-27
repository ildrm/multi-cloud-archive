//! Experimental, bounded encrypted archive engine. No source deletion or GC.
#![forbid(unsafe_code)]

mod crypto;
mod pipeline;
mod store;

pub use crypto::{VaultHeader, create_vault, read_header};
pub use pipeline::{
    ArchiveResult, Progress, RecoveryKit, RestoreResult, SnapshotSummary, archive_to,
    list_snapshots, read_kit, restore_from,
};
pub use store::read_stream_bounded;
pub use store::{LocalStore, ObjectLocator, ObjectStore, Origin};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Archive metadata is invalid or exceeds a safety limit")]
    Format,
    #[error("Wrong secret or damaged authenticated data")]
    Integrity,
    #[error("The source changed during archiving; retry with a stable source")]
    SourceChanged,
    #[error("A source is a link, has an unsupported name, or conflicts with another source")]
    UnsafePath,
    #[error("Use a passphrase of at least 12 characters")]
    WeakPassphrase,
    #[error("Cloud operation failed: {0}")]
    Provider(String),
}
