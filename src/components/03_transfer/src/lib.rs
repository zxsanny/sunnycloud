//! 03 transfer: upload sessions, commit receipts and downloads. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the typed errors with their stable codes, the in-memory
//! progress table and `start_upload` ([`PgUploadService`]); the other operations are implemented by later
//! tasks. The schema is `src/migrations/*_create_transfer_tables.*.sql` and `*_add_upload_session_commit.*.sql`.

mod config;
mod error;
mod progress;
mod session_repo;
mod shortcut;
mod upload;
mod upload_metrics;

pub use config::TransferConfig;
pub use error::TransferError;
pub use progress::{
    Checkpoint, CheckpointError, ProgressError, ProgressTable, SessionProgress, SharedProgress,
};
pub use sc_ids::BLOCK_SIZE;
pub use upload::{PgUploadService, StartFault, UploadPorts};
