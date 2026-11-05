use serde::{Deserialize, Serialize};

use crate::MAX_CLIENT_LANES;

/// `GET /api/v1/server-info`. Unknown fields are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub block_size: u64,
    pub max_lanes: u32,
    pub small_file_max: u64,
    pub batch_max_entries: u32,
    pub max_staged_per_account: u32,
    /// Kept as sent: the wire unit (seconds or milliseconds) is `server_info.md` open point 1.
    pub longpoll_max_wait: u64,
    pub min_client_version: String,
    pub api_versions: Vec<String>,
}

/// Lanes one upload session may use: the lowest of the client cap ([`MAX_CLIENT_LANES`]),
/// `ServerInfo.max_lanes` and `UploadSession.lane_limit`.
///
/// Decision for `sync_core_proto.md` open point 5: when the two server values differ, the lower bounds
/// the session.
pub fn effective_lanes(server_max_lanes: u32, session_lane_limit: u32) -> u32 {
    MAX_CLIENT_LANES
        .min(server_max_lanes)
        .min(session_lane_limit)
}
