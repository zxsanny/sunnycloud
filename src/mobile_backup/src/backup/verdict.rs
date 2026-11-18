//! What a sync-core error means for the loop: the table of `backup_core.md` § Wire errors to checkpoint states and
//! pauses. A pause ends the run, an end state closes one asset and the loop goes on, anything else is returned.

use std::io::ErrorKind;

use sc_clock::SystemClock;
use sync_core::{BatchRunError, SkippedFile, StoreError, SyncError};
use tracing::warn;

use super::Backup;
use crate::checkpoint::{AssetEnd, AssetStore, DbError, StoredError};
use crate::outcome::{PauseReason, RunError};
use crate::source::SourceError;

/// Why the loop stops before its work is done.
pub(super) enum Halt {
    Pause(PauseReason),
    Error(RunError),
}

impl From<RunError> for Halt {
    fn from(error: RunError) -> Self {
        Self::Error(error)
    }
}

impl From<SourceError> for Halt {
    fn from(error: SourceError) -> Self {
        Self::Error(error.into())
    }
}

impl From<StoreError> for Halt {
    fn from(error: StoreError) -> Self {
        Self::Error(error.into())
    }
}

impl From<DbError> for Halt {
    fn from(error: DbError) -> Self {
        Self::Error(error.into())
    }
}

enum Verdict {
    Pause(PauseReason),
    End(AssetEnd),
    Fatal,
}

fn verdict(error: &SyncError) -> Verdict {
    match error {
        SyncError::QuotaExceeded | SyncError::FreeSpaceFloor => {
            Verdict::Pause(PauseReason::QuotaExceeded)
        }
        SyncError::AccountNotActive => Verdict::Pause(PauseReason::AccountNotActive),
        SyncError::UpgradeRequired { .. } => Verdict::Pause(PauseReason::UpgradeRequired),
        SyncError::SizeTooLarge => Verdict::End(AssetEnd::Size),
        SyncError::InvalidName => Verdict::End(AssetEnd::Name),
        // The original is not there (deleted, or no longer granted): the host's `OpenError::Gone` seen later.
        SyncError::Io {
            kind: ErrorKind::NotFound | ErrorKind::PermissionDenied,
        } => Verdict::End(AssetEnd::Gone),
        SyncError::SourceChanged
        | SyncError::Io { .. }
        | SyncError::BlocklistMismatch
        | SyncError::Unexpected { .. } => Verdict::End(AssetEnd::Failed),
        // `Unauthorized` is the contract's `TokenRejected`; the network and rate-limit errors are the host's to
        // handle. A missing folder is recovered by the pipeline before it gets here. All of them end the run with
        // the error and keep the asset pending.
        _ => Verdict::Fatal,
    }
}

/// The text `status` shows, and the server's request id when the error carries one.
fn describe(error: &SyncError) -> (String, Option<String>) {
    match error {
        SyncError::UpgradeRequired { message, .. } => (message.clone(), None),
        SyncError::Unexpected { request_id, .. } => (error.to_string(), request_id.clone()),
        _ => (error.to_string(), None),
    }
}

/// Stores the last error with a correlation id: the server's request id, or a fresh id that the log line carries
/// too, so the status and the log of one failure can be matched.
pub(crate) fn record_error(
    assets: &AssetStore,
    message: String,
    request_id: Option<String>,
) -> Result<(), DbError> {
    let correlation_id =
        request_id.unwrap_or_else(|| sc_ids::new_id(&SystemClock::new()).to_string());
    warn!(%correlation_id, error = %message, "backup error recorded");
    assets.set_last_error(&StoredError {
        message,
        correlation_id,
    })
}

impl Backup {
    fn note(&self, error: &SyncError) -> Result<(), DbError> {
        let (message, request_id) = describe(error);
        record_error(&self.assets, message, request_id)
    }

    /// An error that is not about one asset: a pause ends the run, anything else is returned as it is.
    pub(super) fn halt(&self, error: SyncError) -> Halt {
        match verdict(&error) {
            Verdict::Pause(reason) => match self.note(&error) {
                Ok(()) => Halt::Pause(reason),
                Err(failure) => failure.into(),
            },
            Verdict::End(_) | Verdict::Fatal => RunError::from(error).into(),
        }
    }

    /// The run ends with `error`, stored as `last_error` first: for an error the loop gave up on after its recovery.
    pub(super) fn halt_recorded(&self, error: SyncError) -> Halt {
        match self.note(&error) {
            Ok(()) => RunError::from(error).into(),
            Err(failure) => failure.into(),
        }
    }

    pub(super) fn halt_run(&self, error: RunError) -> Halt {
        match error {
            RunError::Sync(error) => self.halt(error),
            other => other.into(),
        }
    }

    pub(super) fn halted(&self, error: BatchRunError) -> Halt {
        match error {
            BatchRunError::Sync(error) => self.halt(error),
            BatchRunError::Store(error) => RunError::Store(error).into(),
        }
    }

    /// An error of one asset: it ends the asset and the loop goes on, unless it is a pause or fatal.
    pub(super) fn settle(&self, asset_id: i64, error: SyncError) -> Result<(), Halt> {
        let Verdict::End(end) = verdict(&error) else {
            return Err(self.halt(error));
        };
        if end == AssetEnd::Failed {
            self.note(&error)?;
        }
        self.assets.mark_ended(asset_id, end)?;
        Ok(())
    }

    /// The asset ends `failed` for a reason that is not a sync-core error.
    pub(super) fn fail_asset(&self, asset_id: i64, message: &str) -> Result<(), DbError> {
        record_error(&self.assets, message.to_owned(), None)?;
        self.assets.mark_ended(asset_id, AssetEnd::Failed)
    }

    /// Files a batch left out: each ends by its own error. A key that matches no asset was edited meanwhile.
    pub(super) fn settle_left_out(&self, files: Vec<SkippedFile>) -> Result<(), Halt> {
        for file in files {
            match self.assets.asset_of_key(file.entry.staged_key)? {
                Some(asset_id) => self.settle(asset_id, file.error)?,
                None => warn!(error = %file.error, "a file left out of its batch has no asset"),
            }
        }
        Ok(())
    }
}
