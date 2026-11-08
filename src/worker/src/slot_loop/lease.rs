//! The lease heartbeat of one running job (09 section 3 steps 2 and 7).

use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use media_shared::types::ClaimedJob;
use sc_observability::log_warn;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use uuid::Uuid;

use crate::api::{ApiError, WorkerApiClient};

/// Why the job can no longer be worked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaseEnd {
    /// 409: another worker holds the job. Nothing more is sent for it.
    Lost,
    /// 403: the worker secret is refused.
    Refused,
}

/// Extends the lease of one job every `lease_ms / 3` for as long as the value lives. Dropping it stops the
/// heartbeat, so no exit path of the job leaves one running.
pub(super) struct Lease {
    ended: watch::Receiver<Option<LeaseEnd>>,
    task: JoinHandle<()>,
}

impl Lease {
    pub(super) fn start(api: Arc<WorkerApiClient>, job: &ClaimedJob) -> Self {
        let period = Duration::from_millis(job.lease_ms / 3).max(Duration::from_millis(1));
        let (ended_tx, ended) = watch::channel(None);
        let task = tokio::spawn(beat(api, job.job_id, period, ended_tx));
        Self { ended, task }
    }

    /// Completes when a heartbeat learns the lease is gone; never completes while the job holds it.
    pub(super) async fn ended(&mut self) -> LeaseEnd {
        if let Ok(state) = self.ended.wait_for(Option::is_some).await
            && let Some(end) = *state
        {
            return end;
        }
        pending().await
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A failed heartbeat that is neither 409 nor 403 is logged and tried again one period later: the lease
/// has two more periods to run, and a later 409 ends the job if it did expire.
async fn beat(
    api: Arc<WorkerApiClient>,
    job_id: Uuid,
    period: Duration,
    ended: watch::Sender<Option<LeaseEnd>>,
) {
    loop {
        sleep(period).await;
        match api.heartbeat(job_id).await {
            Ok(_) => {}
            Err(ApiError::LeaseLost) => {
                ended.send_replace(Some(LeaseEnd::Lost));
                return;
            }
            Err(ApiError::Forbidden) => {
                ended.send_replace(Some(LeaseEnd::Refused));
                return;
            }
            Err(error) => log_warn!("lease heartbeat failed", err = &error),
        }
    }
}
