//! Recording stubs of the ports whose arguments are not `Debug` (a request body stream, 01's revalidator). Each
//! call is one line of the ordered log of the [`Script`]; a body is drained and logged as its byte count.

use std::future::poll_fn;
use std::sync::Arc;
use std::time::Duration;

use contracts::*;
use sc_ids::Hash32;
use uuid::Uuid;

use super::script::{Script, unscripted};

async fn drain(mut body: ByteStream) -> usize {
    let mut bytes = 0;
    while let Some(chunk) = poll_fn(|cx| body.as_mut().poll_next(cx)).await {
        bytes += chunk
            .expect("the request body reaches the port intact")
            .len();
    }
    bytes
}

pub struct RecordingUploads(Script);

impl RecordingUploads {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, UploadError> {
        self.0
            .take::<Result<T, UploadError>>(key)
            .unwrap_or_else(|| Err(UploadError::Unexpected(unscripted(key))))
    }
}

#[contracts::async_trait]
impl UploadService for RecordingUploads {
    async fn start_upload(
        &self,
        account: Uuid,
        request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError> {
        self.0.record(format!(
            "UploadService::start_upload ({account}, {request:?})"
        ));
        self.next("UploadService::start_upload")
    }

    async fn get_upload(&self, account: Uuid, upload_id: Uuid) -> Result<UploadState, UploadError> {
        self.0.record(format!(
            "UploadService::get_upload ({account}, {upload_id})"
        ));
        self.next("UploadService::get_upload")
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
        let bytes = drain(body).await;
        self.0.record(format!(
            "UploadService::put_block ({account}, {upload_id}, {index}, {expected_hash}, length={length}, bytes={bytes})"
        ));
        self.next("UploadService::put_block")
    }

    async fn abort_upload(&self, account: Uuid, upload_id: Uuid) -> Result<(), UploadError> {
        self.0.record(format!(
            "UploadService::abort_upload ({account}, {upload_id})"
        ));
        self.next("UploadService::abort_upload")
    }

    async fn stage_small(
        &self,
        account: Uuid,
        staged_key: Uuid,
        expected_hash: Option<Hash32>,
        body: ByteStream,
        length: u64,
    ) -> Result<Staged, UploadError> {
        let bytes = drain(body).await;
        let hash = expected_hash.map_or_else(|| "none".to_owned(), |hash| hash.to_string());
        self.0.record(format!(
            "UploadService::stage_small ({account}, {staged_key}, hash={hash}, length={length}, bytes={bytes})"
        ));
        self.next("UploadService::stage_small")
    }
}

/// 05's feed. `read` calls the revalidator it was given and logs the answer, so the test sees that the very
/// revalidator of the services reached the port.
pub struct RecordingFeed(Script);

impl RecordingFeed {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, ChangeFeedError> {
        self.0
            .take::<Result<T, ChangeFeedError>>(key)
            .unwrap_or_else(|| Err(ChangeFeedError::Unexpected(unscripted(key))))
    }
}

#[contracts::async_trait]
impl ChangeFeed for RecordingFeed {
    async fn read(
        &self,
        principal: &Principal,
        revalidator: &dyn PrincipalRevalidator,
        cursor: Cursor,
        wait: Option<Duration>,
        limit: Option<u32>,
    ) -> Result<ChangePage, ChangeFeedError> {
        let revalidated = revalidator.revalidate(principal).await;
        self.0.record(format!(
            "ChangeFeed::read ({principal:?}, {cursor:?}, {wait:?}, {limit:?}) revalidated={revalidated:?}"
        ));
        self.next("ChangeFeed::read")
    }

    async fn head(&self, account: Uuid) -> Result<Cursor, ChangeFeedError> {
        self.0.record(format!("ChangeFeed::head ({account})"));
        self.next("ChangeFeed::head")
    }
}
