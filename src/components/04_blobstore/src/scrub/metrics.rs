//! The scrub series of `deployment/observability.md` § Scrub. A run raises the counters step by step, so an alert
//! on a mismatch does not wait for the end of a long run; `finish_run` sets the gauges.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sc_clock::Timestamp;
use sc_observability::{Counter, Gauge, Metrics, NoLabels, ObsError};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

pub struct ScrubMetrics {
    running: Gauge<NoLabels>,
    verified: Counter<NoLabels>,
    mismatches: Counter<NoLabels>,
    missing: Counter<NoLabels>,
    last_completed: Gauge<NoLabels>,
    last_duration: Gauge<NoLabels>,
    throttle_sleep: Counter<NoLabels>,
    slept_nanos: AtomicU64,
}

impl ScrubMetrics {
    /// Registers the seven families; `Duplicate` when the registry already holds them.
    pub fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        Ok(ScrubMetrics {
            running: metrics.gauge("scrub_running")?,
            verified: metrics.counter("scrub_blobs_verified_total")?,
            mismatches: metrics.counter("scrub_mismatches_total")?,
            missing: metrics.counter("scrub_missing_total")?,
            last_completed: metrics.gauge("scrub_last_completed_timestamp_seconds")?,
            last_duration: metrics.gauge("scrub_last_duration_seconds")?,
            throttle_sleep: metrics.counter("scrub_throttle_sleep_seconds_total")?,
            slept_nanos: AtomicU64::new(0),
        })
    }

    pub(super) fn run_started(&self) {
        self.running.set(NoLabels, 1.0);
    }

    pub(super) fn step_done(&self, checked: u64, mismatched: u64, missing: u64) {
        self.verified.inc_by(NoLabels, checked);
        self.mismatches.inc_by(NoLabels, mismatched);
        self.missing.inc_by(NoLabels, missing);
    }

    /// The counter holds whole seconds, so the sleeps are summed in nanoseconds and the whole seconds that the
    /// sum gained are added.
    pub(super) fn throttled(&self, slept: Duration) {
        let nanos = u64::try_from(slept.as_nanos()).unwrap_or(u64::MAX);
        let before = self.slept_nanos.fetch_add(nanos, Ordering::Relaxed);
        let gained = before.saturating_add(nanos) / NANOS_PER_SECOND - before / NANOS_PER_SECOND;
        self.throttle_sleep.inc_by(NoLabels, gained);
    }

    pub(super) fn run_finished(&self, completed_at: Timestamp, duration: Duration) {
        self.running.set(NoLabels, 0.0);
        self.last_completed
            .set(NoLabels, completed_at.as_unix_micros() as f64 / 1_000_000.0);
        self.last_duration.set(NoLabels, duration.as_secs_f64());
    }
}
