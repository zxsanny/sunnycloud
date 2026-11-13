use sync_core_proto::{BatchEntry, FileHash, Hash32};
use thiserror::Error;
use uuid::Uuid;

/// The host store could not read or write a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("checkpoint store failed")]
pub struct StoreError;

/// Where a large upload stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum UploadState {
    /// `block_hashes` holds the blocks hashed so far; `file_hash` and `session_id` are not known yet.
    Hashing,
    /// The file is hashed and the session is open.
    Uploading,
    /// Every needed block is acknowledged and the commit was sent.
    Committing,
}

/// Resume data of one large upload. `commit_id` holds the upload's `client_key` (13 §2 and §3 name it differently).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct UploadCheckpoint {
    pub session_id: Option<Uuid>,
    pub block_hashes: Vec<Hash32>,
    pub file_hash: Option<FileHash>,
    pub commit_id: Uuid,
    pub state: UploadState,
}

/// A small-file batch that is staged but whose per-entry results the host has not stored yet.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BatchRecord {
    pub commit_id: Uuid,
    pub entries: Vec<BatchEntry>,
}

/// Checkpoints of the host. The change-feed cursor is not stored here: the host persists it after applying a page.
pub trait CheckpointStore: Send + Sync {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError>;
    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError>;
    fn clear(&self, local_key: &str) -> Result<(), StoreError>;

    /// Returns before the first `stage_small` of any member.
    fn save_batch(&self, record: BatchRecord) -> Result<(), StoreError>;
    fn load_batches(&self) -> Result<Vec<BatchRecord>, StoreError>;
    /// Called only after the host stored the per-entry results.
    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError>;
}
