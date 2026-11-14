//! The upload series of `deployment/observability.md` § Transfer: `upload_sessions_active`,
//! `upload_sessions_total{result}`, `upload_rejections_total{reason}`, `blocks_received_total` and
//! `block_bytes_received_total`. A process without a registry leaves them out and records nothing.

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

/// The `result` label of `upload_sessions_total`; `committed` and `expired` come with commit and the reaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionResult {
    Aborted,
}

impl LabelSet for SessionResult {
    const NAMES: &'static [&'static str] = &["result"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            SessionResult::Aborted => "aborted",
        }
    }
}

pub(crate) struct UploadMetrics {
    active: Gauge<NoLabels>,
    sessions: Counter<SessionResult>,
    rejections: Counter<Rejection>,
    blocks: Counter<NoLabels>,
    block_bytes: Counter<NoLabels>,
}

impl UploadMetrics {
    pub(crate) fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        Ok(UploadMetrics {
            active: metrics.gauge("upload_sessions_active")?,
            sessions: metrics.counter("upload_sessions_total")?,
            rejections: metrics.counter("upload_rejections_total")?,
            blocks: metrics.counter("blocks_received_total")?,
            block_bytes: metrics.counter("block_bytes_received_total")?,
        })
    }

    /// A session entered this process's progress table: started, or loaded after a restart.
    pub(crate) fn session_opened(&self) {
        self.active.add(NoLabels, 1.0);
    }

    /// A session left this process's progress table.
    pub(crate) fn session_closed(&self) {
        self.active.add(NoLabels, -1.0);
    }

    pub(crate) fn session_ended(&self, result: SessionResult) {
        self.sessions.inc(result);
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
