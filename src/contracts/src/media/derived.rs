//! `ThumbnailStateSource` and `DerivedOpener`.

use std::collections::HashMap;

use async_trait::async_trait;
use sc_ids::FileHash;
use thiserror::Error;
use uuid::Uuid;

use super::model::DerivedKind;
use crate::blob::BlobRef;
use crate::common::Fault;
use crate::tree::LiveFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbState {
    Ready,
    Pending,
    Failed,
    /// No job is planned for the type, or the hash is unknown to the account.
    None,
}

#[derive(Debug, Error)]
pub enum ThumbnailStateError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait ThumbnailStateSource: Send + Sync {
    /// One state per asked hash.
    async fn states(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError>;
}

#[derive(Debug, Error)]
pub enum DerivedError {
    /// The file has no derived row yet but one is expected.
    #[error("derived file not ready")]
    NotReady,
    /// No derived file will exist: unsupported type, vault file, or another account's node.
    #[error("not found")]
    NotFound,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// A derived blob with the media type stored with it; a `BlobRef` carries none. 11 passes both to 03's
/// `DownloadService::open_blob_content`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedBlob {
    pub blob: BlobRef,
    pub media_type: String,
}

/// Never falls back to the original bytes. 11 streams the returned blob through 03.
#[async_trait]
pub trait DerivedOpener: Send + Sync {
    async fn open_derived(
        &self,
        account: Uuid,
        node_id: Uuid,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError>;

    /// As `open_derived` without the node lookup; used after 06 authorised a share token. `account` owns `file`
    /// (`ShareDerived::account_id`).
    async fn open_derived_for(
        &self,
        account: Uuid,
        file: &LiveFile,
        kind: DerivedKind,
    ) -> Result<DerivedBlob, DerivedError>;
}
