//! Per-account fakes: each wraps a recording stub, asks the [`World`] whether the caller owns every id of the
//! call, answers the port's own outcome for an id it does not own, and passes an owned call on to the stub.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use contracts::*;
use sc_ids::{FileHash, Hash32};
use uuid::Uuid;

use super::world::World;
use crate::harness::recording::{
    RecordingCommits, RecordingDerived, RecordingDownloads, RecordingShares, RecordingTree,
    RecordingTreeReader,
};
use crate::harness::recording_stream::RecordingUploads;

/// The current revision of every node in the world: a base revision the caller does not own is never it.
pub const CURRENT_REVISION: Uuid = Uuid::from_u128(0xC0DE);

pub struct Gate<T> {
    pub(super) inner: Arc<T>,
    world: Arc<World>,
}

impl<T> Gate<T> {
    pub fn new(inner: Arc<T>, world: &Arc<World>) -> Arc<Gate<T>> {
        Arc::new(Gate {
            inner,
            world: world.clone(),
        })
    }

    pub(super) fn admit(&self, key: &'static str, account: Uuid, ids: &[Uuid]) -> bool {
        self.world.admit(key, account, ids)
    }

    fn owns(&self, account: Uuid, id: Uuid) -> bool {
        self.world.owns(account, id)
    }
}

#[contracts::async_trait]
impl TreeService for Gate<RecordingTree> {
    async fn create_folder(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<NodeView, TreeServiceError> {
        if !self.admit("TreeService::create_folder", account, &[parent_id]) {
            return Err(TreeServiceError::NotFound);
        }
        self.inner.create_folder(account, parent_id, name).await
    }

    async fn update_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_parent: Option<Uuid>,
    ) -> Result<NodeView, TreeServiceError> {
        let ids: Vec<Uuid> = [node_id].into_iter().chain(new_parent).collect();
        if !self.admit("TreeService::update_node", account, &ids) {
            return Err(TreeServiceError::NotFound);
        }
        self.inner
            .update_node(account, node_id, new_name, new_parent)
            .await
    }

    async fn delete_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        base_revision: Option<Uuid>,
    ) -> Result<(), TreeServiceError> {
        let ids: Vec<Uuid> = [node_id].into_iter().chain(base_revision).collect();
        if !self.admit("TreeService::delete_node", account, &ids) {
            if !self.owns(account, node_id) {
                return Err(TreeServiceError::NotFound);
            }
            return Err(TreeServiceError::EditedSince {
                current_revision: CURRENT_REVISION,
            });
        }
        self.inner
            .delete_node(account, node_id, base_revision)
            .await
    }

    async fn resolve_path(&self, account: Uuid, path: &str) -> Result<Uuid, TreeServiceError> {
        self.inner.resolve_path(account, path).await
    }

    async fn find_revisions(
        &self,
        account: Uuid,
        client_device: Option<&str>,
        page: Page,
    ) -> Result<Vec<RevisionRef>, TreeServiceError> {
        self.inner
            .find_revisions(account, client_device, page)
            .await
    }
}

