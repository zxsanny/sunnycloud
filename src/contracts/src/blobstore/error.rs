use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

/// The typed failures of the blob data plane. None of them carries a path or key material.
#[derive(Debug, Error)]
pub enum BlobError {
    #[error("no space left on the blob volume")]
    NoSpace,
    /// Free space would fall below `blob_free_reserve`.
    #[error("blob volume below its free-space floor")]
    FreeSpaceFloor,
    #[error("blob i/o failure")]
    Io(#[source] Fault),
    /// The file is absent. Callers map it to `content_unavailable`, never to a server error.
    #[error("blob file missing")]
    BlobMissing,
    /// The master key id is not held by the loaded ring, or a key cannot be unwrapped.
    #[error("master key unavailable")]
    KeyUnavailable,
    #[error("stored blob is corrupt")]
    Corrupt,
    #[error("range not satisfiable")]
    RangeNotSatisfiable,
    #[error("block index out of range")]
    OutOfRange,
    /// The stream is shorter or longer than the declared size.
    #[error("size mismatch")]
    SizeMismatch,
    /// The block index already holds other bytes, or a region that does not authenticate (a torn write).
    /// Nothing was sealed or written; the blob cannot be finished and the upload restarts with a new blob.
    #[error("block already written with other bytes")]
    BlockConflict,
}

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("master key file missing")]
    FileMissing,
    #[error("master key must be exactly 32 bytes")]
    WrongLength,
    #[error("master key does not match the key check")]
    KeyCheckMismatch,
    #[error("key check absent but blobs exist")]
    KeyCheckAbsentButBlobsExist,
    /// The candidate key cannot unwrap the sampled blob; nothing was written.
    #[error("key check sample failed")]
    KeyCheckSampleFailed { blob_id: Uuid },
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}
