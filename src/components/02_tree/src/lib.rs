//! Tree (component 02): folders, files and their revisions. This crate holds the typed configuration section,
//! the typed errors with their conversion through `sc_error`, the retry-once rule for a transaction the
//! database aborts, the folder create, rename, move and delete operations and `TreeCommit::publish` with its
//! conflict outcomes (both on `PgTreeService`), the account lifecycle hook with the blob owner cleanup (`PgTreeLifecycle`), and the folder
//! cascade and tombstone purge (`PgTreeMaintenance`) with their janitors (`TreeJanitor`); the other ports are
//! implemented by later tasks. The schema is `src/migrations/*_create_tree_tables.*.sql`.

mod config;
mod conflict;
mod conflict_repo;
mod error;
mod janitor;
mod lifecycle;
mod lifecycle_repo;
mod maintenance;
mod maintenance_repo;
mod node_repo;
mod publish;
mod publish_repo;
mod retry;
mod service;

pub use config::{DEFAULT_MAINTENANCE_INTERVAL, DEFAULT_PURGE_BATCH_SIZE, TreeConfig};
pub use error::TreeError;
pub use janitor::TreeJanitor;
pub use lifecycle::PgTreeLifecycle;
pub use maintenance::PgTreeMaintenance;
pub use retry::{TxFailure, run_retrying};
pub use service::PgTreeService;
