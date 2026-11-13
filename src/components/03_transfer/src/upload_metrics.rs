//! The upload series of `deployment/observability.md` § Transfer: `upload_sessions_active` and
//! `upload_rejections_total{reason}`. A process without a registry leaves them out and records nothing.

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
}

impl UploadMetrics {
    pub(crate) fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        Ok(UploadMetrics {
            active: metrics.gauge("upload_sessions_active")?,
            rejections: metrics.counter("upload_rejections_total")?,
        })
    }

    pub(crate) fn session_opened(&self) {
        self.active.add(NoLabels, 1.0);
    }

    pub(crate) fn rejected(&self, reason: Rejection) {
        self.rejections.inc(reason);
    }
}
