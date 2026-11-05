use serde::{Deserialize, Serialize};

use crate::ids::{NodeId, PageToken, PairId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub node_id: NodeId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderPage {
    pub items: Vec<FolderEntry>,
    pub next: Option<PageToken>,
}

/// Exactly one target per pair; a request carrying both forms does not decode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairTarget {
    NodeId {
        node_id: NodeId,
    },
    Path {
        server_path: String,
        create_if_missing: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairSpec {
    pub target: PairTarget,
    pub local_root: String,
    pub rules_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairPatch {
    pub rules_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairState {
    Active,
    Paused,
    HoldMassDelete,
    RootMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairView {
    pub pair_id: PairId,
    pub local_root: String,
    pub server_node_id: NodeId,
    pub state: PairState,
    pub rules_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldKind {
    MassDelete,
    RootMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldDecision {
    ConfirmDeletes,
    RestoreFromServer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldView {
    pub pair_id: PairId,
    pub kind: HoldKind,
    pub local_deletes: u32,
    pub remote_deletes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    Placeholder,
    Symlink,
    Junction,
    SpecialFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedEntry {
    pub pair_id: PairId,
    pub rel_path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedPage {
    pub items: Vec<SkippedEntry>,
    pub next: Option<PageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapedEntry {
    pub pair_id: PairId,
    pub original_name: String,
    pub saved_name: String,
    pub rel_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscapedPage {
    pub items: Vec<EscapedEntry>,
    pub next: Option<PageToken>,
}

/// `bandwidth_limit` 0 means unlimited; its unit is an open point of the contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub bandwidth_limit: u64,
    pub hash_throttle: f64,
    pub autostart: bool,
    pub tray: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsPatch {
    pub bandwidth_limit: Option<u64>,
    pub hash_throttle: Option<f64>,
    pub autostart: Option<bool>,
    pub tray: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub account_id: String,
}

/// Present when an update is staged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateView {
    pub version: String,
}

/// Readers ignore unknown fields, so a newer daemon may add optional ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub server_url: Option<String>,
    pub account: Option<AccountView>,
    pub pairs: Vec<PairView>,
    pub skipped_count: u64,
    pub escaped_count: u64,
    pub holds: Vec<HoldView>,
    pub update_available: Option<UpdateView>,
    pub access_denied: Vec<PairId>,
    pub watcher_degraded: Vec<PairId>,
}

/// Always the complete new status; clients never merge partial updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusEvent {
    pub status: DaemonStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginPrompt {
    pub verification_url: String,
    pub user_code: String,
    /// RFC 3339, UTC.
    pub expires_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoginState {
    Pending,
    Approved,
    Denied,
    Expired,
    SignedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogoutOutcome {
    /// `false`: the server copy of the device token still needs revoking.
    pub server_revoked: bool,
}
