use std::sync::Arc;
use std::time::Duration;

use media_shared::decoder_protocol::{DecodeError, LimitKind};
use media_shared::types::{Disposition, JobKind};
use sc_observability::{Counter, Histogram, LabelSet, Metrics, NoLabels, ObsError};

use super::JobEnd;
use crate::decoder_client::{CallError, Unhealthy};

/// Why a decoder call was killed (`decoder_kills_total{reason}`, `observability.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillReason {
    /// The runner killed the encoder for lack of progress (tier 1), or the runner went silent (tier 2).
    Stall,
    /// The wall limit of the job kind ran out.
    Timeout,
    /// The memory limit was exceeded.
    Memory,
}

impl LabelSet for KillReason {
    const NAMES: &'static [&'static str] = &["reason"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            KillReason::Stall => "stall",
            KillReason::Timeout => "timeout",
            KillReason::Memory => "memory",
        }
    }
}

impl KillReason {
    /// A decoder answer that reports a kill. A closed socket says nothing about why, so it is not counted.
    fn of_answer(error: &CallError) -> Option<Self> {
        match error {
            CallError::Decode(DecodeError::Stalled) => Some(KillReason::Stall),
            CallError::Decode(DecodeError::LimitExceeded { limit }) => match limit {
                LimitKind::Wall => Some(KillReason::Timeout),
                LimitKind::Memory => Some(KillReason::Memory),
                LimitKind::Pixels | LimitKind::Output => None,
            },
            _ => None,
        }
    }

    fn of_unhealthy(why: Unhealthy) -> Option<Self> {
        match why {
            Unhealthy::Silent => Some(KillReason::Stall),
            Unhealthy::WallLimit => Some(KillReason::Timeout),
            Unhealthy::Violation => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct JobLabels {
    kind: JobKind,
    result: &'static str,
}

impl LabelSet for JobLabels {
    const NAMES: &'static [&'static str] = &["kind", "result"];

    fn value(&self, index: usize) -> &'static str {
        if index == 0 {
            match self.kind {
                JobKind::Stills => "stills",
                JobKind::Poster => "poster",
                JobKind::Transcode => "transcode",
            }
        } else {
            self.result
        }
    }
}

impl JobEnd {
    /// The `result` label: a fixed enumeration. The documents name `ok`, `obsolete` and `remuxed`; the others
    /// are the remaining ways a job ends.
    fn result_label(&self) -> &'static str {
        match self {
            JobEnd::Derived => "ok",
            JobEnd::Remuxed => "remuxed",
            JobEnd::SkippedCompliant => "skipped_compliant",
            JobEnd::Obsolete => "obsolete",
            JobEnd::Failed(report) => match report.disposition {
                Disposition::Retry => "retry",
                Disposition::Permanent => "permanent",
            },
            JobEnd::LeaseLost => "lease_lost",
            JobEnd::Refused => "refused",
            JobEnd::Released => "released",
        }
    }
}

/// The job families of the loop, registered once per process and shared by every slot.
pub struct JobMetrics {
    jobs: Counter<JobLabels>,
    duration: Histogram<NoLabels>,
    kills: Counter<KillReason>,
}

impl JobMetrics {
    /// Registers `jobs_total{kind,result}`, `job_duration_seconds` and `decoder_kills_total{reason}`.
    pub fn register(metrics: &Metrics) -> Result<Arc<Self>, ObsError> {
        Ok(Arc::new(Self {
            jobs: metrics.counter("jobs_total")?,
            duration: metrics.histogram("job_duration_seconds")?,
            kills: metrics.counter("decoder_kills_total")?,
        }))
    }

    pub(super) fn record(&self, kind: JobKind, end: &JobEnd, elapsed: Duration) {
        self.jobs.inc(JobLabels {
            kind,
            result: end.result_label(),
        });
        self.duration.observe(NoLabels, elapsed.as_secs_f64());
    }

    pub(super) fn record_decoder_error(&self, error: &CallError) {
        if let Some(reason) = KillReason::of_answer(error) {
            self.kills.inc(reason);
        }
    }

    pub(super) fn record_kill(&self, why: Unhealthy) {
        if let Some(reason) = KillReason::of_unhealthy(why) {
            self.kills.inc(reason);
        }
    }

    /// Decoder kills counted for `reason`.
    pub fn kills(&self, reason: KillReason) -> u64 {
        self.kills.get(reason)
    }

    /// Jobs of `kind` that ended like `end`.
    pub fn jobs(&self, kind: JobKind, end: &JobEnd) -> u64 {
        self.jobs.get(JobLabels {
            kind,
            result: end.result_label(),
        })
    }
}
