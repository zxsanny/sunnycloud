//! Small-file upload (`sync_api.md`: `stage_small`, `batch_commit`): one staged request per file, batch records
//! saved before the first staging, commits in chunks, and replay of unfinished batches from the host's store.

mod member;
mod plan;
mod run;
mod wire;

use std::sync::Arc;

use sc_clock::Clock;
use sync_core_proto::{BLOCK_SIZE, BatchEntry, BatchIdError, BatchResult, check_batch_ids};
use thiserror::Error;
use uuid::Uuid;

use crate::error::SyncError;
use crate::host::{BatchRecord, FileSource, StoreError};

pub(crate) use member::blocking;
pub(crate) use run::Runner;
pub(crate) use wire::{batch_commit, stage_small};

/// Largest file that takes the staged path; a bigger one takes the block path. Zero bytes take the staged path.
pub const SMALL_FILE_MAX: u64 = BLOCK_SIZE as u64;

pub fn takes_staged_path(size: u64) -> bool {
    size <= SMALL_FILE_MAX
}

/// A new batch record: a fresh UUIDv7 `commit_id` that equals no member key. The host calls this, and the
/// executor saves the record before the first member is staged.
pub fn new_batch(clock: &dyn Clock, entries: Vec<BatchEntry>) -> Result<BatchRecord, BatchIdError> {
    let commit_id = sc_ids::new_id(clock);
    let keys: Vec<Uuid> = entries.iter().map(|entry| entry.staged_key).collect();
    check_batch_ids(commit_id, &keys)?;
    Ok(BatchRecord { commit_id, entries })
}

/// Finds the source of a replayed entry, `None` when the file is gone.
pub type SourceResolver<'a> = &'a (dyn Fn(&BatchEntry) -> Option<Arc<dyn FileSource>> + Sync);

/// A file to upload by the staged path. `entry.staged_key` is the file's `client_key` and `entry.file_hash`
/// the hash of its current content; the executor refuses to stage bytes that no longer match it.
#[derive(Clone)]
pub struct SmallFile {
    pub entry: BatchEntry,
    pub source: Arc<dyn FileSource>,
}

/// What the server did with a committed batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchOutcome {
    /// One result per entry of the report, in order.
    Committed(BatchResult),
    /// `CommitIdReused`: the server holds another request under this id; the host drops the batch.
    IdReused,
}

/// One finished batch, handed to the host before its record is cleared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchReport {
    pub commit_id: Uuid,
    pub entries: Vec<BatchEntry>,
    pub outcome: BatchOutcome,
}

/// The host stores the results of a batch. `clear_batch` runs only after this returned `Ok`.
pub trait BatchSink: Send + Sync {
    fn on_batch(&self, report: BatchReport) -> Result<(), StoreError>;
}

/// A file left out of every batch of the run, so the host handles it on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedFile {
    pub entry: BatchEntry,
    /// `SourceChanged`, `Io` (the file cannot be read) or `SizeTooLarge`.
    pub error: SyncError,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum BatchRunError {
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error(transparent)]
    Store(#[from] StoreError),
}
