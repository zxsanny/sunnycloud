//! Tree (component 02): folders, files and their revisions. This crate holds the typed configuration section,
//! the typed errors with their conversion through `sc_error`, and the retry-once rule for a transaction the
//! database aborts; the ports are implemented by later tasks. The schema is
//! `src/migrations/*_create_tree_tables.*.sql`.

mod config;
mod error;
mod retry;

pub use config::{DEFAULT_MAINTENANCE_INTERVAL, DEFAULT_PURGE_BATCH_SIZE, TreeConfig};
pub use error::TreeError;
pub use retry::run_retrying;
