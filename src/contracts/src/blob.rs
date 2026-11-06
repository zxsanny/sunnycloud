//! Blob data-plane value types. 04 owns later changes to them (module-layout Rule 6); they are declared here
//! so that the 03 ports compile before the 04 ports.

use std::fmt;
use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

/// Reference to one stored blob with the key material to read it. The media type is not part of it (04 §3);
/// callers pass it separately.
#[derive(Clone, PartialEq, Eq)]
pub struct BlobRef {
    pub blob_id: Uuid,
    pub size: i64,
    pub wrapped_key: Vec<u8>,
    pub master_key_id: i16,
}

impl fmt::Debug for BlobRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlobRef")
            .field("blob_id", &self.blob_id)
            .field("size", &self.size)
            .field("wrapped_key", &"<redacted>")
            .field("master_key_id", &self.master_key_id)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum StreamError {
    /// The authentication tag of a stored block failed; the stream ends and never yields a short body.
    #[error("stored content is corrupt")]
    Corrupt,
    #[error("stream source failed")]
    Source(#[source] Fault),
}

/// A request body as a neutral stream of chunks.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, StreamError>> + Send>>;

/// Decrypted content produced by 04's reader.
pub type PlaintextStream = Pin<Box<dyn Stream<Item = Result<Bytes, StreamError>> + Send>>;

/// One inclusive byte range `start..=end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}
