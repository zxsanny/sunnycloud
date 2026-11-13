//! `upload` (`sync_api.md`): the large-file executor. It starts or resumes a session with the stored
//! `client_key`, drives [`UploadPlanner`](sync_core_proto::UploadPlanner) over up to four lanes, commits,
//! and recovers a lost commit answer with the same key (§ Lost-commit recovery).

mod lanes;
mod wire;

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sync_core_proto::{
    BLOCK_SIZE, CommitOutcome, FileHash, RetryPolicy, StartOutcome, effective_lanes,
    reuse_or_new_key,
};
use uuid::Uuid;

use crate::error::SyncError;
use crate::hash_ops::{HashCheckpoint, HashedFile, Hashing, identity_of};
use crate::host::{
    CheckpointStore, FileIdentity, FileSource, ProgressSink, StoreError, UploadCheckpoint,
    UploadState,
};
use crate::request::RequestPath;
use crate::small::blocking;

use lanes::SessionEnd;
use wire::{StartBody, start_upload, unexpected};

const BLOCK: u64 = BLOCK_SIZE as u64;

/// One large upload. No `Debug`: `name` is a file name and never reaches a log.
#[derive(Clone)]
pub struct UploadRequest {
    /// The host's key of this file in its [`CheckpointStore`].
    pub local_key: String,
    pub parent: Uuid,
    pub name: String,
    pub source: Arc<dyn FileSource>,
    /// The result of `hash_file` for the current content.
    pub hashed: HashedFile,
    /// The file's revision the caller last saw; `None` for a new file.
    pub base_revision: Option<Uuid>,
    /// The key of this content when the store holds none for it: a fresh UUIDv7 on desktop, the derived key
    /// on mobile. A key stored for the same file hash wins, so every retry and resume sends one key.
    pub client_key: Uuid,
    pub client_meta: Option<Value>,
    pub import_hint: bool,
}

/// The settings one upload runs with.
pub(crate) struct Uploader<'a> {
    pub(crate) path: &'a Arc<RequestPath>,
    pub(crate) hashing: &'a Hashing,
    /// `Limits.lanes`, `min(4, ServerInfo.max_lanes)`; each session lowers it to its `lane_limit`.
    pub(crate) lanes: u32,
    pub(crate) policy: RetryPolicy,
    /// Added to every planner `Wait`; never one second or more.
    pub(crate) jitter: fn(Duration) -> Duration,
}

/// Why the run asks the server for a session again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Restart {
    /// A block or a session listing answered `upload_session_gone`, or the planner found a listing it cannot use.
    SessionGone,
    /// The commit answered `blocklist_mismatch`: the file is hashed again first.
    Rehash,
}

impl Uploader<'_> {
    /// Sends `request` and returns the server's outcome. The checkpoint is written before the first block and
    /// cleared after an outcome; it is kept on every error, so the next call resumes with the same key. The
    /// store is best effort: a failed write is logged and the upload goes on. `progress` counts the bytes of
    /// blocks the server answered with 204.
    pub(crate) async fn upload(
        &self,
        request: UploadRequest,
        store: Arc<dyn CheckpointStore>,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<CommitOutcome, SyncError> {
        self.path.check()?;
        let opening = identity_of(&request.source).await?;
        let stored = self.stored(&store, &request.local_key).await?;
        let key = key_for(stored, request.hashed.file_hash, request.client_key);
        let mut run = Run {
            up: self,
            request,
            key,
            store,
            progress,
            opening,
        };
        run.drive().await
    }

    async fn stored(
        &self,
        store: &Arc<dyn CheckpointStore>,
        local_key: &str,
    ) -> Result<Option<UploadCheckpoint>, SyncError> {
        let (store, local_key) = (Arc::clone(store), local_key.to_owned());
        match blocking(move || store.load(&local_key)).await? {
            Ok(checkpoint) => Ok(checkpoint),
            Err(StoreError) => {
                tracing::warn!("upload checkpoint could not be read, the request's key is used");
                Ok(None)
            }
        }
    }
}

/// The state of one `upload` call.
struct Run<'a> {
    up: &'a Uploader<'a>,
    request: UploadRequest,
    key: Uuid,
    store: Arc<dyn CheckpointStore>,
    progress: Arc<dyn ProgressSink>,
    /// The source's identity the hashes describe; a difference before a commit is `SourceChanged`.
    opening: FileIdentity,
}

