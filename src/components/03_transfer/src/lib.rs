//! 03 transfer: upload sessions, commit receipts and downloads. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the typed errors with their stable codes, the in-memory
//! progress table, `UploadService` ([`PgUploadService`]: `start_upload`, `put_block`, `get_upload`,
//! `abort_upload` and `stage_small`) and `commit_upload` with its `BlobCommitEffect` and head sniffer; the other
//! operations are implemented by later tasks. The schema is `src/migrations/*_create_transfer_tables.*.sql` and `*_add_upload_session_commit.*.sql`.

mod blocks;
mod caps;
mod checkpoint;
mod commit;
mod commit_effect;
mod config;
mod error;
mod progress;
mod put_block;
mod resume;
mod service;
mod session_repo;
mod shortcut;
mod sniff;
mod stage;
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
