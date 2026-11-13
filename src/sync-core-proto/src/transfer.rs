use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::digest_serde;
use sc_ids::FileHash;

/// Answer of `start_upload` and `get_upload`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct UploadSession {
    pub upload_id: Uuid,
    pub need_blocks: Vec<u32>,
    pub lane_limit: u32,
    /// Timestamp text as sent by the server; the crate has no clock and never reads it.
    pub expires_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum StartOutcome {
    Session(UploadSession),
    NothingToSend { revision_id: Uuid },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum CommitOutcome {
    Published { node_id: Uuid, revision_id: Uuid },
    ConflictCopy { node_id: Uuid, copy_node_id: Uuid },
    NothingToSend { revision_id: Uuid },
}

/// Answer of `stage_small`: the staged row the server holds for the request. `size` is the byte count it stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Staged {
    pub staged_key: Uuid,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BatchEntry {
    pub staged_key: Uuid,
    pub parent: Uuid,
    pub name: String,
    pub base_revision: Option<Uuid>,
    #[serde(with = "digest_serde")]
    pub file_hash: FileHash,
    pub client_meta: Option<Value>,
    pub import_hint: Option<Value>,
}

/// `commit_id` is a UUIDv7 that equals no member key (see [`crate::check_batch_ids`]); `entries` holds at
/// most `ServerInfo.batch_max_entries`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BatchCommit {
    pub commit_id: Uuid,
    pub entries: Vec<BatchEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum EntryResult {
    Committed {
        node_id: Uuid,
        revision_id: Uuid,
        conflict_copy: Option<Uuid>,
    },
    EntryError {
        code: String,
    },
}

/// One result per submitted entry, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BatchResult {
    pub entries: Vec<EntryResult>,
}
