//! `GcSweeper`: removes the blobs whose `blob_gc` row is due.
//!
//! One batch is three steps, none of which holds a transaction while a file is touched:
//! 1. a short transaction claims up to `gc_batch` due rows and moves their `due_at` forward by `gc_claim_lease`,
//!    so rows stay claimable if the process dies;
//! 2. each file is parked (renamed to `<path>.gc`), see [`files`];
//! 3. a second short transaction locks the claimed blob rows, runs the cleanups of every registered
//!    [`BlobOwnerCleanup`] and removes the rows; only then is the parked file deleted.
//!
//! The lock in step 3 and the check under it (still `gc_pending`, still carrying this lease) are what a
//! concurrent `reinstate_in_tx` meets: it either finished before (the blob is not removed and its file is moved
//! back) or waits until the rows are gone (`BeyondRetention`). A file is never deleted while a row that could be
//! reinstated points at it.

mod files;
mod probe;
mod sweep_repo;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    BlobError, BlobOwnerCleanup, CycleStats, Fault, Janitor, JanitorError, async_trait,
};
use sc_clock::{CancellationToken, Clock, Timestamp};
use sc_db::Db;
use sc_observability::{log_error, log_info, log_warn};
use uuid::Uuid;

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_io, io_fault, row_fault, run_fault};
use crate::registry::BlobRegistry;
use files::{Files, Park};
use probe::{GcPoint, Probe};
use sweep_repo::Verdict;

#[cfg(feature = "sync-hook")]
pub use probe::{GcHook, GcHookStop, GcPoint as GcHookPoint};

/// What one cycle did. `remaining_due` counts the rows still due when the cycle ended; rows claimed by a batch
/// that did not finish are leased, not due.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcStats {
    pub removed: u64,
    pub remaining_due: u64,
    pub budget_spent: bool,
}

pub struct GcSweeper {
    db: Db,
    clock: Arc<dyn Clock>,
    registry: Arc<BlobRegistry>,
    cleanups: Vec<Arc<dyn BlobOwnerCleanup>>,
    files: Files,
    batch: i64,
    lease: Duration,
    budget: Duration,
    shutdown: CancellationToken,
    probe: Probe,
}

fn ids(blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: None,
    }
}

fn cycle_fault(what: &'static str) -> JanitorError {
    JanitorError::Unexpected(row_fault(what, BlobIds::default()))
}

fn elapsed_ms(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// The failure of one blob with its id, so the log line can be joined with the row.
#[derive(Debug)]
struct ItemFailure {
    blob_id: Uuid,
    cause: Fault,
}

impl fmt::Display for ItemFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "blob {}: {}", self.blob_id, self.cause)
    }
}

impl std::error::Error for ItemFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.cause)
    }
}

impl GcSweeper {
    /// `cleanups` is the list the composition root registers; every entry is called for every blob. `shutdown`
    /// ends a cycle between two batches. `config.blob_root` is required and `gc_batch` must fit a `LIMIT`.
    pub fn new(
        config: &BlobstoreConfig,
        db: Db,
        clock: Arc<dyn Clock>,
        registry: Arc<BlobRegistry>,
        cleanups: Vec<Arc<dyn BlobOwnerCleanup>>,
        shutdown: CancellationToken,
    ) -> Result<Self, BlobError> {
        let invalid = || {
            blob_error_from_io(
                std::io::Error::from(std::io::ErrorKind::InvalidInput),
                BlobIds::default(),
            )
        };
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(invalid)?;
        let batch = i64::try_from(config.gc_batch)
            .ok()
            .filter(|batch| *batch > 0)
            .ok_or_else(invalid)?;
        Ok(GcSweeper {
            db,
            clock,
            registry,
            cleanups,
            files: Files::new(root.into()),
            batch,
            lease: config.gc_claim_lease,
            budget: config.gc_cycle_budget,
            shutdown,
            probe: Probe::new(),
        })
    }

    #[cfg(feature = "sync-hook")]
    pub fn with_hook(mut self, hook: Arc<dyn GcHook>) -> Self {
        self.probe.hook = Some(hook);
        self
    }

    /// Repeats batches until nothing is due, `gc_cycle_budget` is spent (measured on the clock's monotonic time
    /// and checked between batches) or shutdown is requested. `now` decides what is due; the time a cycle has
    /// already run is added to it for later batches.
    pub async fn run_cycle(&self, now: Timestamp) -> Result<GcStats, JanitorError> {
        log_info!("gc cycle started");
        self.cycle(now).await.inspect_err(|error| {
            log_error!("gc sweep aborted", err = error);
        })
    }

