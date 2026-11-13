use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cursor::Cursor;
use crate::digest_serde;
use crate::tree::NodeKind;
use sc_ids::FileHash;

/// Whether a node is present or deleted in a feed item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ChangeState {
    Live,
    Tombstone,
}

/// The latest state of one node at `seq`; not an event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChangeItem {
    pub node_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub name: String,
    pub state: ChangeState,
    pub revision_id: Option<Uuid>,
    pub size: Option<u64>,
    #[serde(with = "digest_serde::option", default)]
    pub content_hash: Option<FileHash>,
    pub seq: u64,
}

/// One answer of `GET /api/v1/changes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChangePage {
    pub items: Vec<ChangeItem>,
    pub next_cursor: Cursor,
    pub has_more: bool,
}

/// Why the feed asks for a resync: the first three are the 410 `reason`, `InvalidCursor` is the 422
/// `invalid_cursor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ResyncReason {
    EpochChanged,
    BelowPurgeHorizon,
    CursorAhead,
    InvalidCursor,
}

impl ResyncReason {
    /// Maps the 410 `reason` text through `sc_error`; `None` for a reason this build does not know.
    pub fn from_wire(wire: &str) -> Option<Self> {
        sc_error::ResyncReason::from_wire(wire).map(Self::from)
    }
}

impl From<sc_error::ResyncReason> for ResyncReason {
    fn from(reason: sc_error::ResyncReason) -> Self {
        match reason {
            sc_error::ResyncReason::EpochChanged => Self::EpochChanged,
            sc_error::ResyncReason::BelowPurgeHorizon => Self::BelowPurgeHorizon,
            sc_error::ResyncReason::CursorAhead => Self::CursorAhead,
        }
    }
}
