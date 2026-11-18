//! sync-core's `CheckpointStore` over the `session` and `batch` tables (host traits contract, 13 §2).

use std::sync::Arc;

use rusqlite::{OptionalExtension, params};
use sync_core::{BatchRecord, CheckpointStore, StoreError, UploadCheckpoint, UploadState};
use sync_core_proto::{BatchEntry, FileHash, Hash32};
use tracing::error;
use uuid::Uuid;

use super::{CheckpointDb, DbError};

const HASH_LEN: usize = 32;

/// The persistent [`CheckpointStore`] of the mobile uploader. Cloning shares the database.
#[derive(Clone, Debug)]
pub struct SqliteCheckpointStore {
    db: Arc<CheckpointDb>,
}

impl SqliteCheckpointStore {
    pub fn new(db: Arc<CheckpointDb>) -> Self {
        Self { db }
    }
}

/// The contract's error is a unit type, so the cause is logged here and never dropped.
fn failed(operation: &'static str, cause: &DbError) -> StoreError {
    error!(operation, error = %cause, "checkpoint store failed");
    StoreError
}

fn state_text(state: UploadState) -> &'static str {
    match state {
        UploadState::Hashing => "hashing",
        UploadState::Uploading => "uploading",
        UploadState::Committing => "committing",
    }
}

fn state_of(text: &str) -> Result<UploadState, DbError> {
    match text {
        "hashing" => Ok(UploadState::Hashing),
        "uploading" => Ok(UploadState::Uploading),
        "committing" => Ok(UploadState::Committing),
        _ => Err(DbError::Malformed("upload state")),
    }
}

fn uuid_of(bytes: &[u8], what: &'static str) -> Result<Uuid, DbError> {
    Uuid::from_slice(bytes).map_err(|_| DbError::Malformed(what))
}

fn hash_of(bytes: &[u8], what: &'static str) -> Result<[u8; HASH_LEN], DbError> {
    <[u8; HASH_LEN]>::try_from(bytes).map_err(|_| DbError::Malformed(what))
}

fn pack_hashes(hashes: &[Hash32]) -> Vec<u8> {
    hashes.iter().flat_map(|hash| *hash.as_bytes()).collect()
}

fn unpack_hashes(bytes: &[u8]) -> Result<Vec<Hash32>, DbError> {
    if !bytes.len().is_multiple_of(HASH_LEN) {
        return Err(DbError::Malformed("block hash list"));
    }
    Ok(bytes
        .chunks_exact(HASH_LEN)
        .map(|chunk| {
            let mut hash = [0u8; HASH_LEN];
            hash.copy_from_slice(chunk);
            Hash32::from(hash)
        })
        .collect())
}

impl CheckpointStore for SqliteCheckpointStore {
    fn load(&self, local_key: &str) -> Result<Option<UploadCheckpoint>, StoreError> {
        self.db
            .read(|connection| {
                let row = connection
                    .query_row(
                        "SELECT state, session_id, file_hash, commit_id, block_hashes FROM session WHERE local_key = ?1",
                        [local_key],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<Vec<u8>>>(1)?,
                                row.get::<_, Option<Vec<u8>>>(2)?,
                                row.get::<_, Vec<u8>>(3)?,
                                row.get::<_, Vec<u8>>(4)?,
                            ))
                        },
                    )
                    .optional()?;
                row.map(|(state, session_id, file_hash, commit_id, block_hashes)| {
                    Ok(UploadCheckpoint {
                        session_id: session_id
                            .map(|bytes| uuid_of(&bytes, "session id"))
                            .transpose()?,
                        block_hashes: unpack_hashes(&block_hashes)?,
                        file_hash: file_hash
                            .map(|bytes| hash_of(&bytes, "file hash").map(FileHash::from))
                            .transpose()?,
                        commit_id: uuid_of(&commit_id, "commit id")?,
                        state: state_of(&state)?,
                    })
                })
                .transpose()
            })
            .map_err(|cause| failed("load", &cause))
    }

    fn save(&self, local_key: &str, checkpoint: UploadCheckpoint) -> Result<(), StoreError> {
        self.db
            .write(|transaction| {
                transaction.execute(
                    "INSERT OR REPLACE INTO session (local_key, state, session_id, file_hash, commit_id, block_hashes) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        local_key,
                        state_text(checkpoint.state),
                        checkpoint.session_id.map(|id| id.as_bytes().to_vec()),
                        checkpoint.file_hash.map(|hash| hash.as_bytes().to_vec()),
                        checkpoint.commit_id.as_bytes().to_vec(),
                        pack_hashes(&checkpoint.block_hashes),
                    ],
                )?;
                Ok(())
            })
            .map_err(|cause| failed("save", &cause))
    }

    fn clear(&self, local_key: &str) -> Result<(), StoreError> {
        self.db
            .write(|transaction| {
                transaction.execute("DELETE FROM session WHERE local_key = ?1", [local_key])?;
                Ok(())
            })
            .map_err(|cause| failed("clear", &cause))
    }

    fn save_batch(&self, record: BatchRecord) -> Result<(), StoreError> {
        let entries = serde_json::to_string(&record.entries)
            .map_err(|_| failed("save_batch", &DbError::Malformed("batch entries")))?;
        self.db
            .write(|transaction| {
                transaction.execute(
                    "INSERT OR REPLACE INTO batch (commit_id, entries) VALUES (?1, ?2)",
                    params![record.commit_id.as_bytes().to_vec(), entries],
                )?;
                Ok(())
            })
            .map_err(|cause| failed("save_batch", &cause))
    }

    fn load_batches(&self) -> Result<Vec<BatchRecord>, StoreError> {
        self.db
            .read(|connection| {
                let mut statement = connection
                    .prepare("SELECT commit_id, entries FROM batch ORDER BY commit_id")?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows.into_iter()
                    .map(|(commit_id, entries)| {
                        Ok(BatchRecord {
                            commit_id: uuid_of(&commit_id, "batch commit id")?,
                            entries: serde_json::from_str::<Vec<BatchEntry>>(&entries)
                                .map_err(|_| DbError::Malformed("batch entries"))?,
                        })
                    })
                    .collect()
            })
            .map_err(|cause| failed("load_batches", &cause))
    }

    fn clear_batch(&self, commit_id: Uuid) -> Result<(), StoreError> {
        self.db
            .write(|transaction| {
                transaction.execute(
                    "DELETE FROM batch WHERE commit_id = ?1",
                    [commit_id.as_bytes().to_vec()],
                )?;
                Ok(())
            })
            .map_err(|cause| failed("clear_batch", &cause))
    }
}
