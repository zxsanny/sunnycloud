use media_shared::types::JobKind;
use sc_observability::{Counter, Gauge, Histogram, LabelSet, Metrics, NoLabels, ObsError};

use crate::job_names::kind_name;
use crate::job_repo::Closed;

/// The `kind` label: the job kind (`stills`, `poster`, `transcode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindLabel(pub JobKind);

impl LabelSet for KindLabel {
    const NAMES: &'static [&'static str] = &["kind"];

    fn value(&self, _index: usize) -> &'static str {
        kind_name(self.0)
    }
}

const KINDS: [JobKind; 3] = [JobKind::Stills, JobKind::Poster, JobKind::Transcode];

/// `job_leases_expired_total` moves when a lease is found expired (a running job taken over, or closed dead);
/// `jobs_dead_total` when a job becomes dead; `worker_auth_failures_total` on every refused worker credential.
/// `job_duration_seconds{kind}` observes, when a job becomes `succeeded` or `dead`, the seconds from its enqueue;
/// `derived_bytes_total{kind}` adds the size of every accepted output. `jobs_queued` and `jobs_running` are set
/// from the table before each scrape (`MediaAdminService` as the scrape probe). The counters start at 0 so the
/// series are in the first scrape.
pub struct JobMetrics {
    pub leases_expired: Counter<NoLabels>,
    pub dead: Counter<NoLabels>,
    pub worker_auth_failures: Counter<NoLabels>,
    pub queued: Gauge<NoLabels>,
    pub running: Gauge<NoLabels>,
    pub duration: Histogram<KindLabel>,
    pub derived_bytes: Counter<KindLabel>,
}

impl JobMetrics {
    pub fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        let leases_expired = metrics.counter::<NoLabels>("job_leases_expired_total")?;
        let dead = metrics.counter::<NoLabels>("jobs_dead_total")?;
        let worker_auth_failures = metrics.counter::<NoLabels>("worker_auth_failures_total")?;
        for counter in [&leases_expired, &dead, &worker_auth_failures] {
            counter.inc_by(NoLabels, 0);
        }
        let queued = metrics.gauge::<NoLabels>("jobs_queued")?;
        let running = metrics.gauge::<NoLabels>("jobs_running")?;
        let duration = metrics.histogram::<KindLabel>("job_duration_seconds")?;
        let derived_bytes = metrics.counter::<KindLabel>("derived_bytes_total")?;
        for kind in KINDS {
            derived_bytes.inc_by(KindLabel(kind), 0);
        }
        Ok(JobMetrics {
            leases_expired,
            dead,
            worker_auth_failures,
            queued,
            running,
            duration,
            derived_bytes,
        })
    }

    /// A job became `succeeded`.
    pub(crate) fn succeeded(&self, closed: Closed) {
        self.duration
            .observe(KindLabel(closed.kind), closed.seconds);
    }

    /// A job became `dead`.
    pub(crate) fn died(&self, closed: Closed) {
        self.dead.inc(NoLabels);
        self.duration
            .observe(KindLabel(closed.kind), closed.seconds);
    }
}
