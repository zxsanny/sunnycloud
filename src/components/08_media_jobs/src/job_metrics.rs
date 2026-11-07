use sc_observability::{Counter, Metrics, NoLabels, ObsError};

/// `job_leases_expired_total` moves when a lease is found expired (a running job taken over, or closed dead);
/// `jobs_dead_total` when a job becomes dead. Both start at 0 so the series are in the first scrape.
pub struct JobMetrics {
    pub leases_expired: Counter<NoLabels>,
    pub dead: Counter<NoLabels>,
}

impl JobMetrics {
    pub fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        let leases_expired = metrics.counter::<NoLabels>("job_leases_expired_total")?;
        let dead = metrics.counter::<NoLabels>("jobs_dead_total")?;
        leases_expired.inc_by(NoLabels, 0);
        dead.inc_by(NoLabels, 0);
        Ok(JobMetrics {
            leases_expired,
            dead,
        })
    }
}
