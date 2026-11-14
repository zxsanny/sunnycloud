//! The upload series of `deployment/observability.md` § Transfer: `upload_sessions_active`,
//! `upload_sessions_total{result}`, `upload_rejections_total{reason}`, `blocks_received_total` and
//! `block_bytes_received_total`, and the commit series `commits_total{kind, result}` and
//! `commit_duration_seconds{kind}`. A process without a registry leaves them out and records nothing.

use std::time::Duration;

use sc_observability::{Counter, Gauge, Histogram, LabelSet, Metrics, NoLabels, ObsError};

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

/// The `result` label of `upload_sessions_total`; `expired` comes with the reaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionResult {
    Aborted,
    Committed,
}

impl LabelSet for SessionResult {
    const NAMES: &'static [&'static str] = &["result"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            SessionResult::Aborted => "aborted",
            SessionResult::Committed => "committed",
        }
    }
}

/// The `kind` label of the commit series; `single` and `batch` come with the batch commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitKind {
    BlockList,
}

impl LabelSet for CommitKind {
    const NAMES: &'static [&'static str] = &["kind"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            CommitKind::BlockList => "block_list",
        }
    }
}

/// The `result` label of `commits_total`: committed, refused with a typed error, or failed unexpectedly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitResult {
    Committed,
    Refused,
    Failed,
}

/// `commits_total{kind, result}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommitLabels(CommitKind, CommitResult);

impl LabelSet for CommitLabels {
    const NAMES: &'static [&'static str] = &["kind", "result"];

    fn value(&self, index: usize) -> &'static str {
        if index == 0 {
            return self.0.value(0);
        }
        match self.1 {
            CommitResult::Committed => "committed",
            CommitResult::Refused => "refused",
            CommitResult::Failed => "failed",
        }
    }
}

pub(crate) struct UploadMetrics {
    active: Gauge<NoLabels>,
    sessions: Counter<SessionResult>,
    rejections: Counter<Rejection>,
    blocks: Counter<NoLabels>,
    block_bytes: Counter<NoLabels>,
    commits: Counter<CommitLabels>,
    commit_duration: Histogram<CommitKind>,
}

impl UploadMetrics {
    pub(crate) fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        Ok(UploadMetrics {
            active: metrics.gauge("upload_sessions_active")?,
            sessions: metrics.counter("upload_sessions_total")?,
            rejections: metrics.counter("upload_rejections_total")?,
            blocks: metrics.counter("blocks_received_total")?,
            block_bytes: metrics.counter("block_bytes_received_total")?,
            commits: metrics.counter("commits_total")?,
            commit_duration: metrics.histogram("commit_duration_seconds")?,
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

    /// One `commit_upload` call and how long it took.
    pub(crate) fn block_list_commit(&self, result: CommitResult, elapsed: Duration) {
        self.commits
            .inc(CommitLabels(CommitKind::BlockList, result));
        self.commit_duration
            .observe(CommitKind::BlockList, elapsed.as_secs_f64());
    }
}
