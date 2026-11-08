use std::sync::Arc;
use std::time::Duration;

use media_shared::types::{Disposition, JobKind};
use sc_observability::{Counter, Histogram, LabelSet, Metrics, NoLabels, ObsError};

use super::JobEnd;

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
        }
    }
}

/// The job families of the loop, registered once per process and shared by every slot.
pub struct JobMetrics {
    jobs: Counter<JobLabels>,
    duration: Histogram<NoLabels>,
}

impl JobMetrics {
    /// Registers `jobs_total{kind,result}` and `job_duration_seconds`.
    pub fn register(metrics: &Metrics) -> Result<Arc<Self>, ObsError> {
        Ok(Arc::new(Self {
            jobs: metrics.counter("jobs_total")?,
            duration: metrics.histogram("job_duration_seconds")?,
        }))
    }

    pub(super) fn record(&self, kind: JobKind, end: &JobEnd, elapsed: Duration) {
        self.jobs.inc(JobLabels {
            kind,
            result: end.result_label(),
        });
        self.duration.observe(NoLabels, elapsed.as_secs_f64());
    }

    /// Jobs of `kind` that ended like `end`.
    pub fn jobs(&self, kind: JobKind, end: &JobEnd) -> u64 {
        self.jobs.get(JobLabels {
            kind,
            result: end.result_label(),
        })
    }
}
