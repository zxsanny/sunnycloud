//! The pending assets of one run: each is hashed (the hash and key stored before its upload), the next `hash_ahead`
//! are hashed while it uploads, and it is marked `uploaded` only after the commit acknowledgement. Files of at
//! most 2 MiB go through the batch path, larger ones through the block path.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use serde_json::{Value, json};
use sync_core::{
    BatchRunError, CheckpointStore, HashCheckpoint, HashedFile, MemoryCheckpointStore,
    ProgressSink, SmallFile, SyncError, UploadCheckpoint, UploadRequest, UploadState,
    takes_staged_path,
};
use sync_core_proto::{BatchEntry, CommitOutcome, FileHash, client_key_for_mobile};
use uuid::Uuid;

use super::Backup;
use super::sink::ParentRefusals;
use super::verdict::Halt;
use crate::checkpoint::{Acknowledged, AssetEnd, PendingAsset};
use crate::outcome::{BackupOutcome, RunContext, RunError};
use crate::source::{AssetSource, OpenError, ReadableFd};

/// Counts of bytes are the host's concern (E-27/c07 `status`); the loop needs none.
struct NoProgress;

impl ProgressSink for NoProgress {
    fn on_progress(&self, _done: u64, _total: u64) {}
}

/// The server no longer has the folder: it is recreated once and the asset sent again.
fn is_parent_error(error: &SyncError) -> bool {
    matches!(error, SyncError::ParentNotFound | SyncError::ParentGone)
}

/// An asset whose hash is stored and whose original is open.
#[derive(Clone)]
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
    /// The end of the run's time budget; a budget too long to count is no deadline.
    deadline: Option<Instant>,
    source: Arc<dyn AssetSource>,
    folder: Option<Uuid>,
    /// The location flag last stored, so an unchanged flag is not written again.
    redacted: Option<bool>,
}

impl<'a> Run<'a> {
    pub(super) fn new(backup: &'a Backup, ctx: RunContext, source: Arc<dyn AssetSource>) -> Self {
        Self {
            backup,
            deadline: ctx
                .deadline
                .and_then(|budget| Instant::now().checked_add(budget)),
            source,
            folder: None,
            redacted: None,
        }
    }

