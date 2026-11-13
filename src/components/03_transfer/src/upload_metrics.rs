//! The upload series of `deployment/observability.md` § Transfer: `upload_sessions_active`,
//! `upload_rejections_total{reason}`, `blocks_received_total` and `block_bytes_received_total`. A process without a registry leaves them out and records nothing.

use sc_observability::{Counter, Gauge, LabelSet, Metrics, NoLabels, ObsError};

/// The `reason` label of `upload_rejections_total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rejection {
    Quota,
    SessionCap,
    FreeFloor,
}

impl LabelSet for Rejection {
    const NAMES: &'static [&'static str] = &["reason"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            Rejection::Quota => "quota",
            Rejection::SessionCap => "session_cap",
            Rejection::FreeFloor => "free_floor",
        }
    }
}

pub(crate) struct UploadMetrics {
    active: Gauge<NoLabels>,
    rejections: Counter<Rejection>,
    blocks: Counter<NoLabels>,
    block_bytes: Counter<NoLabels>,
}

impl UploadMetrics {
    pub(crate) fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        Ok(UploadMetrics {
            active: metrics.gauge("upload_sessions_active")?,
            rejections: metrics.counter("upload_rejections_total")?,
            blocks: metrics.counter("blocks_received_total")?,
            block_bytes: metrics.counter("block_bytes_received_total")?,
        })
    }

    pub(crate) fn session_opened(&self) {
        self.active.add(NoLabels, 1.0);
    }

    pub(crate) fn rejected(&self, reason: Rejection) {
        self.rejections.inc(reason);
    }

    /// A new block made durable.
    pub(crate) fn block_received(&self, bytes: u64) {
        self.blocks.inc(NoLabels);
        self.block_bytes.inc_by(NoLabels, bytes);
    }
}
