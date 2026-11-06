//! Narrow ports of 02 for the operator CLI, the quota recompute and the scheduler.

use async_trait::async_trait;
use sc_clock::Timestamp;
use thiserror::Error;
use uuid::Uuid;

use super::service::NodeOrPath;
use crate::common::Fault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The revision is current again on its original node.
    Original,
    /// The original folder is gone: the revision sits in the root folder `Restored`.
    Restored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub restored_revision: Uuid,
    pub placement: Placement,
}

#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("beyond retention")]
    BeyondRetention,
    #[error("file missing")]
    FileMissing,
    #[error("not found")]
    NotFound,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Operator only (`admin restore`).
#[async_trait]
pub trait TreeRestore: Send + Sync {
    /// Restores the latest revision with `created_at <= at`.
    async fn restore_revision(
        &self,
        account: Uuid,
        target: NodeOrPath,
        at: Timestamp,
    ) -> Result<RestoreOutcome, RestoreError>;
}

#[derive(Debug, Error)]
pub enum LiveUsageError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait LiveUsageSource: Send + Sync {
    /// Sum of the sizes of the current revisions of live files.
    async fn live_bytes(&self, account: Uuid) -> Result<i64, LiveUsageError>;
}

#[derive(Debug, Error)]
pub enum MaintenanceError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Each method is restart-safe by query alone and returns the number of rows it handled.
#[async_trait]
pub trait TreeMaintenance: Send + Sync {
    async fn cascade_batch(&self, now: Timestamp, limit: u32) -> Result<u64, MaintenanceError>;

    async fn purge_tombstones(&self, now: Timestamp, limit: u32) -> Result<u64, MaintenanceError>;
}
