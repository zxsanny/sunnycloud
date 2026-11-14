use sc_observability::{Counter, Metrics, NoLabels, ObsError};

/// `job_leases_expired_total` moves when a lease is found expired (a running job taken over, or closed dead);
/// `jobs_dead_total` when a job becomes dead; `worker_auth_failures_total` on every refused worker credential.
/// All start at 0 so the series are in the first scrape.
pub struct JobMetrics {
    pub leases_expired: Counter<NoLabels>,
    pub dead: Counter<NoLabels>,
    pub worker_auth_failures: Counter<NoLabels>,
}

impl JobMetrics {
    pub fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        let leases_expired = metrics.counter::<NoLabels>("job_leases_expired_total")?;
        let dead = metrics.counter::<NoLabels>("jobs_dead_total")?;
        let worker_auth_failures = metrics.counter::<NoLabels>("worker_auth_failures_total")?;
        for counter in [&leases_expired, &dead, &worker_auth_failures] {
            counter.inc_by(NoLabels, 0);
        }
        Ok(JobMetrics {
            leases_expired,
            dead,
            worker_auth_failures,
        })
    }
}
