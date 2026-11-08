//! The per-slot work loop (09 section 3, E-20/c06): claim from the slot's lanes, drive one job at a time to
//! its terminal call, and always remove the job's scratch tree.
//!
//! What the step is, whether an output is compliant and every other media rule come from
//! [`crate::policy_step`] and `media-shared`; this module only runs them and maps each outcome to a call of
//! the API. Around the job the loop keeps the lease alive ([`lease`]) and bounds every decoder call
//! ([`watchdog`]): a lost lease, a silent decoder or a call past its wall limit cancels the call, and the last
//! two take the slot out of service until it has been through the gate again (E-20/c07).

mod access;
mod config;
mod failure;
mod job;
mod lease;
mod metrics;
mod shutdown;
mod watchdog;

use std::sync::Arc;

use media_shared::types::{ClaimedJob, FailureReport, Lane, SkipOutcome};
use sc_observability::{log_error, log_warn};
use tokio::time::{Instant, sleep};

pub use access::ApiAccess;
pub use config::{KindConfig, LoopConfig};
pub use metrics::{JobMetrics, KillReason};
pub use shutdown::Shutdown;

use crate::api::{ApiError, WorkerApiClient};
use crate::decoder_client::{GatedConn, Slot, SlotName, SlotReadiness};
use crate::scratch::ScratchRoot;
use failure::Stop;
use job::{Done, JobRun};
use lease::Lease;
use watchdog::Watchdog;

/// How a claimed job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEnd {
    /// Every output was accepted (the API closes the job itself).
    Derived,
    /// As `Derived`, with a lossless remux as the output.
    Remuxed,
    SkippedCompliant,
    Obsolete,
    /// The failure the API was sent.
    Failed(FailureReport),
    /// 409: the job belongs to someone else now; nothing more was sent.
    LeaseLost,
    /// 403: the worker is not ready and stops claiming; nothing more was sent.
    Refused,
    /// Shutdown: the grace passed, the decoder call was cancelled and the lease released.
    Released,
}

/// What one round of the loop did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotStep {
    /// No lane had work, or a claim failed. Wait `worker_claim_poll_interval` before the next round.
    Idle,
    Ran(JobEnd),
    /// The API refused the worker secret: no more claims.
    Stopped,
    /// Shutdown started: no more claims.
    Drained,
}

/// The lanes a slot claims, first one first. The interactive slot takes `image` only, so a photo never waits
/// behind a video; the general slot takes both and starts with `first`.
pub(crate) fn lane_order(slot: SlotName, first: Lane) -> &'static [Lane] {
    match (slot, first) {
        (SlotName::Interactive, _) => &[Lane::Image],
        (SlotName::General, Lane::Video) => &[Lane::Video, Lane::Image],
        (SlotName::General, Lane::Image) => &[Lane::Image, Lane::Video],
    }
}

fn other(lane: Lane) -> Lane {
    match lane {
        Lane::Image => Lane::Video,
        Lane::Video => Lane::Image,
    }
}

/// One decoder slot's job loop. Jobs run one at a time, so a slot never holds two.
pub struct SlotLoop {
    api: Arc<WorkerApiClient>,
    access: ApiAccess,
    slot: Slot,
    scratch: ScratchRoot,
    config: LoopConfig,
    metrics: Arc<JobMetrics>,
    shutdown: Shutdown,
    gated: Option<GatedConn>,
    /// The general slot's first lane for the next round; it flips after every round.
    first_lane: Lane,
}

impl SlotLoop {
    pub fn new(
        api: Arc<WorkerApiClient>,
        access: ApiAccess,
        slot: Slot,
        scratch: ScratchRoot,
        config: LoopConfig,
        metrics: Arc<JobMetrics>,
        shutdown: Shutdown,
    ) -> Self {
        Self {
            api,
            access,
            slot,
            scratch,
            config,
            metrics,
            shutdown,
            gated: None,
            first_lane: Lane::Video,
        }
    }

    /// The `/readyz` condition of this slot: not ready while the decoder is connecting, failed its gate or was
    /// taken out of service by the watchdog.
    pub fn readiness(&self) -> SlotReadiness {
        self.slot.readiness()
    }

    /// Rounds until the API refuses the worker secret or the shutdown starts. A job that is running when the
    /// shutdown starts goes on until it ends or the grace passes.
    pub async fn run(&mut self) {
        loop {
            match self.step().await {
                SlotStep::Idle => {
                    tokio::select! {
                        () = sleep(self.config.claim_poll_interval) => {}
                        () = self.shutdown.draining() => {}
                    }
                }
                SlotStep::Ran(_) => {}
                SlotStep::Stopped | SlotStep::Drained => return,
            }
        }
    }

    /// One round: make sure the decoder passed its gate, then claim from each lane in turn until one gives a
    /// job, and run it to its end.
    pub async fn step(&mut self) -> SlotStep {
        if self.access.is_refused() {
            return SlotStep::Stopped;
        }
        if self.shutdown.is_draining() {
            return SlotStep::Drained;
        }
        let mut gated = match self.gated.take() {
            Some(gated) if gated.conn.is_open() => gated,
            _ => tokio::select! {
                biased;
                () = self.shutdown.draining() => return SlotStep::Drained,
                gated = self.slot.next_ready() => gated,
            },
        };
        let step = self.claim_round(&mut gated).await;
        self.gated = Some(gated);
        step
    }

