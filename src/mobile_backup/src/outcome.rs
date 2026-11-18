//! The input and output types of one backup run (`backup_core.md` § Types).

use std::io::ErrorKind;
use std::time::Instant;

use sync_core::{BatchRunError, StoreError, SyncError};
use thiserror::Error;

use crate::checkpoint::DbError;

/// The entry point that started the run. A catch-up run always does the visible-id diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    MediaChange,
    CatchUp,
    AppOpen,
    FirstSync,
}

/// `network_policy` of the contract only decides pauses, which E-27/c07 maps, so it is not part of this type yet.
#[derive(Clone, Copy, Debug)]
pub struct RunContext {
    pub trigger: Trigger,
    /// The run ends `Partial` at the first check after this instant.
    pub deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    QuotaExceeded,
    NoNetwork,
    MeteredNetwork,
    PermissionDenied,
    LimitedAccess,
    ServerRejectedName,
    UpgradeRequired,
    AccountNotActive,
    SignedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    AlreadyRunning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupOutcome {
    Completed,
    /// The deadline passed or `cancel()` was called; `remaining` assets still wait.
    Partial {
        remaining: u64,
    },
    Paused(PauseReason),
    Skipped(SkipReason),
}

/// Why a run ended before its work was done; the checkpoint keeps every asset it stood on. E-27/c07 maps these to
/// pauses and the host errors of the contract.
#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error(transparent)]
    Checkpoint(#[from] DbError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// An asset file could not be read.
    #[error("asset source failed ({0:?})")]
    Source(ErrorKind),
    /// The server's folder listing or answer broke a rule of the folder lookup.
    #[error("camera folder unavailable")]
    Folder,
}

impl From<BatchRunError> for RunError {
    fn from(error: BatchRunError) -> Self {
        match error {
            BatchRunError::Sync(error) => Self::Sync(error),
            BatchRunError::Store(error) => Self::Store(error),
        }
    }
}
