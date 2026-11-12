//! Tree (component 02): folders, files and their revisions. This crate holds the typed configuration section,
//! the typed errors with their conversion through `sc_error`, the retry-once rule for a transaction the
//! database aborts, and the folder create, rename, move and delete operations (`PgTreeService`); the other
//! ports are implemented by later tasks. The schema is `src/migrations/*_create_tree_tables.*.sql`.

mod config;
mod error;
mod node_repo;
mod retry;
mod service;

pub use config::{DEFAULT_MAINTENANCE_INTERVAL, DEFAULT_PURGE_BATCH_SIZE, TreeConfig};
pub use error::TreeError;
pub use retry::{TxFailure, run_retrying};
pub use service::PgTreeService;
