//! `OrphanSweeper`: finds blob files that have no row and are older than `orphan_sweep_age`.
//!
//! A run has two phases. The scan visits the 65,536 prefix directories (`blobs/aa/bb`): it lists the names, asks
//! `blobs` for the ids of that prefix and keeps every listed id without a row whose file is old enough. Nothing is
//! removed during the scan, so any listing or query failure ends the run with zero deletions. Only a complete scan
//! reaches the second phase: `report` logs each orphan, `delete` re-checks its row once more and then removes the
//! file. No state is kept between runs and no transaction is open while a directory is read or a file is removed.
//!
//! The scan sees three kinds of names besides blob ids and never touches them: `<id>.gc` files, which belong to the
//! GC sweeper while it works, and any other name or directory, which is counted and reported once per run.

mod orphan_repo;
mod probe;
mod scan;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, CycleStats, Janitor, JanitorError, async_trait};
use sc_clock::{Clock, Timestamp};
use sc_db::Db;
use sc_observability::{log_error, log_info, log_warn};
use uuid::Uuid;

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_io, io_fault, row_fault, run_fault};
use probe::{OrphanPoint, Probe};

#[cfg(feature = "sync-hook")]
pub use probe::{OrphanHook, OrphanPoint as OrphanHookPoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrphanMode {
    Off,
    Report,
    Delete,
}

impl OrphanMode {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(OrphanMode::Off),
            "report" => Some(OrphanMode::Report),
            "delete" => Some(OrphanMode::Delete),
            _ => None,
        }
    }
}

/// What one run did: `found` orphans (nothing in `off` mode), of which `removed` were deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrphanStats {
    pub found: u64,
    pub removed: u64,
}

pub struct OrphanSweeper {
    db: Db,
    clock: Arc<dyn Clock>,
    root: PathBuf,
    mode: OrphanMode,
    age: Duration,
    probe: Probe,
}

struct Scanned {
    orphans: Vec<Uuid>,
    foreign: u64,
}

fn ids(blob_id: Uuid) -> BlobIds {
    BlobIds {
        blob_id: Some(blob_id),
        account_id: None,
    }
}

/// An orphan is strictly older than the age; a file modified later than `now` is not old at all.
fn is_older(now: Timestamp, modified: Timestamp, age: Duration) -> bool {
    now.checked_duration_since(modified)
        .is_some_and(|elapsed| elapsed > age)
}

fn scan_fault(error: &std::io::Error) -> JanitorError {
    JanitorError::Unexpected(io_fault(error, BlobIds::default()))
}

impl OrphanSweeper {
    /// `config.blob_root` is required and `config.orphan_mode` must be `off`, `report` or `delete`.
    pub fn new(config: &BlobstoreConfig, db: Db, clock: Arc<dyn Clock>) -> Result<Self, BlobError> {
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
        let mode = OrphanMode::parse(&config.orphan_mode).ok_or_else(invalid)?;
        Ok(OrphanSweeper {
            db,
            clock,
            root: root.into(),
            mode,
            age: config.orphan_sweep_age,
            probe: Probe::new(),
        })
    }

    #[cfg(feature = "sync-hook")]
    pub fn with_hook(mut self, hook: Arc<dyn OrphanHook>) -> Self {
        self.probe.hook = Some(hook);
        self
    }

    /// One walk of the volume. `now` is the instant file ages are measured against. An `Err` means the scan was
    /// incomplete or the database failed during the delete phase; it is logged at ERROR, and a failed scan has
    /// deleted nothing.
    pub async fn run_cycle(&self, now: Timestamp) -> Result<OrphanStats, JanitorError> {
        if self.mode == OrphanMode::Off {
            return Ok(OrphanStats::default());
        }
        log_info!("orphan cycle started");
        let started = self.clock.monotonic();
        let stats = match self.sweep(now).await {
            Ok(stats) => stats,
            Err(error) => {
                log_error!("orphan sweep aborted", err = &error);
                return Err(error);
            }
        };
        let elapsed = self.clock.monotonic().saturating_duration_since(started);
        log_info!(
            "orphan cycle finished",
            duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        );
        Ok(stats)
    }

