//! `UploadService` for [`PgUploadService`]: each method is the inherent one of the same name.

use contracts::{
    BlockAck, ByteStream, Staged, StartUpload, StartUploadOutcome, UploadError, UploadService,
    UploadState, async_trait,
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
