//! `UploadService`: large uploads in blocks and staged small uploads.

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_ids::{FileHash, Hash32};
use thiserror::Error;
use uuid::Uuid;

use crate::blob::ByteStream;
use crate::common::Fault;

/// `client_key` is opaque to the server (unique per account; its size cap is 03's). `client_meta` is JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartUpload {
    pub parent_id: Uuid,
    pub name: String,
    pub declared_size: i64,
    pub client_key: String,
    pub content_hash: Option<FileHash>,
    pub base_revision: Option<Uuid>,
    pub client_meta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub upload_id: Uuid,
    pub need_blocks: Vec<u32>,
    pub block_size: u32,
    pub lane_limit: u32,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartUploadOutcome {
    Started(Started),
    /// The node already holds a revision with this content hash.
    NothingToSend {
        revision: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadState {
    pub need_blocks: Vec<u32>,
    pub received_count: u32,
    pub expires_at: Timestamp,
}

/// The block is durable on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockAck;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Staged {
    pub staged_key: Uuid,
    pub size: i64,
}

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("invalid name")]
    InvalidName,
    #[error("parent not found")]
    ParentNotFound,
    #[error("not a folder")]
    NotAFolder,
    #[error("size too large")]
    SizeTooLarge,
    #[error("quota exceeded")]
    QuotaExceeded,
    #[error("session cap reached")]
    SessionCapReached,
    #[error("staged cap reached")]
    StagedCapReached,
    #[error("free space floor")]
    FreeSpaceFloor,
    #[error("client key mismatch")]
    ClientKeyMismatch,
    #[error("account not active")]
    AccountNotActive,
    #[error("upload not found")]
    UploadNotFound,
    #[error("index out of range")]
    IndexOutOfRange,
    #[error("length mismatch")]
    LengthMismatch,
    #[error("hash mismatch")]
    HashMismatch,
    #[error("block conflict")]
    BlockConflict,
    #[error("overloaded")]
    Overloaded,
    #[error("storage full")]
    StorageFull,
    #[error("storage failure")]
    Storage(#[source] Fault),
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait UploadService: Send + Sync {
    /// Idempotent by `(account, client_key)`: a repeat returns the existing session.
    async fn start_upload(
        &self,
        account: Uuid,
        request: StartUpload,
    ) -> Result<StartUploadOutcome, UploadError>;

    async fn get_upload(&self, account: Uuid, upload_id: Uuid) -> Result<UploadState, UploadError>;

    async fn put_block(
        &self,
        account: Uuid,
        upload_id: Uuid,
        index: u32,
        expected_hash: Hash32,
        body: ByteStream,
        length: u64,
    ) -> Result<BlockAck, UploadError>;

    async fn abort_upload(&self, account: Uuid, upload_id: Uuid) -> Result<(), UploadError>;

    /// Single request, `length` in `0..=2 MiB`; idempotent by `staged_key`. `expected_hash` is `None` only for
    /// a zero-length body.
    async fn stage_small(
        &self,
        account: Uuid,
        staged_key: Uuid,
        expected_hash: Option<Hash32>,
        body: ByteStream,
        length: u64,
    ) -> Result<Staged, UploadError>;
}
