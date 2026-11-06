use sync_core_proto::FileHash;
use uuid::Uuid;

use crate::platform_fs::FileId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    Synced,
    Dirty,
    DeleteHeld,
    Conflict,
    SkippedPlaceholder,
    NameEscaped,
}

impl EntryState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Dirty => "dirty",
            Self::DeleteHeld => "delete_held",
            Self::Conflict => "conflict",
            Self::SkippedPlaceholder => "skipped_placeholder",
            Self::NameEscaped => "name_escaped",
        }
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "synced" => Self::Synced,
            "dirty" => Self::Dirty,
            "delete_held" => Self::DeleteHeld,
            "conflict" => Self::Conflict,
            "skipped_placeholder" => Self::SkippedPlaceholder,
            "name_escaped" => Self::NameEscaped,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairState {
    Active,
    Paused,
    HoldMassDelete,
    RootMissing,
}

impl PairState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::HoldMassDelete => "hold_mass_delete",
            Self::RootMissing => "root_missing",
        }
    }

    pub(super) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "hold_mass_delete" => Self::HoldMassDelete,
            "root_missing" => Self::RootMissing,
            _ => return None,
        })
    }
}

/// What `add_pair` needs; the pair starts `active` with no hold detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPair {
    pub server_node_id: Uuid,
    pub local_root: String,
    pub rules: String,
    pub root_sentinel: String,
    /// From the probe at `add_pair`; decides how paths of this pair are compared.
    pub case_insensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub id: i64,
    pub server_node_id: Uuid,
    pub local_root: String,
    pub rules: String,
    pub root_sentinel: String,
    pub case_insensitive: bool,
    pub state: PairState,
    pub hold_detail: Option<String>,
}

/// One local file or directory of a pair. `rel_path` is the path as it is on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub pair_id: i64,
    pub rel_path: String,
    pub node_id: Option<Uuid>,
    pub revision_id: Option<Uuid>,
    pub content_hash: Option<FileHash>,
    pub size: u64,
    pub mtime_ns: i64,
    pub file_id: FileId,
    pub state: EntryState,
    pub settle_until: Option<i64>,
    pub escaped_from: Option<String>,
}
