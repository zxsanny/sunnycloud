//! The mass-delete guard and the root sentinel of 14 §5 step 6. Local deletes bound for the server and server
//! deletes bound for the local files both pass [`DeleteGuard::evaluate`]; a pair whose root fails
//! [`DeleteGuard::check_root`] is held as well. Nothing is sent or applied while a pair is held, and the hold
//! is kept in the index (`pair.state` and `pair.hold_detail`) so a restart cannot lose it. The guard does I/O
//! only through [`PlatformFs`] and the [`LocalIndexStore`]; sending, trashing and re-downloading are the
//! callers' job, with the sets [`DeleteGuard::resolve`] returns.
//!
//! Any `Err` from the guard means no delete was released.

mod count;
mod error;
mod hold;
mod root;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ipc_schema::{HoldDecision, HoldKind, HoldView, PairId};

pub use error::GuardError;
pub use hold::{HeldDeletes, hold_view};
pub use root::{RootFault, SENTINEL_NAME, create_sentinel};

use crate::local_index::{LocalIndexStore, Pair, PairState};
use crate::platform_fs::PlatformFs;
use hold::{StoredHold, sorted};

/// The deletes pending for a pair at decision time, one list per direction. A deleted directory is listed as
/// itself; its descendants are counted from the index.
pub struct DeleteBatch<'a> {
    pub local: &'a [String],
    pub remote: &'a [String],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Send and apply every delete of the batch.
    Release,
    /// Send and apply none of them; the batch is part of the hold now.
    Hold(HoldView),
}

#[derive(Debug, PartialEq, Eq)]
pub enum RootCheck {
    /// The root is the one the pair was made for, so a scan of it can be believed.
    Healthy,
    /// Do not scan: an absent file in this root proves nothing.
    Faulted(RootFault),
}

/// What the user decided for the held deletes.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    /// `ConfirmDeletes`: `local` go to the pair worker, `remote` to the change applier.
    Release(HeldDeletes),
    /// `RestoreFromServer`: all of them go to the change applier for re-download; none is released.
    Restore(HeldDeletes),
}

pub struct DeleteGuard {
    index: Arc<LocalIndexStore>,
    threshold: NonZeroU32,
    /// Serialises the read-modify-write of a hold against the other guard calls.
    transitions: Mutex<()>,
}

impl DeleteGuard {
    /// `threshold` is `desktop.mass_delete_threshold`: that many deletes in one direction hold the pair.
    pub fn new(index: Arc<LocalIndexStore>, threshold: NonZeroU32) -> Self {
        Self {
            index,
            threshold,
            transitions: Mutex::new(()),
        }
    }

    /// Decides one batch. A pair already held keeps everything: the batch joins the held deletes.
    pub fn evaluate(&self, pair_id: i64, batch: &DeleteBatch<'_>) -> Result<Verdict, GuardError> {
        let _transition = self.transition();
        let pair = self.pair(pair_id)?;
        let held = match pair.state {
            PairState::Paused => return Err(GuardError::Paused),
            PairState::RootMissing => return self.view(&pair).map(Verdict::Hold),
            PairState::HoldMassDelete => StoredHold::of(&pair)?.paths,
            PairState::Active => HeldDeletes::default(),
        };
        let local = merged(&held.local, batch.local);
        let remote = merged(&held.remote, batch.remote);
        let local_deletes = count::deleted_files(&self.index, pair_id, &local)?;
        let remote_deletes = count::deleted_files(&self.index, pair_id, &remote)?;
        let threshold = self.threshold.get() as usize;
        let already_held = pair.state == PairState::HoldMassDelete;
        if !already_held && local_deletes < threshold && remote_deletes < threshold {
            return Ok(Verdict::Release);
        }
        let stored = StoredHold {
            paths: HeldDeletes {
                local: sorted(&local),
                remote: sorted(&remote),
            },
            local_deletes: saturating(local_deletes),
            remote_deletes: saturating(remote_deletes),
        };
        self.set_state(pair_id, PairState::HoldMassDelete, Some(&stored.encode()?))?;
        if !already_held {
            tracing::warn!(
                pair_id,
                local_deletes = stored.local_deletes,
                remote_deletes = stored.remote_deletes,
                "mass delete hold entered"
            );
        }
        Ok(Verdict::Hold(HoldView {
            pair_id: PairId(pair_id),
            kind: HoldKind::MassDelete,
            local_deletes: stored.local_deletes,
            remote_deletes: stored.remote_deletes,
        }))
    }

    /// Run at start and before every rescan. A root that fails the check puts an active pair into
    /// `root_missing`; a root that passes again returns a `root_missing` pair to `active`. A pair in a mass
    /// delete hold stays there, since it already keeps every delete back.
    pub fn check_root<F: PlatformFs>(&self, fs: &F, pair_id: i64) -> Result<RootCheck, GuardError> {
        let fault = root::probe(fs, &self.index, &self.pair(pair_id)?)?;
        let _transition = self.transition();
        let pair = self.pair(pair_id)?;
        match (fault, pair.state) {
            (Some(fault), PairState::Active) => {
                self.set_state(pair_id, PairState::RootMissing, None)?;
                tracing::warn!(pair_id, ?fault, "root missing hold entered");
                Ok(RootCheck::Faulted(fault))
            }
            (Some(fault), _) => Ok(RootCheck::Faulted(fault)),
            (None, PairState::RootMissing) => {
                self.set_state(pair_id, PairState::Active, None)?;
                Ok(RootCheck::Healthy)
            }
            (None, _) => Ok(RootCheck::Healthy),
        }
    }

    /// Ends a mass delete hold with the user's decision and returns the pair to `active`. The hold is gone
    /// once this returns: if the process stops before the caller has passed the paths on, the next rescan
    /// finds the deletes again and holds them again.
    pub fn resolve(&self, pair_id: i64, decision: HoldDecision) -> Result<Resolution, GuardError> {
        let _transition = self.transition();
        let pair = self.pair(pair_id)?;
        match pair.state {
            PairState::Active | PairState::Paused => return Err(GuardError::NoSuchHold),
            PairState::RootMissing => return Err(GuardError::RootHold),
            PairState::HoldMassDelete => {}
        }
        let held = StoredHold::of(&pair)?.paths;
        self.set_state(pair_id, PairState::Active, None)?;
        Ok(match decision {
            HoldDecision::ConfirmDeletes => Resolution::Release(held),
            HoldDecision::RestoreFromServer => Resolution::Restore(held),
        })
    }

    fn pair(&self, pair_id: i64) -> Result<Pair, GuardError> {
        self.index.get_pair(pair_id)?.ok_or(GuardError::UnknownPair)
    }

    fn set_state(
        &self,
        pair_id: i64,
        state: PairState,
        detail: Option<&str>,
    ) -> Result<(), GuardError> {
        if self.index.set_pair_state(pair_id, state, detail)? {
            Ok(())
        } else {
            Err(GuardError::UnknownPair)
        }
    }

    fn view(&self, pair: &Pair) -> Result<HoldView, GuardError> {
        hold_view(pair)?.ok_or(GuardError::NoSuchHold)
    }

    fn transition(&self) -> MutexGuard<'_, ()> {
        self.transitions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// New files keep uploading during a mass delete hold; a missing root has nothing to read and a paused pair
/// sends nothing.
pub fn uploads_allowed(state: PairState) -> bool {
    matches!(state, PairState::Active | PairState::HoldMassDelete)
}

fn merged(held: &[String], batch: &[String]) -> BTreeSet<String> {
    held.iter().chain(batch).cloned().collect()
}

fn saturating(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
