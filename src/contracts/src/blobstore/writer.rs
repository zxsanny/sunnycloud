use async_trait::async_trait;
use uuid::Uuid;

use super::error::BlobError;
use super::model::{BlobHandle, BlobPlan, DurableToken, WrittenBlob};
use crate::blob::ByteStream;
use bytes::Bytes;

/// Every blob byte is written through this port. It never holds a database transaction across file I/O.
#[async_trait]
pub trait BlobWriter: Send + Sync {
    /// Pure: picks a new v4 blob id and wraps a fresh data key; no file and no row exist afterwards.
    fn plan(&self, account: Uuid, plaintext_size: i64) -> Result<BlobPlan, BlobError>;

    async fn create_file(&self, plan: BlobPlan, preallocate: bool)
    -> Result<BlobHandle, BlobError>;

    async fn open_for_write(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
    ) -> Result<BlobHandle, BlobError>;

    /// `plaintext` is a whole block whose hash the caller verified.
    async fn write_block(
        &self,
        handle: &BlobHandle,
        block_index: u32,
        plaintext: Bytes,
    ) -> Result<DurableToken, BlobError>;

    /// Returns after the group `fdatasync` that covers the write; on `Io` the caller must not acknowledge.
    async fn await_durable(&self, token: DurableToken) -> Result<(), BlobError>;

    /// Writes and syncs the whole object and creates no `blobs` row; the caller registers it afterwards.
    async fn write_complete(
        &self,
        account: Uuid,
        size: i64,
        body: ByteStream,
    ) -> Result<WrittenBlob, BlobError>;

    /// A missing file is `Ok`.
    async fn discard(&self, blob_id: Uuid) -> Result<(), BlobError>;
}
