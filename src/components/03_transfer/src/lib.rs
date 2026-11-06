//! 03 transfer: upload sessions, commit receipts and downloads. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the typed errors with their stable codes and the in-memory
//! progress table; the operations are implemented by later tasks. The schema is
//! `src/migrations/*_create_transfer_tables.*.sql`.

mod config;
mod error;
mod progress;

pub use config::TransferConfig;
pub use error::TransferError;
pub use progress::{
    Checkpoint, CheckpointError, ProgressError, ProgressTable, SessionProgress, SharedProgress,
};
pub use sc_ids::BLOCK_SIZE;
