use std::collections::HashMap;
use std::fs::File;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::Notify;
use uuid::Uuid;

use super::flush;
use crate::error::BlobIds;
use crate::metrics::BlobMetrics;

#[cfg(feature = "sync-hook")]
use super::hook::SyncHook;

/// Why a group's sync failed: the kind of the first failing call and the blob it was for. No OS text is kept.
#[derive(Debug, Clone, Copy)]
pub(super) struct SyncFailure {
    pub kind: ErrorKind,
    pub blob_id: Option<Uuid>,
}

impl SyncFailure {
    pub fn ids(&self) -> BlobIds {
        BlobIds {
            blob_id: self.blob_id,
            account_id: None,
        }
    }
}

pub(super) type Outcome = Result<(), SyncFailure>;

#[derive(Default)]
struct Group {
    files: Vec<(Uuid, Arc<File>)>,
    registrations: usize,
    outcome: Option<Outcome>,
}

#[derive(Default)]
struct State {
    /// The group new registrations join; a flush closes it by moving to the next number.
    open: u64,
    flushing: bool,
    groups: HashMap<u64, Group>,
}

pub(super) struct Shared {
    state: Mutex<State>,
    pub wake: Notify,
    pub delay: Duration,
    #[cfg(feature = "sync-hook")]
    pub hook: Option<Arc<SyncHook>>,
}

impl Shared {
    pub fn new(delay: Duration) -> Self {
        Shared {
            state: Mutex::new(State::default()),
            wake: Notify::new(),
            delay,
            #[cfg(feature = "sync-hook")]
            hook: None,
        }
    }

    #[cfg(feature = "sync-hook")]
    pub fn with_hook(delay: Duration, hook: Arc<SyncHook>) -> Self {
        Shared {
            hook: Some(hook),
            ..Shared::new(delay)
        }
    }

    /// Every critical section leaves the state consistent between statements, so a poisoned lock is still valid.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn register(&self, blob_id: Uuid, file: Arc<File>) -> u64 {
        let mut state = self.lock();
        let open = state.open;
        let group = state.groups.entry(open).or_default();
        group.registrations += 1;
        if !group.files.iter().any(|(id, _)| *id == blob_id) {
            group.files.push((blob_id, file));
        }
        open
    }

    /// The outcome of the token's group when it is known; otherwise, when the group is open and no flush runs,
    /// this waiter starts the flush. An absent group is a token that was never issued or was already awaited.
    pub fn check(
        self: &Arc<Self>,
        group: u64,
        metrics: Option<&Arc<BlobMetrics>>,
    ) -> Option<Outcome> {
        let mut state = self.lock();
        let Some(entry) = state.groups.get(&group) else {
            return Some(Err(SyncFailure {
                kind: ErrorKind::InvalidInput,
                blob_id: None,
            }));
        };
        if let Some(outcome) = entry.outcome {
            return Some(outcome);
        }
        if group == state.open && !state.flushing {
            state.flushing = true;
            tokio::spawn(flush::run(Arc::clone(self), metrics.cloned()));
        }
        None
    }

    /// Closes the open group and hands its files to the flush.
    pub fn close_open_group(&self) -> (u64, Vec<(Uuid, Arc<File>)>) {
        let mut state = self.lock();
        let id = state.open;
        state.open += 1;
        let files = state
            .groups
            .get_mut(&id)
            .map(|group| std::mem::take(&mut group.files))
            .unwrap_or_default();
        (id, files)
    }

    pub fn publish(&self, group: u64, outcome: Outcome) {
        let mut state = self.lock();
        state.flushing = false;
        if let Some(entry) = state.groups.get_mut(&group) {
            entry.outcome = Some(outcome);
            if entry.registrations == 0 {
                state.groups.remove(&group);
            }
        }
        drop(state);
        self.wake.notify_waiters();
    }

    /// One registration of the group has been awaited or abandoned; a finished group nobody waits on is dropped.
    pub fn release(&self, group: u64) {
        let mut state = self.lock();
        let Some(entry) = state.groups.get_mut(&group) else {
            return;
        };
        entry.registrations = entry.registrations.saturating_sub(1);
        if entry.registrations == 0 && entry.outcome.is_some() {
            state.groups.remove(&group);
        }
    }
}
