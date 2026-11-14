//! 03 transfer: upload sessions, commit receipts and downloads. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the typed errors with their stable codes, the in-memory
//! progress table and [`PgUploadService`], which implements `UploadService` (`start_upload`, `put_block`,
//! `get_upload`, `abort_upload`, `stage_small`), `CommitService` (`commit_upload` with its `BlobCommitEffect` and
//! head sniffer, `commit_batch` with its receipt and staged-row effects) and `DownloadService`
//! (`open_revision_content`, `open_blob_content`), `ReservationSource`, and the janitors `SessionReaper`,
//! `ReceiptJanitor` and `IdleFlush`. The schema is `src/migrations/*_create_transfer_tables.*.sql`
//! and `*_add_upload_session_commit.*.sql`.

mod batch;
mod batch_effect;
mod batch_response;
mod blocks;
mod caps;
mod checkpoint;
mod commit;
mod commit_effect;
mod config;
mod download;
mod error;
mod flush;
mod janitor_repo;
mod janitors;
mod progress;
mod put_block;
mod reaper;
mod receipt_repo;
mod resume;
mod service;
mod serving;
mod session_repo;
mod shortcut;
mod sniff;
mod stage;
mod upload;
mod upload_metrics;

pub use checkpoint::CheckpointMark;
pub use config::TransferConfig;
pub use error::TransferError;
pub use janitors::{IdleFlush, ReceiptJanitor, SessionReaper};
pub use progress::{
    BlockStart, Checkpoint, CheckpointError, ProgressError, ProgressTable, SessionInfo,
    SessionProgress, SharedProgress,
};
pub use sc_ids::BLOCK_SIZE;
pub use upload::{PgUploadService, StartFault, UploadPorts};
