//! Media jobs (component 08, crate `media-api`): the API-side owner of the job queue and the derived files. This
//! crate holds the typed configuration section and the typed errors with their conversion through `sc_error`;
//! the queue and the derived store are implemented by later tasks. The schema is
//! `src/migrations/*_create_job_tables.*.sql`.

mod config;
mod error;

pub use config::{JOB_ATTEMPTS_MAX, JOB_ATTEMPTS_MIN, MediaConfig};
pub use error::MediaError;