impl Run<'_> {
    async fn drive(&mut self) -> Result<CommitOutcome, SyncError> {
        let mut restarts = 0;
        let mut recovered = false;
        loop {
            self.up.path.check()?;
            let block_count = self.block_count()?;
            let session = match start_upload(self.up.path, &self.start_body()).await? {
                StartOutcome::NothingToSend { revision_id } => {
                    return self
                        .finish(CommitOutcome::NothingToSend { revision_id })
                        .await;
                }
                StartOutcome::Session(session) => session,
            };
            self.save(Some(session.upload_id), UploadState::Uploading)
                .await?;
            let lanes = effective_lanes(self.up.lanes, session.lane_limit);
            let restart = match self.session(&session, block_count, lanes).await? {
                SessionEnd::Committed(outcome) => return self.finish(outcome).await,
                SessionEnd::Restart(restart) => restart,
                SessionEnd::LostCommit(error) if recovered => return Err(error),
                SessionEnd::LostCommit(_) => {
                    tracing::warn!(upload_id = %session.upload_id, "lost-commit recovery");
                    recovered = true;
                    continue;
                }
            };
            restarts += 1;
            if restarts > self.up.policy.max_attempts_per_block.max(1) {
                return Err(match restart {
                    Restart::Rehash => SyncError::BlocklistMismatch,
                    Restart::SessionGone => unexpected("upload_session_gone"),
                });
            }
            if restart == Restart::Rehash {
                self.rehash().await?;
            }
        }
    }

    /// The block count the hashes describe; a source whose size no longer matches them has changed.
    fn block_count(&self) -> Result<u32, SyncError> {
        let blocks = self.request.hashed.block_hashes.len() as u64;
        if blocks != self.opening.size.div_ceil(BLOCK) {
            return Err(SyncError::SourceChanged);
        }
        u32::try_from(blocks).map_err(|_| SyncError::SizeTooLarge)
    }

    fn start_body(&self) -> StartBody<'_> {
        StartBody {
            parent: self.request.parent,
            name: &self.request.name,
            declared_size: self.opening.size,
            client_key: self.key,
            content_hash: &self.request.hashed.file_hash,
            base_revision: self.request.base_revision,
            client_meta: self.request.client_meta.as_ref(),
        }
    }

    /// After `blocklist_mismatch`: hash the file again under the same key and take its current identity.
    async fn rehash(&mut self) -> Result<(), SyncError> {
        let checkpoint = HashCheckpoint {
            store: Arc::clone(&self.store),
            local_key: self.request.local_key.clone(),
            commit_id: self.key,
        };
        self.request.hashed = self
            .up
            .hashing
            .hash_file(
                self.up.path,
                Arc::clone(&self.request.source),
                None,
                Arc::new(Unreported),
                checkpoint,
            )
            .await?;
        self.opening = identity_of(&self.request.source).await?;
        Ok(())
    }

    async fn finish(&self, outcome: CommitOutcome) -> Result<CommitOutcome, SyncError> {
        let (store, local_key) = (Arc::clone(&self.store), self.request.local_key.clone());
        if blocking(move || store.clear(&local_key)).await?.is_err() {
            tracing::warn!("upload checkpoint was not cleared");
        }
        tracing::info!(bytes = self.opening.size, "upload finished");
        Ok(outcome)
    }

    async fn save(&self, session_id: Option<Uuid>, state: UploadState) -> Result<(), SyncError> {
        let record = UploadCheckpoint {
            session_id,
            block_hashes: self.request.hashed.block_hashes.clone(),
            file_hash: Some(self.request.hashed.file_hash),
            commit_id: self.key,
            state,
        };
        let (store, local_key) = (Arc::clone(&self.store), self.request.local_key.clone());
        if blocking(move || store.save(&local_key, record))
            .await?
            .is_err()
        {
            tracing::warn!("upload checkpoint was not stored, the upload continues");
        }
        Ok(())
    }
}

/// Hashing again after a mismatch is not upload progress: only acknowledged blocks are reported.
struct Unreported;

impl ProgressSink for Unreported {
    fn on_progress(&self, _done: u64, _total: u64) {}
}

/// The key an upload of `content` sends: the stored one when it was stored for the same hash (a retry, a
/// resume, a lost commit), else `candidate` (new content).
fn key_for(stored: Option<UploadCheckpoint>, content: FileHash, candidate: Uuid) -> Uuid {
    let stored = stored.and_then(|stored| stored.file_hash.map(|hash| (stored.commit_id, hash)));
    reuse_or_new_key(stored, content, || candidate)
}

/// Bytes of block `index` of a file of `size` bytes.
fn block_len(size: u64, index: u32) -> u64 {
    size.saturating_sub(u64::from(index) * BLOCK).min(BLOCK)
}

#[cfg(test)]
mod tests {
    use sync_core_proto::FileHash;
    use uuid::Uuid;

    use super::{BLOCK, block_len, key_for};
    use crate::host::{CheckpointStore, MemoryCheckpointStore, UploadCheckpoint, UploadState};

    fn v7(n: u128) -> Uuid {
        Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n)
    }

    fn hash(n: u8) -> FileHash {
        FileHash::from([n; 32])
    }

    /// One attempt: the key it sends, stored afterwards as the executor stores it before the first block.
    fn attempt(store: &MemoryCheckpointStore, content: FileHash, fresh: Uuid) -> Uuid {
        let stored = store.load("f").expect("the memory store reads");
        let key = key_for(stored, content, fresh);
        let record = UploadCheckpoint {
            session_id: None,
            block_hashes: Vec::new(),
            file_hash: Some(content),
            commit_id: key,
            state: UploadState::Uploading,
        };
        store.save("f", record).expect("the memory store writes");
        key
    }

    #[test]
    fn three_attempts_through_a_memory_store_send_one_key_and_new_content_a_new_one() {
        // Arrange
        let store = MemoryCheckpointStore::new();

        // Act
        let keys: Vec<Uuid> = (1..=3).map(|n| attempt(&store, hash(1), v7(n))).collect();
        let edited = attempt(&store, hash(2), v7(4));

        // Assert
        assert_eq!(keys, vec![v7(1); 3]);
        assert_eq!(keys[0].get_version_num(), 7);
        assert_eq!(edited, v7(4));
    }

    #[test]
    fn a_hashing_checkpoint_without_a_file_hash_keeps_the_candidate() {
        // Arrange
        let stored = UploadCheckpoint {
            session_id: None,
            block_hashes: Vec::new(),
            file_hash: None,
            commit_id: v7(9),
            state: UploadState::Hashing,
        };

        // Act
        let key = key_for(Some(stored), hash(1), v7(1));

        // Assert
        assert_eq!(key, v7(1));
    }

    #[test]
    fn the_last_block_holds_the_remainder() {
        // Arrange
        let size = 2 * BLOCK + 5;

        // Act
        let lens: Vec<u64> = (0..3).map(|index| block_len(size, index)).collect();

        // Assert
        assert_eq!(lens, vec![BLOCK, BLOCK, 5]);
    }
}
