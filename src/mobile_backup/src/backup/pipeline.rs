//! The pending assets of one run: each is hashed (the hash and key stored before its upload), the next `hash_ahead`
//! are hashed while it uploads, and it is marked `uploaded` only after the commit acknowledgement. Files of at
//! most 2 MiB go through the batch path, larger ones through the block path.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use serde_json::{Value, json};
use sync_core::{
    BatchSink, CheckpointStore, HashCheckpoint, HashedFile, MemoryCheckpointStore, ProgressSink,
    SmallFile, SyncError, UploadCheckpoint, UploadRequest, UploadState, takes_staged_path,
};
use sync_core_proto::{BatchEntry, CommitOutcome, FileHash, client_key_for_mobile};
use tracing::warn;
use uuid::Uuid;

use super::Backup;
use crate::checkpoint::{Acknowledged, PendingAsset};
use crate::outcome::{BackupOutcome, RunContext, RunError};
use crate::source::{AssetGone, AssetSource, ReadableFd};

/// Counts of bytes are the host's concern (E-27/c07 `status`); the loop needs none.
struct NoProgress;

impl ProgressSink for NoProgress {
    fn on_progress(&self, _done: u64, _total: u64) {}
}

/// An asset whose hash is stored and whose original is open.
struct Ready {
    asset_id: i64,
    name: String,
    source: ReadableFd,
    key: Uuid,
    file_hash: FileHash,
    /// The block hashes of a file that takes the block path; `None` for a file of the batch path.
    blocks: Option<HashedFile>,
}

pub(super) struct Run<'a> {
    backup: &'a Backup,
    ctx: RunContext,
    source: Arc<dyn AssetSource>,
    folder: Option<Uuid>,
}

impl<'a> Run<'a> {
    pub(super) fn new(backup: &'a Backup, ctx: RunContext, source: Arc<dyn AssetSource>) -> Self {
        Self {
            backup,
            ctx,
            source,
            folder: None,
        }
    }

    pub(super) async fn drive(mut self) -> Result<BackupOutcome, RunError> {
        match self.work().await {
            Err(RunError::Sync(SyncError::Cancelled)) => self.partial(),
            other => other,
        }
    }

    async fn work(&mut self) -> Result<BackupOutcome, RunError> {
        let window = usize::try_from(self.backup.config.hash_ahead)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        let batch_max = self.backup.api.batch_commit_max().max(1);
        let mut ready: VecDeque<Ready> = VecDeque::new();
        let mut after = i64::MIN;
        let mut listed_all = false;
        loop {
            while !listed_all && ready.len() < window {
                if self.stopped() {
                    return self.partial();
                }
                let rows = self.backup.assets.pending_after(after, 1)?;
                let Some(row) = rows.into_iter().next() else {
                    listed_all = true;
                    break;
                };
                after = row.asset_id;
                if let Some(item) = self.prepare(row).await? {
                    ready.push_back(item);
                }
            }
            let Some(mut first) = ready.pop_front() else {
                return Ok(BackupOutcome::Completed);
            };
            if self.stopped() {
                return self.partial();
            }
            match first.blocks.take() {
                Some(hashed) => self.upload_blocks(first, hashed).await?,
                None => {
                    let mut group = vec![first];
                    while group.len() < batch_max
                        && ready.front().is_some_and(|next| next.blocks.is_none())
                    {
                        group.extend(ready.pop_front());
                    }
                    self.upload_staged(group).await?;
                }
            }
        }
    }

