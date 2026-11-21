//! Media jobs (component 08, crate `media-api`): the API-side owner of the job queue and the derived files. This
//! crate holds the typed configuration section, the typed errors with their conversion through `sc_error`, the
//! Postgres job queue, the job planning at commit (`JobPlanner`, `MediaEnqueueEffect`), the derived access
//! (`ThumbnailStates`, `DerivedFiles`), the acceptance of worker outputs (`DerivedStore`) and the worker side:
//! `WorkerAuth` (the supervisor's bearer) and `WorkerJobsFacade` (the `WorkerJobs` port), the maintenance ports
//! (`MediaLifecycle`, `JobReaper`, `DerivedJanitor`) and the operator surface (`MediaAdminService`,
//! `MediaHealth`). The schema is
//! `src/migrations/*_create_job_tables.*.sql`.

mod admin_repo;
mod config;
mod derived_access_repo;
mod derived_opener;
mod derived_repo;
mod derived_store;
mod enqueue_effect;
mod enqueue_repo;
mod error;
mod janitors;
mod job_fail_repo;
mod job_metrics;
mod job_names;
mod job_output_repo;
mod job_queue;
mod job_repo;
mod maintenance;
mod maintenance_repo;
mod media_admin;
mod output_rules;
mod planner;
mod retry;
mod sniff;
mod thumb_events;
mod thumb_states;
mod worker_auth;
mod worker_health;
mod worker_jobs;

pub use config::{JOB_ATTEMPTS_MAX, JOB_ATTEMPTS_MIN, MediaConfig};
#[doc(hidden)]
pub use derived_access_repo::explain_thumb_states;
pub use derived_opener::DerivedFiles;
pub use derived_store::{BlobPorts, DerivedStore};
pub use enqueue_effect::MediaEnqueueEffect;
pub use error::{JobQueueError, MediaError};
pub use janitors::{DerivedJanitor, JobReaper};
pub use job_metrics::{JobMetrics, KindLabel};
pub use job_queue::{EnqueueOutcome, JobQueue, Priority, ReapStats};
#[doc(hidden)]
pub use job_repo::explain_claim;
pub use maintenance::MediaLifecycle;
pub use media_admin::MediaAdminService;
pub use planner::JobPlanner;
pub use thumb_states::ThumbnailStates;
pub use worker_auth::WorkerAuth;
pub use worker_health::MediaHealth;
pub use worker_jobs::WorkerJobsFacade;
