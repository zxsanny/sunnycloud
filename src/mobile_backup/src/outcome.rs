//! The input and output types of one backup run (`backup_core.md` § Types).

use std::time::Duration;

use sync_core::{BatchRunError, StoreError, SyncError};
use thiserror::Error;

use crate::checkpoint::DbError;
use crate::source::SourceError;

/// The entry point that started the run. A catch-up run always does the visible-id diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Trigger {
    MediaChange,
    CatchUp,
    AppOpen,
    FirstSync,
}

/// What the phone's connection is right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum NetworkState {
    Offline,
    Metered,
    Unmetered,
}

/// The user's Wi-Fi-only setting and the current connection: together they decide whether a run starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NetworkPolicy {
    pub wifi_only: bool,
    pub network: NetworkState,
}

impl Default for NetworkPolicy {
    /// No constraint: an unmetered connection.
    fn default() -> Self {
        Self {
            wifi_only: false,
            network: NetworkState::Unmetered,
        }
    }
}

impl NetworkPolicy {
    pub(crate) fn pause(self) -> Option<PauseReason> {
        match self.network {
            NetworkState::Offline => Some(PauseReason::NoNetwork),
            NetworkState::Metered if self.wifi_only => Some(PauseReason::MeteredNetwork),
            NetworkState::Metered | NetworkState::Unmetered => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RunContext {
    pub trigger: Trigger,
    /// How long the run may take, counted from its start: it ends `Partial` at the first check after that.
    pub deadline: Option<Duration>,
    pub network_policy: NetworkPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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

impl PauseReason {
    pub(crate) fn as_text(self) -> &'static str {
        match self {
            Self::QuotaExceeded => "quota_exceeded",
            Self::NoNetwork => "no_network",
            Self::MeteredNetwork => "metered_network",
            Self::PermissionDenied => "permission_denied",
            Self::LimitedAccess => "limited_access",
            Self::ServerRejectedName => "server_rejected_name",
            Self::UpgradeRequired => "upgrade_required",
            Self::AccountNotActive => "account_not_active",
            Self::SignedOut => "signed_out",
        }
    }

    pub(crate) fn from_text(text: &str) -> Option<Self> {
        [
            Self::QuotaExceeded,
            Self::NoNetwork,
            Self::MeteredNetwork,
            Self::PermissionDenied,
            Self::LimitedAccess,
            Self::ServerRejectedName,
            Self::UpgradeRequired,
            Self::AccountNotActive,
            Self::SignedOut,
        ]
        .into_iter()
        .find(|reason| reason.as_text() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SkipReason {
    AlreadyRunning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum BackupOutcome {
    Completed,
    /// The deadline passed or `cancel()` was called; `remaining` assets still wait.
    Partial {
        remaining: u64,
    },
    Paused(PauseReason),
    Skipped(SkipReason),
}

/// Why a run ended before its work was done; the checkpoint keeps every asset it stood on. The errors that are
/// pauses or end one asset never get here (`backup_core.md` § Wire errors); `Sync(Unauthorized)` is the contract's
/// `TokenRejected`.
#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error(transparent)]
    Checkpoint(#[from] DbError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Source(#[from] SourceError),
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