    pub(super) async fn work(&mut self) -> Result<BackupOutcome, Halt> {
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
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn partial(&self) -> Result<BackupOutcome, Halt> {
        Ok(self.backup.partial()?)
    }

    /// `Some(value)` when `result` is `Ok`. A failed call settles `asset_id` (ended, or a pause or fatal error that
    /// stops the run) and gives `None`.
    fn settled<T>(&self, asset_id: i64, result: Result<T, SyncError>) -> Result<Option<T>, Halt> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) => self.backup.settle(asset_id, error).map(|()| None),
        }
    }

    fn note_redaction(&mut self, redacted: bool) -> Result<(), RunError> {
        if self.redacted != Some(redacted) {
            self.backup.assets.set_location_redacted(redacted)?;
            self.redacted = Some(redacted);
        }
        Ok(())
    }

    /// Opens the original and gets its hash: from the row when it was stored, otherwise by hashing now. `None` when
    /// the asset ended instead (gone, or an error that closes it).
    async fn prepare(&mut self, row: PendingAsset) -> Result<Option<Ready>, Halt> {
        let opened = match self.source.open_read(row.asset_id) {
            Ok(opened) => opened,
            Err(OpenError::Gone) => {
                self.backup
                    .assets
                    .mark_ended(row.asset_id, AssetEnd::Gone)?;
                return Ok(None);
            }
            Err(OpenError::Failed) => {
                self.backup
                    .fail_asset(row.asset_id, "the host could not open the original")?;
                return Ok(None);
            }
        };
        self.note_redaction(opened.location_redacted)?;
        let source = opened.file;
        let size = source
            .len()
            .map_err(|error| SyncError::Io { kind: error.kind() });
        let Some(size) = self.settled(row.asset_id, size)? else {
            return Ok(None);
        };
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
            None => match self.hash(row.asset_id, &source, staged).await? {
                Some(hashed) => hashed,
                None => return Ok(None),
            },
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
    ) -> Result<Option<(FileHash, Uuid, Option<HashedFile>)>, Halt> {
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
            .await;
        let Some(hashed) = self.settled(asset_id, hashed)? else {
            return Ok(None);
        };
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
        Ok(Some((file_hash, key, (!staged).then_some(hashed))))
    }

    async fn parent(&mut self) -> Result<Uuid, Halt> {
        if let Some(folder) = self.folder {
            return Ok(folder);
        }
        let folder = self
            .backup
            .camera_folder()
            .await
            .map_err(|error| self.backup.halt_run(error))?;
        self.folder = Some(folder);
        Ok(folder)
    }

    fn client_meta(&self, asset_id: i64) -> Value {
        json!({ "device": self.backup.device_id.to_string(), "asset_id": asset_id.to_string() })
    }

    /// The folder is made again, its id stored, and the run goes on with it.
    async fn recreate_parent(&mut self) -> Result<Uuid, Halt> {
        let folder = self
            .backup
            .recreate_camera_folder()
            .await
            .map_err(|error| self.backup.halt_run(error))?;
        self.folder = Some(folder);
        Ok(folder)
    }

    async fn upload_blocks(&mut self, item: Ready, hashed: HashedFile) -> Result<(), Halt> {
        let parent = self.parent().await?;
        let mut outcome = self.send_blocks(&item, &hashed, parent).await;
        if outcome.as_ref().is_err_and(is_parent_error) {
            let parent = self.recreate_parent().await?;
            outcome = self.send_blocks(&item, &hashed, parent).await;
            if let Some(error) = outcome
                .as_ref()
                .err()
                .filter(|error| is_parent_error(error))
            {
                return Err(self.backup.halt_recorded(error.clone()));
            }
        }
        if let Some(outcome) = self.settled(item.asset_id, outcome)? {
            self.backup
                .assets
                .mark_uploaded(&[acknowledged(item.key, outcome)])?;
        }
        Ok(())
    }

    async fn send_blocks(
        &self,
        item: &Ready,
        hashed: &HashedFile,
        parent: Uuid,
    ) -> Result<CommitOutcome, SyncError> {
        let request = UploadRequest {
            local_key: item.asset_id.to_string(),
            parent,
            name: item.name.clone(),
            source: Arc::clone(&item.source),
            hashed: hashed.clone(),
            base_revision: None,
            client_key: item.key,
            client_meta: Some(self.client_meta(item.asset_id)),
            import_hint: false,
        };
        self.backup
            .api
            .upload(request, self.backup.store.clone(), Arc::new(NoProgress))
            .await
    }

    /// The results are stored by the batch sink before `sync-core` clears the batch record. The entries the server
    /// refused for a missing folder are sent once more into the recreated folder; a second refusal ends the run.
    async fn upload_staged(&mut self, group: Vec<Ready>) -> Result<(), Halt> {
        let parent = self.parent().await?;
        let refused = self.send_staged(&group, parent).await?;
        if refused.is_empty() {
            return Ok(());
        }
        let retry: Vec<Ready> = group
            .into_iter()
            .filter(|item| refused.iter().any(|(key, _)| *key == item.key))
            .collect();
        let parent = self.recreate_parent().await?;
        match self.send_staged(&retry, parent).await?.into_iter().next() {
            Some((_, error)) => Err(self.backup.halt_recorded(error)),
            None => Ok(()),
        }
    }

    /// The keys of the group the server refused because the folder is missing, with the error of each.
    async fn send_staged(
        &self,
        group: &[Ready],
        parent: Uuid,
    ) -> Result<Vec<(Uuid, SyncError)>, Halt> {
        let files = group
            .iter()
            .map(|item| SmallFile {
                entry: BatchEntry {
                    staged_key: item.key,
                    parent,
                    name: item.name.clone(),
                    base_revision: None,
                    file_hash: item.file_hash,
                    client_meta: Some(self.client_meta(item.asset_id)),
                    import_hint: None,
                },
                source: Arc::clone(&item.source),
            })
            .collect();
        let sink = Arc::new(ParentRefusals::new(Arc::clone(&self.backup.assets)));
        let sent = self
            .backup
            .api
            .upload_small(files, self.backup.store.clone(), sink.clone())
            .await;
        match sent {
            Ok(left_out) => {
                self.backup.settle_left_out(left_out)?;
                Ok(sink.take())
            }
            Err(BatchRunError::Sync(error)) if is_parent_error(&error) => {
                Ok(group.iter().map(|item| (item.key, error.clone())).collect())
            }
            Err(error) => Err(self.backup.halted(error)),
        }
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
