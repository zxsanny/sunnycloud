//! Media jobs (component 08, crate `media-api`): the API-side owner of the job queue and the derived files. This
//! crate holds the typed configuration section, the typed errors with their conversion through `sc_error`, the
//! Postgres job queue, the job planning at commit (`JobPlanner`, `MediaEnqueueEffect`) and the derived access
//! (`ThumbnailStates`, `DerivedFiles`). The schema is `src/migrations/*_create_job_tables.*.sql`.

mod config;
mod derived_access_repo;
mod derived_opener;
mod derived_repo;
mod enqueue_effect;
mod error;
mod job_metrics;
mod job_queue;
mod job_repo;
mod planner;
mod retry;
mod thumb_states;

pub use config::{JOB_ATTEMPTS_MAX, JOB_ATTEMPTS_MIN, MediaConfig};
#[doc(hidden)]
pub use derived_access_repo::explain_thumb_states;
pub use derived_opener::DerivedFiles;
pub use enqueue_effect::MediaEnqueueEffect;
pub use error::{JobQueueError, MediaError};
pub use job_metrics::JobMetrics;
pub use job_queue::{EnqueueOutcome, JobQueue, Priority, ReapStats};
#[doc(hidden)]
pub use job_repo::explain_claim;
pub use planner::JobPlanner;
pub use thumb_states::ThumbnailStates;
