//! What the scrubber remembers about the runs it started. `ScrubRun` of the port carries only `started_at`, so
//! the rate, the age threshold, the blobs set aside and the counts of the summary live here, keyed by the start.
//! Progress is never kept here: it is `last_scrubbed_at` in the database, so a lost run loses only its counts.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use sc_clock::{MonotonicInstant, Timestamp};
use uuid::Uuid;

use super::pace::Pacer;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Tally {
    pub checked: u64,
    pub mismatched: u64,
    pub missing: u64,
}

impl Tally {
    pub fn add(&mut self, other: Tally) {
        self.checked += other.checked;
        self.mismatched += other.mismatched;
        self.missing += other.missing;
    }
}

struct RunState {
    threshold_micros: i64,
    pacer: Pacer,
    /// Blobs a collector held at the moment of their check. They keep their `last_scrubbed_at`, so the next run
    /// takes them, and this run must not select them again.
    set_aside: Vec<Uuid>,
    tally: Tally,
    began: MonotonicInstant,
}

/// What one step needs from its run.
pub(super) struct StepPlan {
    pub threshold_micros: i64,
    pub pacer: Pacer,
    pub set_aside: Vec<Uuid>,
}

/// What one step hands back.
pub(super) struct StepRecord {
    pub pacer: Pacer,
    pub set_aside: Vec<Uuid>,
    pub tally: Tally,
}

#[derive(Default)]
pub(super) struct RunBook {
    runs: Mutex<HashMap<i64, RunState>>,
}

impl RunBook {
    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<i64, RunState>> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A second run with the same start replaces the first; both would select the same set anyway.
    pub fn start(
        &self,
        started: Timestamp,
        threshold_micros: i64,
        rate: u64,
        began: MonotonicInstant,
    ) {
        self.runs().insert(
            started.as_unix_micros(),
            RunState {
                threshold_micros,
                pacer: Pacer::new(rate),
                set_aside: Vec::new(),
                tally: Tally::default(),
                began,
            },
        );
    }

    pub fn plan(&self, started: Timestamp) -> Option<StepPlan> {
        self.runs()
            .get(&started.as_unix_micros())
            .map(|state| StepPlan {
                threshold_micros: state.threshold_micros,
                pacer: state.pacer.clone(),
                set_aside: state.set_aside.clone(),
            })
    }

    pub fn record(&self, started: Timestamp, record: StepRecord) {
        if let Some(state) = self.runs().get_mut(&started.as_unix_micros()) {
            state.pacer = record.pacer;
            state.set_aside = record.set_aside;
            state.tally.add(record.tally);
        }
    }

    /// Forgets the run and returns its counts and the instant it began.
    pub fn finish(&self, started: Timestamp) -> Option<(Tally, MonotonicInstant)> {
        self.runs()
            .remove(&started.as_unix_micros())
            .map(|state| (state.tally, state.began))
    }
}
