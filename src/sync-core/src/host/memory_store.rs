use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

use super::checkpoint::{BatchRecord, CheckpointStore, StoreError, UploadCheckpoint};

#[derive(Default)]
struct Records {
    checkpoints: HashMap<String, UploadCheckpoint>,
    batches: BTreeMap<Uuid, BatchRecord>,
}

/// In-memory [`CheckpointStore`] for tests and for hosts without a database. Nothing survives the process.
#[derive(Default)]
pub struct MemoryCheckpointStore {
    records: Mutex<Records>,
}

impl MemoryCheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every operation replaces whole values under the lock, so a panic elsewhere cannot leave a half-written record.
    fn records(&self) -> MutexGuard<'_, Records> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CheckpointStore for MemoryCheckpointStore {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError> {
        Ok(self.records().checkpoints.get(local_key).cloned())
    }

    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError> {
        self.records()
            .checkpoints
            .insert(local_key.to_owned(), checkpoint);
        Ok(())
    }

    fn clear(&self, local_key: &str) -> Result<(), StoreError> {
        self.records().checkpoints.remove(local_key);
        Ok(())
    }

    fn save_batch(&self, record: BatchRecord) -> Result<(), StoreError> {
        self.records().batches.insert(record.commit_id, record);
        Ok(())
    }

    fn load_batches(&self) -> Result<Vec<BatchRecord>, StoreError> {
        Ok(self.records().batches.values().cloned().collect())
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError> {
        self.records().batches.remove(&commit_id);
        Ok(())
    }
}