    async fn sweep(&self, now: Timestamp) -> Result<OrphanStats, JanitorError> {
        let scanned = self.scan(now).await?;
        if scanned.foreign > 0 {
            log_warn!("blob folders hold files that are not blob ids");
        }
        let found = u64::try_from(scanned.orphans.len()).unwrap_or(u64::MAX);
        let removed = match self.mode {
            OrphanMode::Delete => self.delete(&scanned.orphans).await?,
            OrphanMode::Report | OrphanMode::Off => {
                for &id in &scanned.orphans {
                    log_warn!("orphan blob file", err = &*row_fault("orphan", ids(id)));
                }
                0
            }
        };
        Ok(OrphanStats { found, removed })
    }

    async fn scan(&self, now: Timestamp) -> Result<Scanned, JanitorError> {
        let mut scanned = Scanned {
            orphans: Vec::new(),
            foreign: 0,
        };
        for prefix in 0..=u16::MAX {
            let listing = scan::list(&self.root, prefix)
                .await
                .map_err(|error| scan_fault(&error))?;
            self.probe.at(OrphanPoint::Listed(prefix)).await;
            scanned.foreign += listing.foreign;
            if listing.blobs.is_empty() {
                continue;
            }
            let (lo, hi) = scan::id_range(prefix);
            let rows = orphan_repo::ids_in_range(&self.db, lo, hi)
                .await
                .map_err(|error| JanitorError::Unexpected(run_fault(error, BlobIds::default())))?;
            for id in listing.blobs.into_iter().filter(|id| !rows.contains(id)) {
                let modified = scan::modified(&self.root, id)
                    .await
                    .map_err(|error| scan_fault(&error))?;
                if modified.is_some_and(|modified| is_older(now, modified, self.age)) {
                    scanned.orphans.push(id);
                }
            }
        }
        Ok(scanned)
    }

    /// Returns how many files were removed. A candidate whose row has appeared since the scan is kept. A file
    /// that cannot be removed is logged and does not stop the others; a failed re-check ends the phase.
    async fn delete(&self, orphans: &[Uuid]) -> Result<u64, JanitorError> {
        let mut removed = 0;
        for &id in orphans {
            self.probe.at(OrphanPoint::BeforeRecheck(id)).await;
            let has_row = orphan_repo::row_exists(&self.db, id)
                .await
                .map_err(|error| JanitorError::Unexpected(run_fault(error, ids(id))))?;
            if has_row {
                continue;
            }
            log_warn!("orphan blob file", err = &*row_fault("orphan", ids(id)));
            match scan::remove(&self.root, id).await {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(error) => log_error!(
                    "orphan blob file not deleted",
                    err = &*io_fault(&error, ids(id))
                ),
            }
        }
        Ok(removed)
    }
}

#[async_trait]
impl Janitor for OrphanSweeper {
    fn name(&self) -> &'static str {
        "orphan_sweeper"
    }

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let stats = OrphanSweeper::run_cycle(self, now).await?;
        Ok(CycleStats {
            processed: stats.removed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGE: Duration = Duration::from_millis(500);

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_unix_micros(millis * 1000)
    }

    #[test]
    fn a_file_is_old_only_when_strictly_older_than_the_age() {
        // Arrange
        let now = at(10_000);

        // Act
        let younger = is_older(now, at(10_000 - 499), AGE);
        let exactly = is_older(now, at(10_000 - 500), AGE);
        let older = is_older(now, at(10_000 - 501), AGE);

        // Assert
        assert_eq!((younger, exactly, older), (false, false, true));
    }

    #[test]
    fn a_file_modified_after_now_is_never_old() {
        // Act
        let old = is_older(at(10_000), at(10_001), AGE);

        // Assert
        assert!(!old);
    }
}
