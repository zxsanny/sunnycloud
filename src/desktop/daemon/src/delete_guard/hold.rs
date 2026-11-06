use std::collections::BTreeSet;

use ipc_schema::{HoldKind, HoldView, PairId};
use serde::{Deserialize, Serialize};

use super::GuardError;
use crate::local_index::{Pair, PairState};

/// The deletes a hold keeps back, per direction, as the paths the senders were asked to delete (a deleted
/// directory is one path). `local` are local deletes bound for the server, `remote` are server deletes bound
/// for the local files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldDeletes {
    pub local: Vec<String>,
    pub remote: Vec<String>,
}

/// The JSON kept in `pair.hold_detail` while the pair is in `hold_mass_delete`. The counts are of individual
/// deleted files (see `count`), not of `paths`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StoredHold {
    pub paths: HeldDeletes,
    pub local_deletes: u32,
    pub remote_deletes: u32,
}

impl StoredHold {
    pub(super) fn of(pair: &Pair) -> Result<Self, GuardError> {
        let text = pair
            .hold_detail
            .as_deref()
            .ok_or(GuardError::HoldDetailUnreadable)?;
        serde_json::from_str(text).map_err(|error| {
            tracing::error!(%error, pair_id = pair.id, "stored hold detail is unreadable");
            GuardError::HoldDetailUnreadable
        })
    }

    pub(super) fn encode(&self) -> Result<String, GuardError> {
        serde_json::to_string(self).map_err(|error| {
            tracing::error!(%error, "hold detail cannot be encoded");
            GuardError::HoldDetailUnreadable
        })
    }
}

/// What `DaemonStatus.holds[]` shows for the pair; `None` when it is not held.
pub fn hold_view(pair: &Pair) -> Result<Option<HoldView>, GuardError> {
    let (kind, local_deletes, remote_deletes) = match pair.state {
        PairState::Active | PairState::Paused => return Ok(None),
        PairState::RootMissing => (HoldKind::RootMissing, 0, 0),
        PairState::HoldMassDelete => {
            let hold = StoredHold::of(pair)?;
            (
                HoldKind::MassDelete,
                hold.local_deletes,
                hold.remote_deletes,
            )
        }
    };
    Ok(Some(HoldView {
        pair_id: PairId(pair.id),
        kind,
        local_deletes,
        remote_deletes,
    }))
}

pub(super) fn sorted(paths: &BTreeSet<String>) -> Vec<String> {
    paths.iter().cloned().collect()
}
