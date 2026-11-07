use std::sync::Arc;
use std::time::Duration;

use media_shared::policy::JobSpec;
use media_shared::types::{
    ClaimedJob, Disposition, FailResult, JobKind, Lane, LeaseExtended, SkipOutcome,
};
use sc_clock::{Clock, Timestamp};
use sc_db::{Db, DbError, DbTx};
use sc_ids::new_id;
use sc_observability::NoLabels;
use uuid::Uuid;

use crate::config::MediaConfig;
use crate::error::JobQueueError;
use crate::job_metrics::JobMetrics;
use crate::job_repo::{self, FailPlan, Failed, Lease, Next, PURGE_BATCH};
use crate::retry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Interactive,
    Bulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    AlreadyExists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReapStats {
    /// Running jobs with an expired lease and exhausted attempts, now dead.
    pub dead: u64,
}

/// The Postgres job queue. It keeps no queue state: every call is one short transaction, so a restart loses
/// nothing. All times come from the injected clock; the worker's clock is never read. Enqueue joins the caller's
/// account-scoped transaction; every other call runs in system scope.
pub struct JobQueue {
    db: Db,
    clock: Arc<dyn Clock>,
    lease: Duration,
    max_attempts: i64,
    backoff: Duration,
    metrics: Arc<JobMetrics>,
}

impl JobQueue {
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        config: &MediaConfig,
        metrics: Arc<JobMetrics>,
    ) -> Self {
        JobQueue {
            db,
            clock,
            lease: config.job_lease,
            max_attempts: i64::try_from(config.job_attempts).unwrap_or(i64::MAX),
            backoff: config.job_retry_backoff,
            metrics,
        }
    }

    fn lease_ms(&self) -> u64 {
        u64::try_from(self.lease.as_millis()).unwrap_or(u64::MAX)
    }

    fn lease_end(&self, now: Timestamp) -> Timestamp {
        retry::add(now, self.lease)
    }

    /// Inserts the job unless one exists for (account, kind, content hash, params version). The statement runs on
    /// `tx`, so the job exists only if the caller's transaction commits.
    pub async fn enqueue_in_tx(
        &self,
        tx: &mut DbTx,
        spec: JobSpec,
        priority: Priority,
    ) -> Result<EnqueueOutcome, DbError> {
        let priority = match priority {
            Priority::Interactive => 0,
            Priority::Bulk => 1,
        };
        let now = self.clock.now_utc();
        let inserted =
            job_repo::enqueue(tx, new_id(self.clock.as_ref()), &spec, priority, now).await?;
        Ok(if inserted {
            EnqueueOutcome::Enqueued
        } else {
            EnqueueOutcome::AlreadyExists
        })
    }

    /// The next due job of the lane by priority then due time, leased to `worker_id`. A running job whose lease
    /// expired is taken over (the retry path); one whose attempts are exhausted is closed `dead` and skipped.
    pub async fn claim(
        &self,
        lane: Lane,
        worker_id: &str,
    ) -> Result<Option<ClaimedJob>, JobQueueError> {
        let now = self.clock.now_utc();
        let lease = Lease {
            worker: worker_id,
            now,
            until: self.lease_end(now),
            lease_ms: self.lease_ms(),
            max_attempts: self.max_attempts,
        };
        loop {
            match job_repo::claim(&self.db, lane, &lease).await? {
                Next::Empty => return Ok(None),
                Next::Exhausted { was_running } => {
                    self.metrics.dead.inc(NoLabels);
                    if was_running {
                        self.metrics.leases_expired.inc(NoLabels);
                    }
                }
                Next::Leased { job, took_over } => {
                    if took_over {
                        self.metrics.leases_expired.inc(NoLabels);
                    }
                    return Ok(Some(job));
                }
            }
        }
    }

    /// Heartbeat: the lease runs `job_lease` from now.
    pub async fn extend_lease(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<LeaseExtended, JobQueueError> {
        let now = self.clock.now_utc();
        let held = job_repo::extend(&self.db, job_id, worker_id, now, self.lease_end(now)).await?;
        held.then_some(LeaseExtended {
            lease_ms: self.lease_ms(),
        })
        .ok_or(JobQueueError::LeaseLost)
    }

    /// Graceful shutdown: back to `queued`, claimable at once, the attempt given back.
    pub async fn release(&self, job_id: Uuid, worker_id: &str) -> Result<(), JobQueueError> {
        let now = self.clock.now_utc();
        let held = job_repo::release(&self.db, job_id, worker_id, now).await?;
        held.then_some(()).ok_or(JobQueueError::LeaseLost)
    }

    /// Closes the job `succeeded` with the skip outcome and no derived row.
    pub async fn complete_skipped(
        &self,
        job_id: Uuid,
        worker_id: &str,
        outcome: SkipOutcome,
    ) -> Result<(), JobQueueError> {
        let now = self.clock.now_utc();
        let held = job_repo::complete_skipped(&self.db, job_id, worker_id, now, outcome).await?;
        held.then_some(()).ok_or(JobQueueError::LeaseLost)
    }

    /// `Dead` for a permanent failure or when this was the last attempt; otherwise back to `queued` after the
    /// backoff, which doubles with every attempt.
    pub async fn fail(
        &self,
        job_id: Uuid,
        worker_id: &str,
        code: &str,
        disposition: Disposition,
    ) -> Result<FailResult, JobQueueError> {
        let error = retry::bounded_error(code);
        let plan = FailPlan {
            worker: worker_id,
            now: self.clock.now_utc(),
            disposition,
            max_attempts: self.max_attempts,
            backoff: self.backoff,
            error: &error,
        };
        match job_repo::fail(&self.db, job_id, &plan).await? {
            Failed::LeaseLost => Err(JobQueueError::LeaseLost),
            Failed::Requeued => Ok(FailResult::Requeued),
            Failed::Dead => {
                self.metrics.dead.inc(NoLabels);
                Ok(FailResult::Dead)
            }
        }
    }

    /// Marks `dead` the running jobs whose lease expired with their attempts used up. Expired leases with
    /// attempts left stay claimable and are counted when a claim takes them over.
    pub async fn reap(&self, now: Timestamp) -> Result<ReapStats, JobQueueError> {
        let dead = job_repo::reap(&self.db, now, self.max_attempts).await?;
        self.metrics.leases_expired.inc_by(NoLabels, dead);
        self.metrics.dead.inc_by(NoLabels, dead);
        Ok(ReapStats { dead })
    }

    /// Deletes terminal jobs finished before `before`, one short batch per transaction, until none is left.
    pub async fn purge_terminal(&self, before: Timestamp) -> Result<u64, JobQueueError> {
        let mut total = 0;
        loop {
            let deleted = job_repo::purge_batch(&self.db, before).await?;
            total += deleted;
            if i64::try_from(deleted).is_ok_and(|deleted| deleted < PURGE_BATCH) {
                return Ok(total);
            }
        }
    }

    /// Sets the matching dead jobs back to `queued` with attempts reset and returns how many.
    pub async fn requeue_dead(
        &self,
        account: Option<Uuid>,
        kind: Option<JobKind>,
    ) -> Result<u64, JobQueueError> {
        let now = self.clock.now_utc();
        Ok(job_repo::requeue_dead(&self.db, now, account, kind).await?)
    }
}
