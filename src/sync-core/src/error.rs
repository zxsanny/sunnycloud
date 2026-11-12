//! The client's typed error set. Texts are fixed strings plus server codes that passed a charset check:
//! no token, file name, path or server-provided free text appears in `Display`.

use std::io::ErrorKind;
use std::time::Duration;

use sc_error::TransportKind;
use sync_core_proto::{DeviceRefusal, ResyncReason};
use thiserror::Error;
use uuid::Uuid;

/// Every failure an operation of `SyncApi` can end with: the common errors first, then the
/// per-operation errors of `sync_api.md`. Wire codes become values of this type only in
/// [`map_problem`](crate::map_problem).
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SyncError {
    #[error("transport failure ({0:?})")]
    Transport(TransportKind),
    #[error("cancelled")]
    Cancelled,
    /// `message` is the server text meant for the user; `update_url` is kept only when it is an `https` URL.
    #[error("client upgrade required")]
    UpgradeRequired {
        message: String,
        min_version: Option<String>,
        update_url: Option<String>,
    },
    #[error("unauthorized")]
    Unauthorized,
    /// The host could not give a credential (a locked keyring, a store call past its bound, no stored token).
    /// Nothing was sent; it never rotates tokens or stops work, and the next call reads the host again.
    #[error("credential unavailable")]
    CredentialUnavailable,
    /// `retry_after` is `None` when the server sent no delay; the caller then uses its own backoff.
    #[error("rate limited")]
    RateLimited { retry_after: Option<Duration> },
    #[error("account not active")]
    AccountNotActive,
    /// A code outside the wire table, or a body without a usable code (`internal`).
    #[error("unexpected server answer `{code}`{}", request_suffix(.request_id))]
    Unexpected {
        code: String,
        request_id: Option<String>,
    },

    #[error("not found")]
    NotFound,
    #[error("content unavailable")]
    BlobUnavailable,
    #[error("not a folder")]
    NotAFolder,
    #[error("name conflict")]
    NameConflict,
    #[error("invalid name")]
    InvalidName,
    #[error("cycle")]
    Cycle,
    #[error("parent not found")]
    ParentNotFound,
    #[error("parent deleted")]
    ParentGone,
    /// `current` has no source on the wire yet (`sync_api.md` open point 2); the mapping always leaves it `None`.
    #[error("edited since")]
    EditedSince { current: Option<Uuid> },
    #[error("quota exceeded")]
    QuotaExceeded,
    #[error("size too large")]
    SizeTooLarge,
    #[error("upload session cap reached")]
    SessionCapReached,
    #[error("free space floor reached")]
    FreeSpaceFloor,
    #[error("staged file cap reached")]
    StagedCapReached,
    #[error("batch too large")]
    BatchTooLarge,
    #[error("commit id reused")]
    CommitIdReused,
    #[error("blocklist mismatch")]
    BlocklistMismatch,
    #[error("resync required ({reason:?})")]
    ResyncRequired { reason: ResyncReason },

    #[error("source changed")]
    SourceChanged,
    #[error("io error ({kind:?})")]
    Io { kind: ErrorKind },
    #[error("hash mismatch in block {block}")]
    HashMismatch { block: u32 },
    #[error("protocol violation: {detail}")]
    ProtocolViolation { detail: &'static str },
    #[error("invalid code")]
    InvalidCode,
    /// An RFC 8628 poll refusal; `device_grant_poll` answers it as its outcome, not as an error.
    #[error("device grant refused ({0:?})")]
    DeviceGrantRefused(DeviceRefusal),
}

impl SyncError {
    /// Whether the same request may be sent again unchanged. Everything else is final for the
    /// operation; 426 in particular is never retried (`sync_api.md` step 6).
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::RateLimited { .. })
    }
}

fn request_suffix(request_id: &Option<String>) -> String {
    request_id
        .as_deref()
        .map(|id| format!(" (request {id})"))
        .unwrap_or_default()
}

/// Wire answers that are inputs of the upload planner rather than operation errors. The executor picks
/// the planner call from where it asked: [`as_block_result`](Self::as_block_result) after a block `PUT`,
/// [`as_commit_result`](Self::as_commit_result) after a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannerInput {
    BlockConflict { retry_after: Option<Duration> },
    HashRejected,
    SessionGone,
    NeedBlocks { list: Vec<u32> },
}

impl PlannerInput {
    pub fn as_block_result(&self) -> Option<sync_core_proto::BlockResult> {
        use sync_core_proto::BlockResult;
        match self {
            Self::BlockConflict { retry_after } => Some(BlockResult::BlockConflict {
                retry_after: *retry_after,
            }),
            Self::HashRejected => Some(BlockResult::HashRejected),
            Self::SessionGone => Some(BlockResult::SessionGone),
            Self::NeedBlocks { .. } => None,
        }
    }

    pub fn as_commit_result(&self) -> Option<sync_core_proto::CommitResult> {
        use sync_core_proto::CommitResult;
        match self {
            Self::NeedBlocks { list } => Some(CommitResult::NeedBlocks { list: list.clone() }),
            Self::SessionGone => Some(CommitResult::SessionGone),
            Self::BlockConflict { .. } | Self::HashRejected => None,
        }
    }
}

/// Result of [`map_problem`](crate::map_problem).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mapped {
    Error(SyncError),
    Planner(PlannerInput),
}
