//! `TreeCommit`: the only way a revision becomes current. 02 owns the transaction; the caller supplies its
//! effects per call and never sees the transaction.

use std::sync::Arc;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_error::ErrorCode;
use sc_ids::{FileHash, Hash32};
use thiserror::Error;
use uuid::Uuid;

use super::effect::CommitEffect;
use crate::common::Fault;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitActor {
    pub account_id: Uuid,
    pub device_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Existing {
        node_id: Uuid,
        base_revision: Option<Uuid>,
    },
    New {
        parent_id: Uuid,
        name: String,
    },
}

/// `client_meta` is JSON text. `reserved_bytes` is released from the quota ledger inside the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRevision {
    pub revision_id: Uuid,
    pub blob_id: Uuid,
    pub size: i64,
    pub content_hash: FileHash,
    pub blocklist: Vec<Hash32>,
    pub media_type: Option<String>,
    pub client_mtime: Option<Timestamp>,
    pub client_meta: Option<String>,
    pub reserved_bytes: i64,
    pub import_hint: bool,
}

/// `vault` is false in v1.0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishEntry {
    pub target: Target,
    pub revision: NewRevision,
    pub vault: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    Created,
    Updated,
    Replaced,
    ConflictCopied { copy_node: Uuid },
    CreatedAsCopy { node: Uuid },
    Revived,
    NoChange { existing_revision: Uuid },
}

/// A per-entry failure: a value in the result list, not an error of the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryFailure {
    NameInvalid(String),
    ParentDeleted,
    ParentNotFound,
    NotAFolder,
    NotFound,
    /// The blob is not owned by the account.
    BlocklistMismatch,
    EffectFailed(ErrorCode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryResult {
    Published {
        outcome: PublishOutcome,
        node_id: Uuid,
        revision_id: Uuid,
        conflict_copy: Option<Uuid>,
    },
    Failed(EntryFailure),
}

/// The stored response of an earlier batch, replayed by `Begin::Replay`: 03's serialised `BatchResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredResponse(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishResult {
    /// One result per entry, in entry order.
    Published(Vec<EntryResult>),
    Replayed(StoredResponse),
}

#[derive(Debug, Error)]
pub enum TreeCommitError {
    #[error("account not active")]
    AccountNotActive,
    #[error("commit id reused")]
    CommitIdReused,
    #[error("effect failed: {}", .0.as_str())]
    EffectFailed(ErrorCode),
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait TreeCommit: Send + Sync {
    /// Runs `ensure_active`, the account lock, every effect's `on_begin`, each entry in its own savepoint
    /// (with every effect's `on_entry`), every effect's `on_batch`, the feed stamp and COMMIT.
    async fn publish(
        &self,
        actor: CommitActor,
        entries: Vec<PublishEntry>,
        effects: Vec<Arc<dyn CommitEffect>>,
        now: Timestamp,
    ) -> Result<PublishResult, TreeCommitError>;
}