    async fn claim_round(&mut self, gated: &mut GatedConn) -> SlotStep {
        let lanes = lane_order(self.slot.name(), self.first_lane);
        self.first_lane = other(self.first_lane);
        for lane in lanes {
            if self.shutdown.is_draining() {
                return SlotStep::Drained;
            }
            match self.api.claim(*lane).await {
                Ok(Some(job)) => return SlotStep::Ran(self.run_job(job, gated).await),
                Ok(None) => {}
                Err(ApiError::Forbidden) => {
                    self.refuse();
                    return SlotStep::Stopped;
                }
                Err(error) => {
                    log_warn!("claim failed", err = &error);
                    return SlotStep::Idle;
                }
            }
        }
        SlotStep::Idle
    }

    fn refuse(&self) {
        log_error!("the API refused the worker secret; no more claims");
        self.access.refuse();
    }

    async fn run_job(&self, job: ClaimedJob, gated: &mut GatedConn) -> JobEnd {
        let started = Instant::now();
        let end = self.execute(&job, gated).await;
        self.metrics.record(job.kind, &end, started.elapsed());
        end
    }

    /// Scratch exists for the call and is removed on every path out of it; a panic is covered by the tree's
    /// `Drop`.
    async fn execute(&self, job: &ClaimedJob, gated: &mut GatedConn) -> JobEnd {
        let mut dir = match self.scratch.create(job.job_id) {
            Ok(dir) => dir,
            Err(error) => return self.finish(job, Err(Stop::from(&error))).await,
        };
        let kind = self.config.for_kind(job.kind);
        let mut lease = Lease::start(Arc::clone(&self.api), job, self.shutdown.clone());
        let outcome = JobRun {
            api: &self.api,
            job,
            root: &self.scratch,
            scratch: &dir,
            conn: &mut gated.conn,
            capabilities: &gated.capabilities,
            kind,
            lease: &mut lease,
            watchdog: Watchdog::new(&self.config, kind),
            metrics: &self.metrics,
            reran: false,
        }
        .run()
        .await;
        drop(lease);
        if matches!(outcome, Err(Stop::Unhealthy(_))) {
            gated.conn.close();
        }
        let end = self.finish(job, outcome).await;
        if let Err(error) = dir.remove() {
            log_error!("job scratch tree not removed", err = &error);
        }
        end
    }

    /// The one call that ends the job, if it needs one.
    async fn finish(&self, job: &ClaimedJob, outcome: Result<Done, Stop>) -> JobEnd {
        match outcome {
            Ok(Done::Derived) => JobEnd::Derived,
            Ok(Done::Remuxed) => JobEnd::Remuxed,
            Ok(Done::SkippedCompliant) => JobEnd::SkippedCompliant,
            Err(Stop::LeaseLost) => JobEnd::LeaseLost,
            Err(Stop::Refused) => {
                self.refuse();
                JobEnd::Refused
            }
            Err(Stop::Obsolete) => self.close_obsolete(job).await,
            Err(Stop::Released) => self.release(job).await,
            Err(Stop::Unhealthy(why)) => {
                self.metrics.record_kill(why);
                self.slot.mark_unhealthy(why);
                self.report_failure(job, why.report()).await
            }
            Err(Stop::Fail(report)) => self.report_failure(job, report).await,
        }
    }

    async fn close_obsolete(&self, job: &ClaimedJob) -> JobEnd {
        match self.api.complete(job.job_id, SkipOutcome::Obsolete).await {
            Ok(()) => JobEnd::Obsolete,
            Err(error) => self.call_failed(&error, JobEnd::Obsolete),
        }
    }

    /// Gives the lease back so the job is claimable at once instead of after the lease expires.
    async fn release(&self, job: &ClaimedJob) -> JobEnd {
        match self.api.release(job.job_id).await {
            Ok(()) => JobEnd::Released,
            Err(error) => self.call_failed(&error, JobEnd::Released),
        }
    }

    async fn report_failure(&self, job: &ClaimedJob, report: FailureReport) -> JobEnd {
        match self.api.fail(job.job_id, &report).await {
            Ok(_) => JobEnd::Failed(report),
            Err(error) => self.call_failed(&error, JobEnd::Failed(report)),
        }
    }

    /// A closing call that was not accepted: 409 and 403 stop quietly; anything else is logged, and the lease
    /// expiry hands the job to a later claim.
    fn call_failed(&self, error: &ApiError, ended: JobEnd) -> JobEnd {
        match error {
            ApiError::LeaseLost => JobEnd::LeaseLost,
            ApiError::Forbidden => {
                self.refuse();
                JobEnd::Refused
            }
            other => {
                log_warn!("closing call of the job failed", err = other);
                ended
            }
        }
    }
}

#[cfg(test)]
mod tests;
