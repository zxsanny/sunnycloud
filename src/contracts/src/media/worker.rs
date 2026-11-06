//! `WorkerAuthenticator`, `WorkerJobs` and `MediaAdmin`.

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use super::model::{
    ClaimedJob, DerivedKind, FailDisposition, FailResult, JobKind, Lane, LeaseExtended, OutputMeta,
    SkipOutcome, WorkerStatus,
};
use crate::blob::{ByteStream, PlaintextStream};
use crate::common::Fault;

/// The authenticated worker. It carries no account; the worker id is the `X-Worker-Id` the caller passes to
/// each job call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerIdentity;

#[derive(Debug, Error)]
pub enum WorkerAuthError {
    /// One variant for every cause: no header, wrong secret, a user session, a device token (AC-79).
    #[error("forbidden")]
    Forbidden,
}

#[async_trait]
pub trait WorkerAuthenticator: Send + Sync {
    async fn authenticate(&self, bearer: &str) -> Result<WorkerIdentity, WorkerAuthError>;
}

#[derive(Debug, Error)]
pub enum WorkerJobError {
    /// The job is not running, is leased by another worker, or its lease expired.
    #[error("lease lost")]
    LeaseLost,
    #[error("revision obsolete")]
    RevisionObsolete,
    #[error("output too large")]
    OutputTooLarge,
    #[error("output type rejected")]
    OutputTypeRejected,
    /// The job does not produce this derived kind.
    #[error("output kind rejected")]
    OutputKindRejected,
    #[error("storage failure")]
    Storage(#[source] Fault),
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptOutcome {
    Accepted,
    AlreadyPresent,
}

/// The whole plaintext of a job's input revision; `length` is exact.
pub struct WorkerInput {
    pub length: u64,
    pub body: PlaintextStream,
}

/// The queue as the worker supervisor sees it, reached only through the API (ADR-009).
#[async_trait]
pub trait WorkerJobs: Send + Sync {
    /// `None` when nothing is due in the lane.
    async fn claim(
        &self,
        lane: Lane,
        worker_id: &str,
    ) -> Result<Option<ClaimedJob>, WorkerJobError>;

    async fn extend_lease(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<LeaseExtended, WorkerJobError>;

    async fn release(&self, job_id: Uuid, worker_id: &str) -> Result<(), WorkerJobError>;

    async fn complete_skipped(
        &self,
        job_id: Uuid,
        worker_id: &str,
        outcome: SkipOutcome,
    ) -> Result<(), WorkerJobError>;

    async fn fail(
        &self,
        job_id: Uuid,
        worker_id: &str,
        code: &str,
        disposition: FailDisposition,
    ) -> Result<FailResult, WorkerJobError>;

    /// Decrypts through `BlobReader` with no range.
    async fn open_input(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<WorkerInput, WorkerJobError>;

    async fn accept_output(
        &self,
        job_id: Uuid,
        worker_id: &str,
        kind: DerivedKind,
        meta: OutputMeta,
        body: ByteStream,
        length: u64,
    ) -> Result<AcceptOutcome, WorkerJobError>;

    async fn record_status(
        &self,
        worker_id: &str,
        status: WorkerStatus,
    ) -> Result<(), WorkerJobError>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequeueFilter {
    pub account: Option<Uuid>,
    pub kind: Option<JobKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobCount {
    pub lane: Lane,
    pub status: JobStatus,
    pub count: u64,
}

/// Job counts per lane and status; the contract leaves the exact fields open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaStats {
    pub counts: Vec<JobCount>,
}

#[derive(Debug, Error)]
pub enum MediaAdminError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait MediaAdmin: Send + Sync {
    /// Sets the matching dead jobs back to queued with attempts reset and returns how many.
    async fn requeue_dead(&self, filter: RequeueFilter) -> Result<u64, MediaAdminError>;

    async fn stats(&self) -> Result<MediaStats, MediaAdminError>;
}
