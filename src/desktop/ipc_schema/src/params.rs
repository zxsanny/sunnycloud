use serde::{Deserialize, Serialize};

use crate::ids::{NodeId, PageToken, PairId};
use crate::types::{HoldDecision, PairPatch};

/// Params of the methods that take none: an empty object on the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetServerParams {
    pub url: String,
}

/// No `parent` lists the account root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListServerFoldersParams {
    pub parent: Option<NodeId>,
    pub page: Option<PageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePairParams {
    pub pair_id: PairId,
    pub patch: PairPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairIdParams {
    pub pair_id: PairId,
}

/// No `pair_id` lists the entries of every pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntriesParams {
    pub pair_id: Option<PairId>,
    pub page: Option<PageToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveHoldParams {
    pub pair_id: PairId,
    pub decision: HoldDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginStartParams {
    pub device_name: String,
}
