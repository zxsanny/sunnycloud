//! The blob I/O series of `deployment/observability.md` § Transfer: `block_fsync_duration_seconds`,
//! `blob_read_duration_seconds`, `blob_free_bytes` and `blob_free_floor_bytes`. The scrub series are
//! [`crate::ScrubMetrics`]. Each owner of a hot path takes the handle with a `with_metrics` builder; a process
//! that has no registry (the CLI) leaves it out and records nothing.

use std::sync::Arc;

use sc_clock::{Clock, MonotonicInstant};
use sc_observability::{Gauge, Histogram, LabelSet, Metrics, NoLabels, ObsError};

/// The `kind` label of `blob_read_duration_seconds`: a read with a byte range, or one of the whole blob.
#[derive(Clone, Copy)]
pub(crate) enum ReadKind {
    Range,
    Stream,
}

impl LabelSet for ReadKind {
    const NAMES: &'static [&'static str] = &["kind"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            ReadKind::Range => "range",
            ReadKind::Stream => "stream",
        }
    }
}

pub struct BlobMetrics {
    clock: Arc<dyn Clock>,
    fsync: Histogram<NoLabels>,
    read: Histogram<ReadKind>,
    free: Gauge<NoLabels>,
    floor: Gauge<NoLabels>,
}

impl BlobMetrics {
    /// Registers the four families; `Duplicate` when the registry already holds them. Durations are measured on
    /// `clock`'s monotonic time.
    pub fn register(metrics: &Metrics, clock: Arc<dyn Clock>) -> Result<Arc<Self>, ObsError> {
        Ok(Arc::new(BlobMetrics {
            clock,
            fsync: metrics.histogram("block_fsync_duration_seconds")?,
            read: metrics.histogram("blob_read_duration_seconds")?,
            free: metrics.gauge("blob_free_bytes")?,
            floor: metrics.gauge("blob_free_floor_bytes")?,
        }))
    }

    pub(crate) fn begin(&self) -> MonotonicInstant {
        self.clock.monotonic()
    }

    /// One group flush that began at `began`, whatever its outcome.
    pub(crate) fn flushed(&self, began: MonotonicInstant) {
        self.fsync.observe(NoLabels, self.since(began));
    }

    /// One `open` that began at `began` and returned its first authenticated chunk.
    pub(crate) fn opened(&self, kind: ReadKind, began: MonotonicInstant) {
        self.read.observe(kind, self.since(began));
    }

    pub(crate) fn volume_probed(&self, free_bytes: u64, reserve: u64) {
        self.free.set(NoLabels, free_bytes as f64);
        self.floor.set(NoLabels, reserve as f64);
    }

    fn since(&self, began: MonotonicInstant) -> f64 {
        self.clock
            .monotonic()
            .saturating_duration_since(began)
            .as_secs_f64()
    }
}
