use crate::local_index::IndexError;
use crate::platform_fs::FsError;

/// Any error means the guard released nothing: callers keep every delete pending.
#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error("no such pair")]
    UnknownPair,
    /// The pair has no hold to resolve.
    #[error("the pair is not in a hold")]
    NoSuchHold,
    /// A `root_missing` hold has no held deletes to decide on; it ends when the root passes the check again.
    #[error("the pair is held because its root is missing; it has no deletes to decide on")]
    RootHold,
    /// A paused pair sends and applies nothing, so there is nothing to evaluate.
    #[error("the pair is paused")]
    Paused,
    #[error("the hold detail stored in the index cannot be read")]
    HoldDetailUnreadable,
    #[error("cannot write the root sentinel: {0}")]
    Sentinel(#[from] FsError),
}
