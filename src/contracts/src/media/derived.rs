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

/// Never falls back to the original bytes. 11 streams the returned `BlobRef` through 03.
#[async_trait]
pub trait DerivedOpener: Send + Sync {
    async fn open_derived(
        &self,
        account: Uuid,
        node_id: Uuid,
        kind: DerivedKind,
    ) -> Result<BlobRef, DerivedError>;

    /// As `open_derived` without the node lookup; used after 06 authorised a share token.
    async fn open_derived_for(
        &self,
        file: &LiveFile,
        kind: DerivedKind,
    ) -> Result<BlobRef, DerivedError>;
}
