//! The test-only fault hook around the group sync. It wraps the real `fdatasync`: it can hold a flush, delay
//! it, fail it before any file is synced, and records every flush and every synced file. It exists only with the
//! `sync-hook` feature, which the crate enables for its own tests and nothing else.

use std::fs::File;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use uuid::Uuid;

use super::flush::sync_files;
use super::state::{Outcome, SyncFailure};

/// One file synced by one group flush. The stamp is taken just before the sync starts, on the clock shared with
/// [`SyncHook::tick`]: a write stamped lower than the record was complete when its sync began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncRecord {
    pub blob_id: Uuid,
    pub flush: usize,
    pub stamp: u64,
}

#[derive(Default)]
struct HookState {
    held: bool,
    delay: Duration,
    failures: usize,
    started: usize,
    records: Vec<SyncRecord>,
}

#[derive(Default)]
pub struct SyncHook {
    state: Mutex<HookState>,
    released: Condvar,
    clock: AtomicU64,
}

impl SyncHook {
    pub fn new() -> Arc<Self> {
        Arc::new(SyncHook::default())
    }

    fn lock(&self) -> MutexGuard<'_, HookState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A strictly increasing stamp. A test takes one after a write completes and compares it with the stamps of
    /// the sync records.
    pub fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Every flush from now on waits this long before it syncs.
    pub fn delay_flushes(&self, delay: Duration) {
        self.lock().delay = delay;
    }

    /// Every flush from now on blocks until [`SyncHook::release`].
    pub fn hold(&self) {
        self.lock().held = true;
    }

    pub fn release(&self) {
        self.lock().held = false;
        self.released.notify_all();
    }

    /// The next `count` group flushes fail with an I/O error before any file is synced.
    pub fn fail_next_flushes(&self, count: usize) {
        self.lock().failures = count;
    }

    /// Group flushes that reached the sync, failed ones included.
    pub fn flush_count(&self) -> usize {
        self.lock().started
    }

    pub fn records(&self) -> Vec<SyncRecord> {
        self.lock().records.clone()
    }

    pub(super) fn run(&self, files: &[(Uuid, Arc<File>)]) -> Outcome {
        let flush = {
            let mut state = self.lock();
            state.started += 1;
            let flush = state.started;
            while state.held {
                state = self
                    .released
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            let delay = state.delay;
            if state.failures > 0 {
                state.failures -= 1;
                return Err(SyncFailure {
                    kind: ErrorKind::Other,
                    blob_id: files.first().map(|(id, _)| *id),
                });
            }
            drop(state);
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            flush
        };
        sync_files(files, |blob_id| {
            let stamp = self.tick();
            self.lock().records.push(SyncRecord {
                blob_id,
                flush,
                stamp,
            });
        })
    }
}