    async fn cycle(&self, now: Timestamp) -> Result<GcStats, JanitorError> {
        let started = self.clock.monotonic();
        let mut removed = 0u64;
        let mut budget_hit = false;
        let (ended_at, elapsed) = loop {
            let elapsed = self.clock.monotonic().saturating_duration_since(started);
            let at = now
                .checked_add(elapsed)
                .ok_or_else(|| cycle_fault("clock overflow"))?;
            if self.shutdown.is_cancelled() {
                break (at, elapsed);
            }
            if elapsed >= self.budget {
                budget_hit = true;
                break (at, elapsed);
            }
            let lease_until = at
                .checked_add(self.lease)
                .ok_or_else(|| cycle_fault("clock overflow"))?;
            let claimed = sweep_repo::claim(
                &self.db,
                at.as_unix_micros(),
                lease_until.as_unix_micros(),
                self.batch,
            )
            .await
            .map_err(|error| JanitorError::Unexpected(run_fault(error, BlobIds::default())))?;
            if claimed.is_empty() {
                break (at, elapsed);
            }
            removed += self.remove_claimed(&claimed, lease_until).await;
        };
        let remaining = sweep_repo::count_due(&self.db, ended_at.as_unix_micros())
            .await
            .map_err(|error| JanitorError::Unexpected(run_fault(error, BlobIds::default())))?;
        let remaining_due = u64::try_from(remaining).map_err(|_| cycle_fault("negative count"))?;
        log_info!("gc cycle finished", duration_ms = elapsed_ms(elapsed));
        Ok(GcStats {
            removed,
            remaining_due,
            budget_spent: budget_hit && remaining_due > 0,
        })
    }

    /// Returns how many blobs this batch removed. A failure of one blob, of the batch's database work or of a
    /// file step is logged and leaves the rows leased, so the next cycle after the lease retries them.
    async fn remove_claimed(&self, claimed: &[Uuid], lease_until: Timestamp) -> u64 {
        if self.probe.at(GcPoint::Claimed).await.is_err() {
            log_error!("gc batch stopped");
            return 0;
        }
        let mut parked = Vec::with_capacity(claimed.len());
        for &id in claimed {
            match self.files.park(id).await {
                Ok(Park::Missing) => {
                    let fault = row_fault("file already missing", ids(id));
                    log_warn!("gc blob file already missing", err = &*fault);
                }
                Ok(Park::Moved | Park::Earlier) => {}
                Err(error) => {
                    log_error!("gc blob file not parked", err = &*io_fault(&error, ids(id)));
                    continue;
                }
            }
            parked.push(id);
            if self.probe.at(GcPoint::Parked).await.is_err() {
                log_error!("gc batch stopped");
                return 0;
            }
        }
        if self.probe.at(GcPoint::BeforeSettle).await.is_err() {
            log_error!("gc batch stopped");
            return 0;
        }
        let settled = sweep_repo::settle(
            &self.db,
            &parked,
            lease_until.as_unix_micros(),
            &self.cleanups,
            &self.registry,
        )
        .await;
        match settled {
            Ok(verdicts) => self.finish_files(verdicts).await,
            Err(error) => {
                log_error!(
                    "gc batch not settled",
                    err = &*run_fault(error, BlobIds::default())
                );
                0
            }
        }
    }

    /// The row stays `gc_pending` and restorable, so its file goes back to its own name until a later cycle.
    async fn restore_file(&self, id: Uuid) {
        if let Err(error) = self.files.unpark(id).await {
            log_error!(
                "gc parked file not moved back",
                err = &*io_fault(&error, ids(id))
            );
        }
    }

    async fn finish_files(&self, verdicts: Vec<(Uuid, Verdict)>) -> u64 {
        let mut removed = 0;
        for (id, verdict) in verdicts {
            match verdict {
                Verdict::Removed | Verdict::Gone => {
                    if let Err(error) = self.files.discard(id).await {
                        log_error!(
                            "gc parked file not deleted",
                            err = &*io_fault(&error, ids(id))
                        );
                    }
                    if matches!(verdict, Verdict::Removed) {
                        removed += 1;
                    }
                }
                Verdict::Kept => self.restore_file(id).await,
                Verdict::Failed(cause) => {
                    let failure = ItemFailure { blob_id: id, cause };
                    log_error!("gc blob not removed", err = &failure);
                    self.restore_file(id).await;
                }
            }
        }
        removed
    }
}

#[async_trait]
impl Janitor for GcSweeper {
    fn name(&self) -> &'static str {
        "gc_sweeper"
    }

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let stats = GcSweeper::run_cycle(self, now).await?;
        Ok(CycleStats {
            processed: stats.removed,
        })
    }
}