#[contracts::async_trait]
impl TreeReader for Gate<RecordingTreeReader> {
    async fn get_node(
        &self,
        account: Uuid,
        selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError> {
        if let NodeSelector::Id(node) = selector
            && !self.admit("TreeReader::get_node", account, &[node])
        {
            return Err(TreeReaderError::NotFound);
        }
        self.inner.get_node(account, selector).await
    }

    async fn list_children(
        &self,
        account: Uuid,
        folder_id: Uuid,
        after_name_key: Option<&str>,
        limit: u32,
    ) -> Result<ListingPage, TreeReaderError> {
        if !self.admit("TreeReader::list_children", account, &[folder_id]) {
            return Err(TreeReaderError::NotFound);
        }
        self.inner
            .list_children(account, folder_id, after_name_key, limit)
            .await
    }

    async fn find_live_node(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError> {
        self.inner.find_live_node(account, parent_id, name).await
    }

    async fn resolve_live_file(
        &self,
        account: Uuid,
        node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError> {
        self.inner.resolve_live_file(account, node_id).await
    }

    async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError> {
        self.inner.root_node(account).await
    }

    async fn get_revision(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<RevisionView, TreeReaderError> {
        if !self.admit("TreeReader::get_revision", account, &[revision_id]) {
            return Err(TreeReaderError::NotFound);
        }
        self.inner.get_revision(account, revision_id).await
    }

    async fn is_current_or_retained(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<bool, TreeReaderError> {
        self.inner
            .is_current_or_retained(account, revision_id)
            .await
    }

    async fn describe_blobs(
        &self,
        blob_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError> {
        self.inner.describe_blobs(blob_ids).await
    }

    async fn lookup_revisions(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<Vec<RevisionMatch>, TreeReaderError> {
        self.inner.lookup_revisions(account, hashes).await
    }
}

#[contracts::async_trait]
impl UploadService for Gate<RecordingUploads> {
    async fn start_upload(
        &self,
        account: Uuid,
        request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError> {
        let ids: Vec<Uuid> = [request.parent_id]
            .into_iter()
            .chain(request.base_revision)
            .collect();
        if !self.admit("UploadService::start_upload", account, &ids) {
            return Err(UploadError::ParentNotFound);
        }
        self.inner.start_upload(account, request).await
    }

    async fn get_upload(&self, account: Uuid, upload_id: Uuid) -> Result<UploadState, UploadError> {
        if !self.admit("UploadService::get_upload", account, &[upload_id]) {
            return Err(UploadError::UploadNotFound);
        }
        self.inner.get_upload(account, upload_id).await
    }

    async fn put_block(
        &self,
        account: Uuid,
        upload_id: Uuid,
        index: u32,
        expected_hash: Hash32,
        body: ByteStream,
        length: u64,
    ) -> Result<BlockAck, UploadError> {
        if !self.admit("UploadService::put_block", account, &[upload_id]) {
            return Err(UploadError::UploadNotFound);
        }
        self.inner
            .put_block(account, upload_id, index, expected_hash, body, length)
            .await
    }

    async fn abort_upload(&self, account: Uuid, upload_id: Uuid) -> Result<(), UploadError> {
        if !self.admit("UploadService::abort_upload", account, &[upload_id]) {
            return Err(UploadError::UploadNotFound);
        }
        self.inner.abort_upload(account, upload_id).await
    }

    /// Staged rows are keyed `(account, staged_key)`: a key of another account is a fresh key of the caller.
    async fn stage_small(
        &self,
        account: Uuid,
        staged_key: Uuid,
        expected_hash: Option<Hash32>,
        body: ByteStream,
        length: u64,
    ) -> Result<Staged, UploadError> {
        self.admit("UploadService::stage_small", account, &[staged_key]);
        self.inner
            .stage_small(account, staged_key, expected_hash, body, length)
            .await
    }
}

#[contracts::async_trait]
impl CommitService for Gate<RecordingCommits> {
    async fn commit_upload(
        &self,
        actor: CommitActor,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        let account = actor.account_id;
        let ids: Vec<Uuid> = [request.upload_id, request.parent_id]
            .into_iter()
            .chain(request.base_revision)
            .collect();
        if !self.admit("CommitService::commit_upload", account, &ids) {
            return Err(if !self.owns(account, request.upload_id) {
                CommitServiceError::UploadNotFound
            } else if !self.owns(account, request.parent_id) {
                CommitServiceError::ParentNotFound
            } else {
                CommitServiceError::EditedSince
            });
        }
        self.inner.commit_upload(actor, request).await
    }

    /// An entry naming an id the caller does not own fails alone, inside a 200 (03 section 4).
    async fn commit_batch(
        &self,
        actor: CommitActor,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        let account = actor.account_id;
        let ids: Vec<Uuid> = request
            .entries
            .iter()
            .flat_map(|entry| {
                [entry.staged_key, entry.parent_id]
                    .into_iter()
                    .chain(entry.base_revision)
            })
            .collect();
        if self.admit("CommitService::commit_batch", account, &ids) {
            return self.inner.commit_batch(actor, request).await;
        }
        let per_entry = request
            .entries
            .iter()
            .map(|entry| {
                if !self.owns(account, entry.staged_key) {
                    BatchEntryResult::Failed(EntryError::UploadNotFound)
                } else if !self.owns(account, entry.parent_id) {
                    BatchEntryResult::Failed(EntryError::ParentNotFound)
                } else if entry
                    .base_revision
                    .is_some_and(|base| !self.owns(account, base))
                {
                    BatchEntryResult::Failed(EntryError::EditedSince)
                } else {
                    BatchEntryResult::Committed(Committed {
                        node_id: entry.parent_id,
                        revision_id: CURRENT_REVISION,
                        conflict_copy: None,
                    })
                }
            })
            .collect();
        Ok(BatchResult { per_entry })
    }
}

#[contracts::async_trait]
impl DownloadService for Gate<RecordingDownloads> {
    async fn open_revision_content(
        &self,
        account: Uuid,
        target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        let id = match target {
            RevisionOrCurrent::Revision(id) | RevisionOrCurrent::Current(id) => id,
        };
        if !self.admit("DownloadService::open_revision_content", account, &[id]) {
            return Err(DownloadError::NotFound);
        }
        self.inner
            .open_revision_content(account, target, options)
            .await
    }

    async fn open_blob_content(
        &self,
        account: Uuid,
        blob: BlobRef,
        media_type: Option<String>,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        self.inner
            .open_blob_content(account, blob, media_type, options)
            .await
    }
}

#[contracts::async_trait]
impl DerivedOpener for Gate<RecordingDerived> {
    async fn open_derived(
        &self,
        account: Uuid,
        node_id: Uuid,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        if !self.admit("DerivedOpener::open_derived", account, &[node_id]) {
            return Err(DerivedError::NotFound);
        }
        self.inner.open_derived(account, node_id, kind).await
    }

    async fn open_derived_for(
        &self,
        account: Uuid,
        file: &LiveFile,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError> {
        self.inner.open_derived_for(account, file, kind).await
    }
}

#[contracts::async_trait]
impl ShareService for Gate<RecordingShares> {
    async fn create(
        &self,
        account: Uuid,
        node_id: Uuid,
        expires_in: Option<Duration>,
        password: Option<&str>,
    ) -> Result<CreatedShare, ShareError> {
        if !self.admit("ShareService::create", account, &[node_id]) {
            return Err(ShareError::TargetNotFound);
        }
        self.inner
            .create(account, node_id, expires_in, password)
            .await
    }

    /// 06 lists the caller's own links: none on a node the caller does not own.
    async fn list(
        &self,
        account: Uuid,
        node_id: Option<Uuid>,
    ) -> Result<Vec<ShareInfo>, ShareError> {
        let ids: Vec<Uuid> = node_id.into_iter().collect();
        if !self.admit("ShareService::list", account, &ids) {
            return Ok(Vec::new());
        }
        self.inner.list(account, node_id).await
    }

    async fn revoke(&self, account: Uuid, share_id: Uuid) -> Result<(), ShareError> {
        if !self.admit("ShareService::revoke", account, &[share_id]) {
            return Err(ShareError::NotFound);
        }
        self.inner.revoke(account, share_id).await
    }
}
