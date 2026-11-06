//! `BlobRegistry`, `BlobCatalog` and `GcQueue`: the `blobs` and `blob_gc` rows.

use async_trait::async_trait;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use super::model::{BlobPlan, WrittenBlob};
use crate::blob::BlobRef;
use crate::common::Fault;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrittenOrPlanned {
    Written(WrittenBlob),
    Planned(BlobPlan),
}

/// The state a new `blobs` row starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterState {
    Uploading,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReinstateOutcome {
    Reinstated,
    BeyondRetention,
    FileMissing,
}

#[derive(Debug, Error)]
pub enum BlobRegistryError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Row writes of `blobs`, always inside the caller's transaction; none of these opens one:
///
/// ```compile_fail,E0061
/// async fn without_a_transaction(registry: &dyn contracts::BlobRegistry) {
///     let _ = registry.remove_row_in_tx(uuid::Uuid::nil()).await;
/// }
/// ```
#[async_trait]
pub trait BlobRegistry: Send + Sync {
    async fn register_in_tx(
        &self,
        tx: &mut DbTx,
        blob: WrittenOrPlanned,
        state: RegisterState,
    ) -> Result<(), BlobRegistryError>;

    async fn mark_committed_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
        size: i64,
    ) -> Result<(), BlobRegistryError>;

    /// Checks the file before its statements: within retention the queue row is removed and the state returns
    /// to committed.
    async fn reinstate_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
    ) -> Result<ReinstateOutcome, BlobRegistryError>;

    /// Deletes the `blobs` and `blob_gc` rows.
    async fn remove_row_in_tx(&self, tx: &mut DbTx, blob_id: Uuid)
    -> Result<(), BlobRegistryError>;
}

#[derive(Debug, Error)]
pub enum BlobCatalogError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// The only read of `blobs` on the request path.
#[async_trait]
pub trait BlobCatalog: Send + Sync {
    /// Account-scoped: another account's blob id and an unknown id are both `None`.
    async fn get_ref(
        &self,
        account: Uuid,
        blob_id: Uuid,
    ) -> Result<Option<BlobRef>, BlobCatalogError>;
}

/// Why a blob is queued for removal (data model 6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcReason {
    Replaced,
    Deleted,
    AccountDeleted,
    DerivedRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// `now + retention`.
    AfterRetention,
    Now,
}

#[derive(Debug, Error)]
pub enum GcQueueError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Every method that writes the queue takes the caller's transaction:
///
/// ```compile_fail,E0061
/// async fn without_a_transaction(queue: &dyn contracts::GcQueue) {
///     let _ = queue
///         .schedule_in_tx(
///             uuid::Uuid::nil(),
///             contracts::GcReason::Deleted,
///             contracts::Due::Now,
///         )
///         .await;
/// }
/// ```
#[async_trait]
pub trait GcQueue: Send + Sync {
    /// The single writer of `blob_gc` and of `blobs.state = gc_pending`. Scheduling a queued blob again keeps
    /// one row with the earliest due time.
    async fn schedule_in_tx(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
        reason: GcReason,
        due: Due,
    ) -> Result<(), GcQueueError>;

    /// The account's queued blobs that are not yet removed.
    async fn pending_for_account(&self, account: Uuid) -> Result<u64, GcQueueError>;
}
