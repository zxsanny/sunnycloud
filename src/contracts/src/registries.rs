//! Ports that the composition root (12) registers late, as lists, to break wiring cycles.
//!
//! `HealthContributor` and `ReadinessCheck` are declared by `sc_observability` and re-exported unchanged.

use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

pub use sc_observability::{
    CheckFuture, HealthContributor, HealthItem, HealthStatus, ReadinessCheck, ReadinessItem,
    ReadinessStatus,
};

#[derive(Debug, Error)]
pub enum JanitorError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// What one janitor cycle did; the fields of the spec are open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CycleStats {
    pub processed: u64,
}

/// A periodic maintenance job, run by the scheduler in 12 at its configured interval.
#[async_trait]
pub trait Janitor: Send + Sync {
    fn name(&self) -> &'static str;

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError>;
}

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// `remaining == 0` means this hook has nothing left for the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EraseProgress {
    pub remaining: u64,
}

/// Account lifecycle step of a component. Only 01 calls `initialize`; only 07 calls `erase_batch`.
#[async_trait]
pub trait AccountLifecycleHook: Send + Sync {
    async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError>;

    async fn erase_batch(&self, account: Uuid, limit: u32)
    -> Result<EraseProgress, LifecycleError>;
}

#[derive(Debug, Error)]
pub enum BlobCleanupError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Removes the rows of one component that own a blob, in the caller's transaction, before 04 deletes it.
#[async_trait]
pub trait BlobOwnerCleanup: Send + Sync {
    async fn remove_owner_rows(&self, tx: &mut DbTx, blob_id: Uuid)
    -> Result<(), BlobCleanupError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorTag {
    Restore,
    PreFlight,
}

/// The `doctor` options of 12 that a check may read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoctorContext {
    pub pre_flight: bool,
    pub backup_interval: Option<Duration>,
    pub attest_backup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingLevel {
    Ok,
    Warn,
    Fail,
}

/// One doctor result. `message` and `remedy` never hold a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: FindingLevel,
    pub message: String,
    pub remedy: String,
}

/// A read-only check of `sunnycloud doctor`. A problem is a [`Finding`], never an error.
#[async_trait]
pub trait DoctorCheck: Send + Sync {
    fn name(&self) -> &'static str;

    fn tags(&self) -> &[DoctorTag];

    async fn run(&self, ctx: &DoctorContext) -> Vec<Finding>;
}
