use std::fmt;

use uuid::Uuid;

/// A new blob before any file or row exists: a v4 id and a fresh data key wrapped under the current master key.
#[derive(Clone, PartialEq, Eq)]
pub struct BlobPlan {
    pub blob_id: Uuid,
    pub wrapped_key: Vec<u8>,
    pub master_key_id: i16,
}

impl fmt::Debug for BlobPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlobPlan")
            .field("blob_id", &self.blob_id)
            .field("wrapped_key", &"<redacted>")
            .field("master_key_id", &self.master_key_id)
            .finish()
    }
}

/// An open blob file for block writes; `open_for_write` rebuilds it after a restart from the same fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobHandle {
    pub plan: BlobPlan,
    pub plaintext_size: i64,
}

/// Returned by `write_block`. It does not mean the block is durable; `await_durable` does.
/// The value is the provider's group-commit sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableToken(pub u64);

/// A synced blob file that has no `blobs` row yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenBlob {
    pub plan: BlobPlan,
    pub size: i64,
}
