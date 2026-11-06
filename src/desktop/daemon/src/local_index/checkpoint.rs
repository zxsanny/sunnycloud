use sync_core::{BatchRecord, CheckpointStore, StoreError, UploadCheckpoint};
use uuid::Uuid;

use super::repo::checkpoints;
use super::{IndexError, LocalIndexStore};

/// `local_key` is the opaque key sync-core passes; the daemon derives it from the pair and the relative path.
impl CheckpointStore for LocalIndexStore {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError> {
        self.read(|conn| Ok(checkpoints::load(conn, local_key)?))
            .map_err(failed)
    }

    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError> {
        self.write(|conn| Ok(checkpoints::save(conn, local_key, &checkpoint)?))
            .map_err(failed)
    }

    fn clear(&self, local_key: &str) -> Result<(), StoreError> {
        self.write(|conn| Ok(checkpoints::clear(conn, local_key)?))
            .map_err(failed)
    }

    fn save_batch(&self, record: BatchRecord) -> Result<(), StoreError> {
        self.write(|conn| Ok(checkpoints::save_batch(conn, &record)?))
            .map_err(failed)
    }

    fn load_batches(&self) -> Result<Vec<BatchRecord>, StoreError> {
        self.read(|conn| Ok(checkpoints::load_batches(conn)?))
            .map_err(failed)
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError> {
        self.write(|conn| Ok(checkpoints::clear_batch(conn, commit_id)?))
            .map_err(failed)
    }
}

/// The trait's error carries no detail, so the cause is logged here.
fn failed(error: IndexError) -> StoreError {
    tracing::error!(%error, "local index checkpoint operation failed");
    StoreError
}
