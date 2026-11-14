//! `MediaAdmin` (E-18/c11): the operator's requeue of dead jobs and the job counts per lane and status. The same
//! counts set the `jobs_queued` and `jobs_running` gauges before each scrape, so the gauges are the table's.

use std::sync::Arc;

use contracts::{
    CheckFuture, JobCount, JobKind, JobStatus, Lane, MediaAdmin, MediaAdminError, MediaStats,
    RequeueFilter, async_trait,
};
use media_shared::types as wire;
use sc_db::{Db, DbError};
use sc_observability::{NoLabels, ScrapeProbe, log_error};

use crate::job_metrics::JobMetrics;
use crate::job_queue::JobQueue;
use crate::job_repo::{self, overflow};

/// Registered by 12 as the `MediaAdmin` port and as a scrape probe of the metrics registry.
pub struct MediaAdminService {
    db: Db,
    queue: Arc<JobQueue>,
    metrics: Arc<JobMetrics>,
}

impl MediaAdminService {
    pub fn new(db: Db, queue: Arc<JobQueue>, metrics: Arc<JobMetrics>) -> Self {
        MediaAdminService { db, queue, metrics }
    }

    async fn counts(&self) -> Result<MediaStats, DbError> {
        let rows = job_repo::job_counts(&self.db)
            .await
            .map_err(|error| match error {
                sc_db::RunError::Closure(error) | sc_db::RunError::Db(error) => error,
            })?;
        let counts = rows
            .into_iter()
            .map(|(lane, status, count)| {
                Ok(JobCount {
                    lane: parse_lane(&lane)?,
                    status: parse_status(&status)?,
                    count: u64::try_from(count).map_err(|_| overflow("job count"))?,
                })
            })
            .collect::<Result<_, DbError>>()?;
        Ok(MediaStats { counts })
    }
}

fn wire_kind(kind: JobKind) -> wire::JobKind {
    match kind {
        JobKind::Stills => wire::JobKind::Stills,
        JobKind::Poster => wire::JobKind::Poster,
        JobKind::Transcode => wire::JobKind::Transcode,
    }
}

fn parse_lane(name: &str) -> Result<Lane, DbError> {
    match name {
        "image" => Ok(Lane::Image),
        "video" => Ok(Lane::Video),
        other => Err(DbError::Other(format!("unknown job lane {other:?}"))),
    }
}

fn parse_status(name: &str) -> Result<JobStatus, DbError> {
    match name {
        "queued" => Ok(JobStatus::Queued),
        "running" => Ok(JobStatus::Running),
        "succeeded" => Ok(JobStatus::Succeeded),
        "dead" => Ok(JobStatus::Dead),
        other => Err(DbError::Other(format!("unknown job status {other:?}"))),
    }
}

/// The jobs of `status` over both lanes.
fn total(stats: &MediaStats, status: JobStatus) -> u64 {
    stats
        .counts
        .iter()
        .filter(|count| count.status == status)
        .map(|count| count.count)
        .sum()
}

#[async_trait]
impl MediaAdmin for MediaAdminService {
    /// The filter fields combine with AND; an empty filter requeues every dead job.
    async fn requeue_dead(&self, filter: RequeueFilter) -> Result<u64, MediaAdminError> {
        self.queue
            .requeue_dead(filter.account, filter.kind.map(wire_kind))
            .await
            .map_err(|error| MediaAdminError::Unexpected(Box::new(error)))
    }

    /// One row per (lane, status) present in the table, ordered by lane then status.
    async fn stats(&self) -> Result<MediaStats, MediaAdminError> {
        self.counts()
            .await
            .map_err(|error| MediaAdminError::Unexpected(Box::new(error)))
    }
}

impl ScrapeProbe for MediaAdminService {
    /// A failed read leaves the gauges at their last values and is logged.
    fn refresh(&self) -> CheckFuture<'_, ()> {
        Box::pin(async move {
            match self.counts().await {
                Ok(stats) => {
                    let queued = total(&stats, JobStatus::Queued);
                    let running = total(&stats, JobStatus::Running);
                    self.metrics.queued.set(NoLabels, queued as f64);
                    self.metrics.running.set(NoLabels, running as f64);
                }
                Err(error) => log_error!("job counts for the queue gauges failed", err = &error),
            }
        })
    }
}