    fn stopped(&self) -> bool {
        self.backup.cancel.load(Ordering::Acquire)
            || self
                .ctx
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn partial(&self) -> Result<BackupOutcome, RunError> {
        Ok(BackupOutcome::Partial {
            remaining: self.backup.assets.pending_count()?,
        })
    }

    /// Opens the original and gets its hash: from the row when it was stored, otherwise by hashing now. `None` when
    /// the asset is gone.
    async fn prepare(&self, row: PendingAsset) -> Result<Option<Ready>, RunError> {
        let source = match self.source.open_read(row.asset_id) {
            Ok(source) => source,
            Err(AssetGone) => {
                self.backup.assets.mark_skipped_gone(row.asset_id)?;
                return Ok(None);
            }
        };
        let size = source
            .len()
            .map_err(|error| RunError::Source(error.kind()))?;
        let staged = takes_staged_path(size);
        let stored = match row.hashed {
            Some((hash, key)) if staged => Some((hash, key, None)),
            Some((hash, key)) => self
                .stored_blocks(row.asset_id, hash)?
                .map(|hashed| (hash, key, Some(hashed))),
            None => None,
        };
        let (file_hash, key, blocks) = match stored {
            Some(stored) => stored,
            None => self.hash(row.asset_id, &source, staged).await?,
        };
        Ok(Some(Ready {
            asset_id: row.asset_id,
            name: row.upload_name,
            source,
            key,
            file_hash,
            blocks,
        }))
    }

    /// The block hashes a large asset's resume record holds for this content hash.
    fn stored_blocks(
        &self,
        asset_id: i64,
        file_hash: FileHash,
    ) -> Result<Option<HashedFile>, RunError> {
        let record = self.backup.store.load(&asset_id.to_string())?;
        Ok(record
            .filter(|record| record.file_hash == Some(file_hash) && !record.block_hashes.is_empty())
            .map(|record| HashedFile {
                block_hashes: record.block_hashes,
                file_hash,
            }))
    }

    /// Hashing is not resumed across a kill, so its progress goes to a store that is dropped; the finished hash is
    /// what is stored, on the row and, for a large file, as the resume record the block path starts from.
    async fn hash(
        &self,
        asset_id: i64,
        source: &ReadableFd,
        staged: bool,
    ) -> Result<(FileHash, Uuid, Option<HashedFile>), RunError> {
        let local_key = asset_id.to_string();
        let hashed = self
            .backup
            .api
            .hash_file(
                Arc::clone(source),
                Arc::new(NoProgress),
                HashCheckpoint {
                    store: Arc::new(MemoryCheckpointStore::new()),
                    local_key: local_key.clone(),
                    commit_id: Uuid::nil(),
                },
            )
            .await?;
        let key = client_key_for_mobile(self.backup.device_id, &local_key, &hashed.file_hash);
        if !staged {
            self.backup.store.save(
                &local_key,
                UploadCheckpoint {
                    session_id: None,
                    block_hashes: hashed.block_hashes.clone(),
                    file_hash: Some(hashed.file_hash),
                    commit_id: key,
                    state: UploadState::Uploading,
                },
            )?;
        }
        self.backup
            .assets
            .mark_hashed(asset_id, &hashed.file_hash, key)?;
        let file_hash = hashed.file_hash;
        Ok((file_hash, key, (!staged).then_some(hashed)))
    }

    async fn parent(&mut self) -> Result<Uuid, RunError> {
        if let Some(folder) = self.folder {
            return Ok(folder);
        }
        let folder = self.backup.camera_folder().await?;
        self.folder = Some(folder);
        Ok(folder)
    }

    fn client_meta(&self, asset_id: i64) -> Value {
        json!({ "device": self.backup.device_id.to_string(), "asset_id": asset_id.to_string() })
    }

    async fn upload_blocks(&mut self, item: Ready, hashed: HashedFile) -> Result<(), RunError> {
        let request = UploadRequest {
            local_key: item.asset_id.to_string(),
            parent: self.parent().await?,
            name: item.name,
            source: item.source,
            hashed,
            base_revision: None,
            client_key: item.key,
            client_meta: Some(self.client_meta(item.asset_id)),
            import_hint: false,
        };
        let outcome = self
            .backup
            .api
            .upload(request, self.backup.store.clone(), Arc::new(NoProgress))
            .await?;
        self.backup
            .assets
            .mark_uploaded(&[acknowledged(item.key, outcome)])?;
        Ok(())
    }

    /// The results are stored by the batch sink before `sync-core` clears the batch record.
    async fn upload_staged(&mut self, group: Vec<Ready>) -> Result<(), RunError> {
        let parent = self.parent().await?;
        let files = group
            .into_iter()
            .map(|item| SmallFile {
                entry: BatchEntry {
                    staged_key: item.key,
                    parent,
                    name: item.name,
                    base_revision: None,
                    file_hash: item.file_hash,
                    client_meta: Some(self.client_meta(item.asset_id)),
                    import_hint: None,
                },
                source: item.source,
            })
            .collect();
        let sink = Arc::clone(&self.backup.assets) as Arc<dyn BatchSink>;
        let left_out = self
            .backup
            .api
            .upload_small(files, self.backup.store.clone(), sink)
            .await?;
        for file in left_out {
            warn!(error = %file.error, "a staged file was left out of its batch");
        }
        Ok(())
    }
}

fn acknowledged(client_key: Uuid, outcome: CommitOutcome) -> Acknowledged {
    let (node_id, revision_id) = match outcome {
        CommitOutcome::Published {
            node_id,
            revision_id,
        } => (Some(node_id), Some(revision_id)),
        CommitOutcome::ConflictCopy { copy_node_id, .. } => (Some(copy_node_id), None),
        CommitOutcome::NothingToSend { revision_id } => (None, Some(revision_id)),
    };
    Acknowledged {
        client_key,
        node_id,
        revision_id,
    }
}
