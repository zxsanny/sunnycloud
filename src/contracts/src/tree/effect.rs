//! The commit-effect protocol, implemented by 03 and 08 and run inside 02's transaction. Effects do no I/O,
//! read only rows they own and add no statement after the feed stamp.

use async_trait::async_trait;
use sc_db::DbTx;
use sc_error::ErrorCode;
use sc_ids::FileHash;
use thiserror::Error;
use uuid::Uuid;

use super::publish::{EntryResult, StoredResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Begin {
    Proceed,
    /// The commit id was already applied: nothing else runs and this response is returned.
    Replay(StoredResponse),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EffectError {
    /// Rolls back that entry's savepoint only; the other entries commit.
    #[error("entry effect failed: {}", .0.as_str())]
    Entry(ErrorCode),
    /// Rolls back the whole transaction.
    #[error("batch effect failed: {}", .0.as_str())]
    Batch(ErrorCode),
}

/// The entry as published; `account_id` is not part of it (read it from the `CommitActor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedEntry {
    pub node_id: Uuid,
    pub revision_id: Uuid,
    pub content_hash: FileHash,
    pub media_type: Option<String>,
    pub size: i64,
    pub import_hint: bool,
    pub vault: bool,
}

/// The result of the entry at `index`, successful or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryOutcome {
    pub index: usize,
    pub result: EntryResult,
}

#[async_trait]
pub trait CommitEffect: Send + Sync {
    async fn on_begin(&self, tx: &mut DbTx) -> Result<Begin, EffectError>;

    async fn on_entry(&self, tx: &mut DbTx, entry: &PublishedEntry) -> Result<(), EffectError>;

    /// Receives every entry outcome, failed ones included.
    async fn on_batch(&self, tx: &mut DbTx, outcomes: &[EntryOutcome]) -> Result<(), EffectError>;
}
