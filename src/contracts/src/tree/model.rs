//! Node and revision views. The field lists are derived from data_model §5 and 05's `ChangeItem`; 02 lists none.

use sc_clock::Timestamp;
use sc_ids::{FileHash, Hash32};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Live,
    Tombstone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeRef {
    pub id: Uuid,
    pub kind: NodeKind,
}

/// `parent_id` is `None` for the root. The file-only fields are `None` for a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub name: String,
    pub state: NodeState,
    pub current_revision_id: Option<Uuid>,
    pub size: Option<i64>,
    pub content_hash: Option<FileHash>,
    pub media_type: Option<String>,
    pub client_mtime: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub seq: i64,
}

/// `blocklist` is filled only by `TreeReader::get_revision`. `client_meta` is JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionView {
    pub id: Uuid,
    pub node_id: Uuid,
    pub size: i64,
    pub content_hash: FileHash,
    pub blocklist: Option<Vec<Hash32>>,
    pub media_type: Option<String>,
    pub client_mtime: Option<Timestamp>,
    pub client_meta: Option<String>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionRef {
    pub revision_id: Uuid,
    pub node_id: Uuid,
}

/// A current revision of a live file whose content hash was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionMatch {
    pub hash: FileHash,
    pub node_id: Uuid,
    pub revision_id: Uuid,
    pub client_meta: Option<String>,
}

/// `next` is the `name_key` to pass as `after_name_key`; `None` on the last page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingPage {
    pub items: Vec<NodeView>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeSelector {
    Id(Uuid),
    Root,
}

/// A live file or folder. For a folder `revision` and `blob_ref` are `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFile {
    pub kind: NodeKind,
    pub revision: Option<RevisionView>,
    /// The id of the current revision's blob (not a `BlobRef`); 03 resolves its key material through 04.
    pub blob_ref: Option<Uuid>,
}

/// Tombstoned, purged, of another account, or under a tombstoned ancestor: `Gone`, never an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveFileLookup {
    Found(LiveFile),
    Gone,
}
