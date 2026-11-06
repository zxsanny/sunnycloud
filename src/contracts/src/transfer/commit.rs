//! `CommitService`: turns finished uploads into revisions through `TreeCommit::publish`.

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_error::ErrorCode;
use sc_ids::Hash32;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;
use crate::tree::CommitActor;

/// `client_meta` is JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitUpload {
    pub upload_id: Uuid,
    pub parent_id: Uuid,
    pub name: String,
    pub base_revision: Option<Uuid>,
    pub blocklist: Vec<Hash32>,
    pub client_mtime: Option<Timestamp>,
    pub client_meta: Option<String>,
    pub import_hint: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Committed {
    pub node_id: Uuid,
    pub revision_id: Uuid,
    pub conflict_copy: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchEntry {
    pub staged_key: Uuid,
    pub parent_id: Uuid,
    pub name: String,
    pub base_revision: Option<Uuid>,
    pub client_mtime: Option<Timestamp>,
    pub client_meta: Option<String>,
    pub import_hint: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCommit {
    pub commit_id: Uuid,
    pub entries: Vec<BatchEntry>,
}

/// Why one entry of a batch was not committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryError {
    InvalidName,
    EditedSince,
    ParentGone,
    ParentNotFound,
    NotAFolder,
    BlocklistMismatch,
    /// The staged upload is unknown or expired.
    UploadNotFound,
    EffectFailed(ErrorCode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchEntryResult {
    Committed(Committed),
    Failed(EntryError),
}

/// One result per entry, in entry order. Its serialised form is the stored receipt response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchResult {
    pub per_entry: Vec<BatchEntryResult>,
}

#[derive(Debug, Error)]
pub enum CommitServiceError {
    /// The upload still lacks these blocks.
    #[error("blocks missing")]
    NeedBlocks(Vec<u32>),
    #[error("blocklist mismatch")]
    BlocklistMismatch,
    #[error("upload not found")]
    UploadNotFound,
    #[error("invalid name")]
    InvalidName,
    #[error("edited since")]
    EditedSince,
    #[error("parent gone")]
    ParentGone,
    #[error("parent not found")]
    ParentNotFound,
    #[error("not a folder")]
    NotAFolder,
    #[error("account not active")]
    AccountNotActive,
    #[error("batch too large")]
    BatchTooLarge,
    /// The same commit id arrived with a different request.
    #[error("commit id reused")]
    CommitIdReused,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait CommitService: Send + Sync {
    async fn commit_upload(
        &self,
        actor: CommitActor,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError>;

    /// A replayed commit id returns the stored result.
    async fn commit_batch(
        &self,
        actor: CommitActor,
        request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError>;
}
