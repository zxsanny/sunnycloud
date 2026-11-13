use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::digest_serde;
use sc_ids::{FileHash, Hash32};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum NodeKind {
    File,
    Folder,
}

/// Opaque continuation text of a listing page; the caller never builds one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageToken(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NodeView {
    pub node_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub name: String,
    pub current_revision: Option<Uuid>,
}

/// A node of a listing page; `content_hash` is present for files and absent for folders.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ListingItem {
    pub node_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub name: String,
    pub current_revision: Option<Uuid>,
    #[serde(with = "digest_serde::option", default)]
    pub content_hash: Option<FileHash>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ListingPage {
    pub items: Vec<ListingItem>,
    pub next: Option<PageToken>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RevisionView {
    pub revision_id: Uuid,
    #[serde(with = "digest_serde::list")]
    pub blocklist: Vec<Hash32>,
    #[serde(with = "digest_serde")]
    pub content_hash: FileHash,
    pub size: u64,
    pub client_meta: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RevisionRef {
    pub revision_id: Uuid,
    pub node_id: Uuid,
    #[serde(with = "digest_serde")]
    pub content_hash: FileHash,
    pub size: u64,
    pub client_meta: Option<Value>,
}
