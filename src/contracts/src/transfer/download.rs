//! `DownloadService` and `ReservationSource`.

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::blob::{BlobRef, ByteRange, PlaintextStream};
use crate::common::Fault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionOrCurrent {
    Revision(Uuid),
    /// The current revision of this node.
    Current(Uuid),
}

/// The request options of both open methods. `retained_ok` is set by 11 for an admin principal only; without
/// it a non-current revision is not served. `if_range` and `if_none_match` carry entity tags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadOptions {
    pub retained_ok: bool,
    pub range: Option<ByteRange>,
    pub if_range: Option<String>,
    pub if_none_match: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStatus {
    Full,
    Partial,
    NotModified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Inline,
    Attachment,
}

pub struct Download {
    pub status: DownloadStatus,
    pub total_size: i64,
    /// The served range when `status` is `Partial`.
    pub range: Option<ByteRange>,
    pub etag: String,
    pub media_type: Option<String>,
    pub disposition: Disposition,
    pub suggested_name: String,
    pub body: PlaintextStream,
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("not found")]
    NotFound,
    #[error("range not satisfiable")]
    RangeNotSatisfiable,
    /// The blob is missing or suspect.
    #[error("content unavailable")]
    ContentUnavailable,
    #[error("storage failure")]
    Storage(#[source] Fault),
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait DownloadService: Send + Sync {
    async fn open_revision_content(
        &self,
        account: Uuid,
        target: RevisionOrCurrent,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError>;

    /// `media_type` is separate because a `BlobRef` does not carry it.
    async fn open_blob_content(
        &self,
        account: Uuid,
        blob: BlobRef,
        media_type: Option<String>,
        options: DownloadOptions,
    ) -> Result<Download, DownloadError>;
}

#[derive(Debug, Error)]
pub enum ReservationError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait ReservationSource: Send + Sync {
    /// Bytes reserved by the account's open uploads, for the caller of `QuotaAdmin::recompute`.
    async fn open_reserved_bytes(&self, account: Uuid) -> Result<i64, ReservationError>;
}
