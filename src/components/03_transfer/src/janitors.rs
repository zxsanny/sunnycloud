//! The maintenance cycles of 03 on the `Janitor` port, for 12's scheduler (registration is E-22/c06). Each holds
//! the upload service, whose ports, progress table and metrics its cycle uses; none needs a leader.
//!
//! - [`SessionReaper`] (every `session_reap_interval`): session and staged rows past `expires_at` are deleted and
//!   their reservations released exactly once (`crate::reaper`).
//! - [`ReceiptJanitor`] (every `receipt_janitor_interval`): commit receipts older than `commit_receipt_ttl` are
//!   deleted in batches; a later batch with a deleted commit id is a new batch.
//! - [`IdleFlush`]: progress entries idle for `checkpoint_interval` are checkpointed and evicted (`crate::flush`).

use std::sync::Arc;

use contracts::{CycleStats, Fault, Janitor, JanitorError, async_trait};
use sc_clock::Timestamp;

use crate::janitor_repo;
use crate::upload::PgUploadService;

/// Receipts one janitor statement deletes.
const RECEIPT_BATCH: i64 = 1000;

fn stats(result: Result<u64, Fault>) -> Result<CycleStats, JanitorError> {
    result
        .map(|processed| CycleStats { processed })
        .map_err(JanitorError::Unexpected)
}

pub struct SessionReaper {
    service: Arc<PgUploadService>,
}

impl SessionReaper {
    pub fn new(service: Arc<PgUploadService>) -> Self {
        SessionReaper { service }
    }
}

#[async_trait]
impl Janitor for SessionReaper {
    fn name(&self) -> &'static str {
        "session_reaper"
    }

    /// `processed` is the session, staged and committed-key rows deleted.
    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        stats(self.service.reap_expired(now).await)
    }
}

pub struct ReceiptJanitor {
    service: Arc<PgUploadService>,
}

impl ReceiptJanitor {
    pub fn new(service: Arc<PgUploadService>) -> Self {
        ReceiptJanitor { service }
    }
}

#[async_trait]
impl Janitor for ReceiptJanitor {
    fn name(&self) -> &'static str {
        "receipt_janitor"
    }

    /// `processed` is the receipts deleted: those created before `now - commit_receipt_ttl`.
    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let Some(before) = now.checked_sub(self.service.config.commit_receipt_ttl) else {
            return Ok(CycleStats::default());
        };
        let mut processed = 0u64;
        loop {
            let deleted =
                janitor_repo::delete_old_receipts(&self.service.db, before, RECEIPT_BATCH)
                    .await
                    .map_err(|error| JanitorError::Unexpected(Box::new(error)))?;
            processed = processed.saturating_add(deleted);
            if deleted < RECEIPT_BATCH.unsigned_abs() {
                return Ok(CycleStats { processed });
            }
        }
    }
}

pub struct IdleFlush {
    service: Arc<PgUploadService>,
}

impl IdleFlush {
    pub fn new(service: Arc<PgUploadService>) -> Self {
        IdleFlush { service }
    }
}

#[async_trait]
impl Janitor for IdleFlush {
    fn name(&self) -> &'static str {
        "progress_flush"
    }

    /// `processed` is the entries evicted; `now` stamps the checkpoints written.
    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        stats(self.service.flush_idle(now).await)
    }
}
