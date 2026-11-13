//! Ports of 08 media jobs. The worker job DTOs are declared again in `media-shared` for the supervisor; the
//! field names are the same and a wire test in E-18/E-20 keeps them equal. `contracts` has no serde, so the
//! JSON is produced by 11.

mod derived;
mod model;
mod worker;

pub use derived::{
    DerivedBlob, DerivedError, DerivedOpener, ThumbState, ThumbnailStateError, ThumbnailStateSource,
};
pub use model::{
    ClaimedJob, DerivedKind, FailDisposition, FailResult, JobKind, Lane, LeaseExtended, OutputMeta,
    SkipOutcome, WorkerStatus,
};
pub use worker::{
    AcceptOutcome, JobCount, JobStatus, MediaAdmin, MediaAdminError, MediaStats, RequeueFilter,
    WorkerAuthError, WorkerAuthenticator, WorkerIdentity, WorkerInput, WorkerJobError, WorkerJobs,
};
