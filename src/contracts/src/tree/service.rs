//! `TreeService` (mutations and path queries) and `TreeReader` (reads). Each method opens its own short
//! transaction in the provider.

use std::collections::HashMap;

use async_trait::async_trait;
use sc_ids::FileHash;
use thiserror::Error;
use uuid::Uuid;

use super::model::{
    ListingPage, LiveFileLookup, NodeRef, NodeSelector, NodeView, RevisionMatch, RevisionRef,
    RevisionView,
};
use crate::common::{Fault, Page};

#[derive(Debug, Error)]
pub enum TreeServiceError {
    #[error("invalid name: {0}")]
    NameInvalid(String),
    #[error("name conflict")]
    NameConflict,
    #[error("parent deleted")]
    ParentDeleted,
    #[error("not found")]
    NotFound,
    #[error("not a folder")]
    NotAFolder,
    #[error("cycle")]
    Cycle,
    #[error("root node")]
    IsRoot,
    #[error("edited since")]
    EditedSince { current_revision: Uuid },
    #[error("invalid cursor")]
    InvalidCursor,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait TreeService: Send + Sync {
    async fn create_folder(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<NodeView, TreeServiceError>;

    /// Rename, move, or both; neither carries a `base_revision`.
    async fn update_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        new_name: Option<&str>,
        new_parent: Option<Uuid>,
    ) -> Result<NodeView, TreeServiceError>;

    /// Idempotent; deleting a tombstoned node succeeds.
    async fn delete_node(
        &self,
        account: Uuid,
        node_id: Uuid,
        base_revision: Option<Uuid>,
    ) -> Result<(), TreeServiceError>;

    async fn resolve_path(&self, account: Uuid, path: &str) -> Result<Uuid, TreeServiceError>;

    /// `client_device` filters on the device recorded in `client_meta`.
    async fn find_revisions(
        &self,
        account: Uuid,
        client_device: Option<&str>,
        page: Page,
    ) -> Result<Vec<RevisionRef>, TreeServiceError>;
}

#[derive(Debug, Error)]
pub enum TreeReaderError {
    #[error("not found")]
    NotFound,
    #[error("not a folder")]
    NotAFolder,
    #[error("invalid cursor")]
    InvalidCursor,
    /// More than 1000 hashes in `lookup_revisions`.
    #[error("batch too large")]
    BatchTooLarge,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// A node addressed by id or by path, for the operator restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOrPath {
    Node(Uuid),
    Path(String),
}

#[async_trait]
pub trait TreeReader: Send + Sync {
    /// A node under a tombstoned ancestor is reported deleted at once.
    async fn get_node(
        &self,
        account: Uuid,
        selector: NodeSelector,
    ) -> Result<NodeView, TreeReaderError>;

    async fn list_children(
        &self,
        account: Uuid,
        folder_id: Uuid,
        after_name_key: Option<&str>,
        limit: u32,
    ) -> Result<ListingPage, TreeReaderError>;

    /// By `name_key`, live nodes only.
    async fn find_live_node(
        &self,
        account: Uuid,
        parent_id: Uuid,
        name: &str,
    ) -> Result<Option<NodeRef>, TreeReaderError>;

    /// A folder is `Found` with `kind = Folder`; a gone node is `Gone`.
    async fn resolve_live_file(
        &self,
        account: Uuid,
        node_id: Uuid,
    ) -> Result<LiveFileLookup, TreeReaderError>;

    async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError>;

    /// Includes the blocklist.
    async fn get_revision(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<RevisionView, TreeReaderError>;

    async fn is_current_or_retained(
        &self,
        account: Uuid,
        revision_id: Uuid,
    ) -> Result<bool, TreeReaderError>;

    /// System scope (no account): the node paths that use each blob.
    async fn describe_blobs(
        &self,
        blob_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError>;

    /// At most 1000 hashes; current revisions of live files only.
    async fn lookup_revisions(
        &self,
        account: Uuid,
        hashes: &[FileHash],
    ) -> Result<Vec<RevisionMatch>, TreeReaderError>;
}
