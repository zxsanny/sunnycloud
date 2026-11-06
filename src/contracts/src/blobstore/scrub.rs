use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use thiserror::Error;

use crate::common::Fault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrubMode {
    /// Every committed blob.
    Full,
    /// Blobs not scrubbed within `scrub_interval`.
    Due,
}

/// Read limit of a scrub run, in bytes per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRate(pub u64);

/// `started_at` is the run-start cursor: the run covers only blobs committed at or before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrubRun {
    pub started_at: Timestamp,
}

/// `done` is true when the run's set is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepResult {
    pub checked: u64,
    pub mismatched: u64,
    pub missing: u64,
    pub done: bool,
}

/// The counts and duration of the `scrub_summary` audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrubSummary {
    pub checked: u64,
    pub mismatched: u64,
    pub missing: u64,
    pub duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrubStatus {
    pub oldest_scrubbed_at: Option<Timestamp>,
    pub unscrubbed: u64,
    pub latest_summary: Option<ScrubSummary>,
    /// Blobs with `unavailable_at` set.
    pub unavailable: u64,
}

#[derive(Debug, Error)]
pub enum ScrubError {
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait Scrubber: Send + Sync {
    /// `rate` defaults to `scrub_max_rate` when `None`.
    async fn start_run(
        &self,
        mode: ScrubMode,
        rate: Option<ByteRate>,
    ) -> Result<ScrubRun, ScrubError>;

    async fn run_step(&self, run: &ScrubRun) -> Result<StepResult, ScrubError>;

    /// Writes one `scrub_summary` audit event and sets the scrub metrics.
    async fn finish_run(&self, run: &ScrubRun) -> Result<(), ScrubError>;

    async fn status(&self) -> Result<ScrubStatus, ScrubError>;
}
