//! `UploadService`, `CommitService`, `DownloadService` and `ReservationSource` for [`PgUploadService`]: each method is the inherent
//! one of the same name.

use contracts::{
    BatchCommit, BatchResult, BlobRef, BlockAck, ByteStream, CommitActor, CommitService,
    CommitServiceError, CommitUpload, Committed, Download, DownloadError, DownloadOptions,
    DownloadService, ReservationError, ReservationSource, RevisionOrCurrent, Staged, StartUpload,
    StartUploadOutcome, UploadError, UploadService, UploadState, async_trait,
};
use sc_ids::Hash32;
use uuid::Uuid;

use crate::upload::PgUploadService;

#[async_trait]
impl UploadService for PgUploadService {
    async fn start_upload(
        &self,
        account: Uuid,
        request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError> {
        PgUploadService::start_upload(self, account, request).await
    }

    async fn get_upload(&self, account: Uuid, upload_id: Uuid) -> Result<UploadState, UploadError> {
        PgUploadService::get_upload(self, account, upload_id).await
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
        PgUploadService::put_block(self, account, upload_id, index, expected_hash, body, length)
            .await
    }

    async fn abort_upload(&self, account: Uuid, upload_id: Uuid) -> Result<(), UploadError> {
        PgUploadService::abort_upload(self, account, upload_id).await
    }

    async fn stage_small(
        &self,
        account: Uuid,
        staged_key: Uuid,
        expected_hash: Option<Hash32>,
        body: ByteStream,
        length: u64,
    ) -> Result<Staged, UploadError> {
        PgUploadService::stage_small(self, account, staged_key, expected_hash, body, length).await
    }
}

#[async_trait]
impl CommitService for PgUploadService {
    async fn commit_upload(
        &self,
        actor: CommitActor,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        PgUploadService::commit_upload(self, actor, request).await
    }

    async fn commit_batch(
        &self,
        actor: CommitActor,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        PgUploadService::commit_batch(self, actor, request).await
    }
}

#[async_trait]
impl DownloadService for PgUploadService {
    async fn open_revision_content(
        &self,
        account: Uuid,
        target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        PgUploadService::open_revision_content(self, account, target, options).await
    }

    async fn open_blob_content(
        &self,
        account: Uuid,
        blob: BlobRef,
        media_type: Option<String>,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError> {
        PgUploadService::open_blob_content(self, account, blob, media_type, options).await
    }
}

#[async_trait]
impl ReservationSource for PgUploadService {
    async fn open_reserved_bytes(&self, account: Uuid) -> Result<i64, ReservationError> {
        PgUploadService::open_reserved_bytes(self, account).await
    }
}
