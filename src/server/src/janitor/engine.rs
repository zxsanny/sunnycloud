use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::Janitor;
use sc_clock::{Clock, MonotonicInstant};
use sc_observability::{ReadinessCheck, field};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::metrics::{CycleResult, JanitorMetrics};
use super::readiness::{LoopGuard, SchedulerAlive};

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("janitor `{0}` has a zero interval")]
    ZeroInterval(&'static str),
}

/// A janitor with its interval from configuration.
pub struct Scheduled {
    pub janitor: Arc<dyn Janitor>,
    pub interval: Duration,
}

/// One timer loop per janitor. A janitor's loop runs its cycles one after another, so a janitor never overlaps
/// itself; different janitors run concurrently.
pub struct Scheduler {
    stop: CancellationToken,
    loops: Mutex<Vec<JoinHandle<()>>>,
    running: Arc<AtomicUsize>,
    total: usize,
}

impl Scheduler {
    /// Starts one loop per entry on the current runtime. The first cycle of a janitor runs one interval after
    /// the start.
    pub fn start(
        clock: Arc<dyn Clock>,
        metrics: Arc<JanitorMetrics>,
        jobs: Vec<Scheduled>,
    ) -> Result<Scheduler, SchedulerError> {
        if let Some(job) = jobs.iter().find(|job| job.interval.is_zero()) {
            return Err(SchedulerError::ZeroInterval(job.janitor.name()));
        }
        let stop = CancellationToken::new();
        let running = Arc::new(AtomicUsize::new(0));
        let total = jobs.len();
        let begun = clock.monotonic();
        let loops = jobs
            .into_iter()
            .map(|job| {
                tokio::spawn(run_loop(
                    job,
                    begun,
                    Arc::clone(&clock),
                    Arc::clone(&metrics),
                    stop.clone(),
                    LoopGuard::new(&running),
                ))
            })
            .collect();
        Ok(Scheduler {
            stop,
            loops: Mutex::new(loops),
            running,
            total,
        })
    }

    /// The `scheduler_alive` item for the readiness registry.
    pub fn readiness(&self) -> Arc<dyn ReadinessCheck> {
        Arc::new(SchedulerAlive::new(
            self.stop.clone(),
            Arc::clone(&self.running),
            self.total,
        ))
    }

    /// Ends the loops and waits for them. A cycle that is running finishes first (owners bound their cycles by
    /// a time budget); no cycle starts after this returns.
    pub async fn stop(&self) {
        self.stop.cancel();
        let loops = std::mem::take(&mut *self.loops.lock().unwrap_or_else(PoisonError::into_inner));
        for handle in loops {
            if let Err(error) = handle.await {
                tracing::error!(err = field::err(&error), "janitor loop ended abnormally");
            }
        }
    }
}

async fn run_loop(
    job: Scheduled,
    mut started: MonotonicInstant,
    clock: Arc<dyn Clock>,
    metrics: Arc<JanitorMetrics>,
    stop: CancellationToken,
    _running: LoopGuard,
) {
    let name = job.janitor.name();
    loop {
        let Some(next) = started.checked_add(job.interval) else {
            tracing::error!("janitor {name} interval overflows the clock; its loop stops");
            return;
        };
        tokio::select! {
            biased;
            () = stop.cancelled() => return,
            woke = clock.sleep_until_monotonic(next) => {
                if woke.is_err() {
                    return;
                }
            }
        }
        started = clock.monotonic();
        run_cycle(&job.janitor, name, clock.as_ref(), &metrics).await;
    }
}

/// Runs one cycle on its own task, so a panic surfaces as a `JoinError` instead of ending the loop.
async fn run_cycle(
    janitor: &Arc<dyn Janitor>,
    name: &'static str,
    clock: &dyn Clock,
    metrics: &JanitorMetrics,
) {
    let now = clock.now_utc();
    let task = Arc::clone(janitor);
    let result = match tokio::spawn(async move { task.run_cycle(now).await }).await {
        Ok(Ok(_)) => CycleResult::Ok,
        Ok(Err(error)) => {
            tracing::error!(err = field::err(&error), "janitor {name} cycle failed");
            CycleResult::Failed
        }
        Err(error) if error.is_panic() => {
            tracing::error!("janitor {name} cycle panicked");
            CycleResult::Failed
        }
        Err(_) => {
            tracing::error!("janitor {name} cycle was cancelled");
            CycleResult::Failed
        }
    };
    metrics.cycle_finished(name, result);
    if result == CycleResult::Ok {
        let micros = clock.now_utc().as_unix_micros();
        metrics.succeeded_at(name, micros as f64 / 1_000_000.0);
    }
}
