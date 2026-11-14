//! The two media maintenance cycles for 12's scheduler. Both are restart-safe by query alone and need no leader:
//! every statement selects its rows from the tables, claiming them with `SKIP LOCKED` where it deletes.

use std::sync::Arc;
use std::time::Duration;

use contracts::{CycleStats, GcQueue, Janitor, JanitorError, async_trait};
use sc_clock::{Clock, MonotonicInstant, Timestamp};
use sc_db::Db;
use sc_observability::log_info;

use crate::config::MediaConfig;
use crate::error::JobQueueError;
use crate::job_queue::JobQueue;
use crate::job_repo;
use crate::maintenance::fault;
use crate::retry;

/// Rows one derived janitor transaction handles.
const ORPHAN_BATCH: u32 = 100;

fn unexpected(error: impl std::error::Error + Send + Sync + 'static) -> JanitorError {
    JanitorError::Unexpected(Box::new(error))
}

fn queue_error(error: JobQueueError) -> JanitorError {
    unexpected(error)
}

/// Milliseconds since `started` on the injected clock, for the cycle's INFO line.
fn elapsed_ms(clock: &dyn Clock, started: MonotonicInstant) -> u64 {
    let took = clock.monotonic().saturating_duration_since(started);
    u64::try_from(took.as_millis()).unwrap_or(u64::MAX)
}

/// Every `job_reap_interval`: running jobs with an expired lease and no attempt left become `dead`, terminal jobs
/// finished more than `job_terminal_ttl` ago are deleted in batches, and `worker_status` rows not seen for
/// `job_terminal_ttl` are deleted.
pub struct JobReaper {
    db: Db,
    clock: Arc<dyn Clock>,
    queue: Arc<JobQueue>,
    ttl: Duration,
}

impl JobReaper {
    pub fn new(db: Db, clock: Arc<dyn Clock>, queue: Arc<JobQueue>, config: &MediaConfig) -> Self {
        JobReaper {
            db,
            clock,
            queue,
            ttl: config.job_terminal_ttl,
        }
    }
}

#[async_trait]
impl Janitor for JobReaper {
    fn name(&self) -> &'static str {
        "job_reaper"
    }

    /// `processed` is the jobs marked dead plus the job and worker rows deleted.
    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let started = self.clock.monotonic();
        let reaped = self.queue.reap(now).await.map_err(queue_error)?;
        let before = retry::sub(now, self.ttl);
        let purged = self
            .queue
            .purge_terminal(before)
            .await
            .map_err(queue_error)?;
        let workers = job_repo::purge_worker_status(&self.db, before)
            .await
            .map_err(unexpected)?;
        let processed = reaped.dead.saturating_add(purged).saturating_add(workers);
        if processed > 0 {
            let duration_ms = elapsed_ms(self.clock.as_ref(), started);
            log_info!("job reaper cycle changed rows", duration_ms = duration_ms);
        }
        Ok(CycleStats { processed })
    }
}

/// Every `derived_janitor_interval`: derived rows whose (account, content hash) has no revision left are deleted
/// and their blobs queued `Due::Now`, in batches of 100, until a batch finds fewer.
pub struct DerivedJanitor {
    db: Db,
    clock: Arc<dyn Clock>,
    gc: Arc<dyn GcQueue>,
}

impl DerivedJanitor {
    pub fn new(db: Db, clock: Arc<dyn Clock>, gc: Arc<dyn GcQueue>) -> Self {
        DerivedJanitor { db, clock, gc }
    }
}

#[async_trait]
impl Janitor for DerivedJanitor {
    fn name(&self) -> &'static str {
        "derived_janitor"
    }

    /// `processed` is the derived rows deleted. `now` plays no part: an orphan has no retention.
    async fn run_cycle(&self, _now: Timestamp) -> Result<CycleStats, JanitorError> {
        let started = self.clock.monotonic();
        let mut processed = 0;
        loop {
            let deleted = job_repo::remove_orphan_derived(&self.db, ORPHAN_BATCH, self.gc.as_ref())
                .await
                .map_err(|error| JanitorError::Unexpected(fault(error)))?;
            processed += deleted;
            if deleted < u64::from(ORPHAN_BATCH) {
                break;
            }
        }
        if processed > 0 {
            let duration_ms = elapsed_ms(self.clock.as_ref(), started);
            log_info!(
                "derived janitor cycle removed rows",
                duration_ms = duration_ms
            );
        }
        Ok(CycleStats { processed })
    }
}
