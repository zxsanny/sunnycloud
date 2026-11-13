//! 03 transfer: upload sessions, commit receipts and downloads. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the typed errors with their stable codes, the in-memory
//! progress table, `start_upload` and `put_block` ([`PgUploadService`]); the other operations are implemented by later
//! tasks. The schema is `src/migrations/*_create_transfer_tables.*.sql` and `*_add_upload_session_commit.*.sql`.

mod blocks;
mod caps;
mod checkpoint;
mod config;
mod error;
mod progress;
mod put_block;
mod session_repo;
mod shortcut;
mod upload;
mod upload_metrics;

pub use checkpoint::CheckpointMark;
pub use config::TransferConfig;
pub use error::TransferError;
pub use progress::{
    BlockStart, Checkpoint, CheckpointError, ProgressError, ProgressTable, SessionInfo,
    SessionProgress, SharedProgress,
};
pub use sc_ids::BLOCK_SIZE;
pub use upload::{PgUploadService, StartFault, UploadPorts};
