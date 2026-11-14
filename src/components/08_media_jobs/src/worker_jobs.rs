//! `WorkerJobs` (E-18/c08): the one port the worker listener calls. Lease operations go to the `JobQueue`, input
//! and outputs to the `DerivedStore`; both check the holder (running, leased by `worker_id`, lease live) in the
//! statement itself, so a caller that does not hold the job gets `LeaseLost` and no job data, never `Forbidden`.
//! The DTOs of `contracts` and `media-shared` are mapped field for field.

use std::sync::Arc;

use contracts::{
    AcceptOutcome, ByteStream, ClaimedJob, DerivedKind, FailDisposition, FailResult, JobKind, Lane,
    LeaseExtended, OutputMeta, SkipOutcome, WorkerInput, WorkerJobError, WorkerJobs, WorkerStatus,
    async_trait,
};
use media_shared::types as wire;
use sc_clock::Clock;
use sc_db::Db;
use uuid::Uuid;

use crate::derived_store::DerivedStore;
use crate::error::JobQueueError;
use crate::job_queue::JobQueue;
use crate::job_repo;

pub struct WorkerJobsFacade {
    db: Db,
    clock: Arc<dyn Clock>,
    queue: Arc<JobQueue>,
    store: Arc<DerivedStore>,
}

impl WorkerJobsFacade {
    pub fn new(
        db: Db,
        clock: Arc<dyn Clock>,
        queue: Arc<JobQueue>,
        store: Arc<DerivedStore>,
    ) -> Self {
        WorkerJobsFacade {
            db,
            clock,
            queue,
            store,
        }
    }
}

fn job_error(error: JobQueueError) -> WorkerJobError {
    match error {
        JobQueueError::LeaseLost => WorkerJobError::LeaseLost,
        JobQueueError::Db(error) => WorkerJobError::Unexpected(Box::new(error)),
    }
}

fn lane(lane: Lane) -> wire::Lane {
    match lane {
        Lane::Image => wire::Lane::Image,
        Lane::Video => wire::Lane::Video,
    }
}

fn claimed(job: wire::ClaimedJob) -> ClaimedJob {
    ClaimedJob {
        job_id: job.job_id,
        kind: match job.kind {
            wire::JobKind::Stills => JobKind::Stills,
            wire::JobKind::Poster => JobKind::Poster,
            wire::JobKind::Transcode => JobKind::Transcode,
        },
        content_hash: job.content_hash,
        params_version: job.params_version,
        attempt: job.attempt,
        lease_ms: job.lease_ms,
    }
}

fn skip_outcome(outcome: SkipOutcome) -> wire::SkipOutcome {
    match outcome {
        SkipOutcome::SkippedCompliant => wire::SkipOutcome::SkippedCompliant,
        SkipOutcome::Obsolete => wire::SkipOutcome::Obsolete,
    }
}

fn disposition(disposition: FailDisposition) -> wire::Disposition {
    match disposition {
        FailDisposition::Retry => wire::Disposition::Retry,
        FailDisposition::Permanent => wire::Disposition::Permanent,
    }
}

#[async_trait]
impl WorkerJobs for WorkerJobsFacade {
    async fn claim(
        &self,
        lane_of: Lane,
        worker_id: &str,
    ) -> Result<Option<ClaimedJob>, WorkerJobError> {
        let job = self
            .queue
            .claim(lane(lane_of), worker_id)
            .await
            .map_err(job_error)?;
        Ok(job.map(claimed))
    }

    async fn extend_lease(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<LeaseExtended, WorkerJobError> {
        let extended = self
            .queue
            .extend_lease(job_id, worker_id)
            .await
            .map_err(job_error)?;
        Ok(LeaseExtended {
            lease_ms: extended.lease_ms,
        })
    }

    async fn release(&self, job_id: Uuid, worker_id: &str) -> Result<(), WorkerJobError> {
        self.queue
            .release(job_id, worker_id)
            .await
            .map_err(job_error)
    }

    async fn complete_skipped(
        &self,
        job_id: Uuid,
        worker_id: &str,
        outcome: SkipOutcome,
    ) -> Result<(), WorkerJobError> {
        self.queue
            .complete_skipped(job_id, worker_id, skip_outcome(outcome))
            .await
            .map_err(job_error)
    }

    async fn fail(
        &self,
        job_id: Uuid,
        worker_id: &str,
        code: &str,
        how: FailDisposition,
    ) -> Result<FailResult, WorkerJobError> {
        let result = self
            .queue
            .fail(job_id, worker_id, code, disposition(how))
            .await
            .map_err(job_error)?;
        Ok(match result {
            wire::FailResult::Requeued => FailResult::Requeued,
            wire::FailResult::Dead => FailResult::Dead,
        })
    }

    async fn open_input(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<WorkerInput, WorkerJobError> {
        self.store.open_input(job_id, worker_id).await
    }

    async fn accept_output(
        &self,
        job_id: Uuid,
        worker_id: &str,
        kind: DerivedKind,
        meta: OutputMeta,
        body: ByteStream,
        length: u64,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        self.store
            .accept_output(job_id, worker_id, kind, meta, body, length)
            .await
    }

    /// `seen_at` is the API clock's now; the report carries no time of the worker's.
    async fn record_status(
        &self,
        worker_id: &str,
        status: WorkerStatus,
    ) -> Result<(), WorkerJobError> {
        let image_age_days = i32::try_from(status.image_age_days)
            .map_err(|error| WorkerJobError::Unexpected(Box::new(error)))?;
        job_repo::record_status(
            &self.db,
            worker_id,
            self.clock.now_utc(),
            image_age_days,
            status.decoder_ok,
        )
        .await
        .map_err(|error| WorkerJobError::Unexpected(Box::new(error)))
    }
}
