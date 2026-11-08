//! The health of the blob volume: the probe that `/readyz` and `/metrics` read, and the `HealthContributor`
//! built on it.

use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::sync::Arc;

use contracts::{BlobError, CheckFuture, HealthContributor, HealthItem, HealthStatus};
use sc_db::Db;
use sc_observability::log_error;

use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_db, blob_error_from_io, blob_io_failure};
use crate::metrics::BlobMetrics;
use crate::reader::finding_repo;
use crate::volume_facts;

/// What [`BlobHealth::probe`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobHealthState {
    /// The process can write to the blob root; false for a read-only mount or a root it has no right to write.
    pub writable: bool,
    /// Bytes the writer can still allocate, from the filesystem's own figure.
    pub free_bytes: u64,
    /// `free_bytes` is under `blob_free_reserve`.
    pub below_floor: bool,
    /// Rows of `blobs` with `unavailable_at` set. Read from the column on every call, so it survives a restart.
    pub unavailable_count: u64,
}

pub struct BlobHealth {
    db: Db,
    root: PathBuf,
    reserve: u64,
    /// Set by every probe; absent in a process that has no registry.
    metrics: Option<Arc<BlobMetrics>>,
}

impl BlobHealth {
    /// `config.blob_root` is required.
    pub fn new(config: &BlobstoreConfig, db: Db) -> Result<Self, BlobError> {
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(|| {
                blob_error_from_io(io::Error::from(ErrorKind::InvalidInput), BlobIds::default())
            })?;
        Ok(BlobHealth {
            db,
            root: root.into(),
            reserve: config.blob_free_reserve,
            metrics: None,
        })
    }

    /// Every probe sets `blob_free_bytes` to its `free_bytes` and `blob_free_floor_bytes` to `blob_free_reserve`.
    pub fn with_metrics(mut self, metrics: Arc<BlobMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// A blob root that cannot be queried and a database failure are `Io`; an unwritable root is a result.
    pub async fn probe(&self) -> Result<BlobHealthState, BlobError> {
        let root = self.root.clone();
        let facts = tokio::task::spawn_blocking(move || volume_facts::inspect(&root))
            .await
            .map_err(|_| blob_io_failure(ErrorKind::Other, BlobIds::default()))?
            .map_err(|error| blob_io_failure(error.kind(), BlobIds::default()))?;
        if let Some(metrics) = &self.metrics {
            metrics.volume_probed(facts.free_bytes, self.reserve);
        }
        let unavailable = finding_repo::count_unavailable(&self.db)
            .await
            .map_err(|error| blob_error_from_db(error, BlobIds::default()))?;
        Ok(BlobHealthState {
            writable: facts.writable,
            free_bytes: facts.free_bytes,
            below_floor: facts.free_bytes < self.reserve,
            unavailable_count: u64::try_from(unavailable).unwrap_or(0),
        })
    }
}

/// Registered with 07's health aggregation.
pub struct BlobHealthContributor {
    health: Arc<BlobHealth>,
}

impl BlobHealthContributor {
    pub fn new(health: Arc<BlobHealth>) -> Self {
        BlobHealthContributor { health }
    }
}

impl HealthContributor for BlobHealthContributor {
    fn name(&self) -> &str {
        "blobstore"
    }

    /// `Degraded` while any blob is marked unavailable. A probe that cannot run is `Down`. The other fields of
    /// the probe (unwritable root, below the floor) belong to `/readyz`; no level for them is specified here.
    fn check(&self) -> CheckFuture<'_, HealthItem> {
        Box::pin(async move {
            match self.health.probe().await {
                Ok(state) if state.unavailable_count > 0 => HealthItem::new(
                    HealthStatus::Degraded,
                    format!("{} blobs unavailable", state.unavailable_count),
                ),
                Ok(_) => HealthItem::ok(),
                Err(error) => {
                    log_error!("blob health probe failed", err = &error);
                    HealthItem::new(HealthStatus::Down, "blob health probe failed")
                }
            }
        })
    }
}
