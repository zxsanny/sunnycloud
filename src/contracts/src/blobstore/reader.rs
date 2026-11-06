use async_trait::async_trait;
use bytes::Bytes;
use uuid::Uuid;

use super::error::BlobError;
use crate::blob::{ByteRange, PlaintextStream};

/// The one decrypt path: user downloads and the worker input fetch both use it. The caller passes the key
/// material from `BlobCatalog::get_ref`; the reader reads no `blobs` row and keeps no key cache.
#[async_trait]
pub trait BlobReader: Send + Sync {
    /// The first chunk of the range is read and authenticated before this returns. A later chunk whose tag
    /// fails ends the stream with `StreamError::Corrupt`.
    async fn open(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
        range: Option<ByteRange>,
    ) -> Result<PlaintextStream, BlobError>;

    /// The first `max_bytes` (at most 64 KiB) of the plaintext, for type sniffing at commit.
    async fn read_head(
        &self,
        blob_id: Uuid,
        wrapped_key: &[u8],
        master_key_id: i16,
        plaintext_size: i64,
        max_bytes: u32,
    ) -> Result<Bytes, BlobError>;
}
