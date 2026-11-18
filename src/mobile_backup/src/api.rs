//! The part of `sync-core` the backup loop drives. `upload`, `upload_small` and `resume_batches` are inherent methods
//! of `HttpSyncClient` and not of `SyncApi` (`sync_api.md` open point 9), so this narrow trait is the seam that
//! unit tests fake; `HttpSyncClient` is its one production implementation.

use std::sync::Arc;

use sync_core::{
    BatchRunError, BatchSink, BoxFuture, CheckpointStore, FileSource, HashCheckpoint, HashedFile,
    HttpSyncClient, ProgressSink, SkippedFile, SmallFile, SourceResolver, SyncApi, SyncError,
    UploadRequest,
};
use sync_core_proto::{CommitOutcome, ListingPage, NodeView, PageToken};
use uuid::Uuid;

pub trait BackupApi: Send + Sync {
    /// Ends every operation in flight with `Cancelled`; nothing is sent afterwards. The sync client stays cancelled
    /// for good, so a `Backup` that was cancelled is not run again over the same client.
    fn cancel(&self);

    /// The server's `batch_max_entries`, learnt at connect.
    fn batch_commit_max(&self) -> usize;

    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>>;

    fn list_children<'a>(
        &'a self,
        node: Uuid,
        after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>>;

    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>>;

    fn hash_file<'a>(
        &'a self,
        source: Arc<dyn FileSource>,
        progress: Arc<dyn ProgressSink>,
        checkpoint: HashCheckpoint,
    ) -> BoxFuture<'a, Result<HashedFile, SyncError>>;

    fn upload(
        &self,
        request: UploadRequest,
        checkpoints: Arc<dyn CheckpointStore>,
        progress: Arc<dyn ProgressSink>,
    ) -> BoxFuture<'_, Result<CommitOutcome, SyncError>>;

    fn upload_small(
        &self,
        files: Vec<SmallFile>,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
    ) -> BoxFuture<'_, Result<Vec<SkippedFile>, BatchRunError>>;

    fn resume_batches<'a>(
        &'a self,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
        resolve: SourceResolver<'a>,
    ) -> BoxFuture<'a, Result<Vec<SkippedFile>, BatchRunError>>;
}

impl BackupApi for HttpSyncClient {
    fn cancel(&self) {
        SyncApi::cancel(self);
    }

    fn batch_commit_max(&self) -> usize {
        usize::try_from(self.limits().batch_max_entries).unwrap_or(usize::MAX)
    }

    fn root_node(&self) -> BoxFuture<'_, Result<NodeView, SyncError>> {
        SyncApi::root_node(self)
    }

    fn list_children<'a>(
        &'a self,
        node: Uuid,
        after: Option<&'a PageToken>,
    ) -> BoxFuture<'a, Result<ListingPage, SyncError>> {
        SyncApi::list_children(self, node, after)
    }

    fn create_folder<'a>(
        &'a self,
        parent: Uuid,
        name: &'a str,
    ) -> BoxFuture<'a, Result<NodeView, SyncError>> {
        SyncApi::create_folder(self, parent, name)
    }

    fn hash_file<'a>(
        &'a self,
        source: Arc<dyn FileSource>,
        progress: Arc<dyn ProgressSink>,
        checkpoint: HashCheckpoint,
    ) -> BoxFuture<'a, Result<HashedFile, SyncError>> {
        SyncApi::hash_file(self, source, None, progress, checkpoint)
    }

    fn upload(
        &self,
        request: UploadRequest,
        checkpoints: Arc<dyn CheckpointStore>,
        progress: Arc<dyn ProgressSink>,
    ) -> BoxFuture<'_, Result<CommitOutcome, SyncError>> {
        Box::pin(HttpSyncClient::upload(self, request, checkpoints, progress))
    }

    fn upload_small(
        &self,
        files: Vec<SmallFile>,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
    ) -> BoxFuture<'_, Result<Vec<SkippedFile>, BatchRunError>> {
        Box::pin(HttpSyncClient::upload_small(self, files, store, sink))
    }

    fn resume_batches<'a>(
        &'a self,
        store: Arc<dyn CheckpointStore>,
        sink: Arc<dyn BatchSink>,
        resolve: SourceResolver<'a>,
    ) -> BoxFuture<'a, Result<Vec<SkippedFile>, BatchRunError>> {
        Box::pin(HttpSyncClient::resume_batches(self, store, sink, resolve))
    }
}
