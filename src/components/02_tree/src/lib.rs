//! Tree (component 02): folders, files and their revisions. This crate holds the typed configuration section,
//! the typed errors with their conversion through `sc_error`, the retry-once rule for a transaction the
//! database aborts, the folder create, rename, move and delete operations and `TreeCommit::publish` with its
//! conflict outcomes and the path and device queries (`TreeService`, `TreeCommit` on `PgTreeService`), the node,
//! listing and revision reads with the thumbnail states of a listing page and the live usage sum (`TreeReader`,
//! `LiveUsageSource` on `PgTreeReader`), the account lifecycle hook with the blob owner cleanup
//! (`PgTreeLifecycle`), and the folder cascade and tombstone purge (`PgTreeMaintenance`) with their janitors
//! (`TreeJanitor`); every change of a file's current revision applies its usage delta and queues the blob it
//! unreferences in its own transaction; the other ports are implemented by later tasks.
//! The schema is `src/migrations/*_create_tree_tables.*.sql`.

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
mod publish_usage;
mod reader;
mod reader_repo;
mod reader_revisions;
mod reader_thumbs;
mod retry;
mod revision_repo;
mod service;
mod service_queries;

pub use config::{DEFAULT_MAINTENANCE_INTERVAL, DEFAULT_PURGE_BATCH_SIZE, TreeConfig};
pub use error::TreeError;
pub use janitor::TreeJanitor;
pub use lifecycle::PgTreeLifecycle;
pub use maintenance::PgTreeMaintenance;
pub use reader::PgTreeReader;
#[doc(hidden)]
pub use reader_repo::LISTING_PAGE_SQL;
pub use reader_revisions::MAX_LOOKUP_HASHES;
pub use retry::{TxFailure, run_retrying};
#[doc(hidden)]
pub use revision_repo::{DEVICE_REVISIONS_SQL, LOOKUP_REVISIONS_SQL};
pub use service::PgTreeService;
pub use service_queries::MAX_REVISION_PAGE;
