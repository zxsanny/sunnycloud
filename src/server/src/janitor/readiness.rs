use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use sc_observability::{CheckFuture, ReadinessCheck, ReadinessItem};
use tokio_util::sync::CancellationToken;

/// The `scheduler_alive` item of `/readyz`: Ok while every janitor loop runs, Failing before the scheduler has
/// started and once it has been stopped or a loop has ended.
pub struct SchedulerAlive {
    stop: CancellationToken,
    running: Arc<AtomicUsize>,
    /// The number of loops, set when the scheduler starts.
    total: Arc<OnceLock<usize>>,
}

impl SchedulerAlive {
    pub(crate) fn new(
        stop: CancellationToken,
        running: Arc<AtomicUsize>,
        total: Arc<OnceLock<usize>>,
    ) -> Self {
        SchedulerAlive {
            stop,
            running,
            total,
        }
    }
}

impl ReadinessCheck for SchedulerAlive {
    fn name(&self) -> &str {
        "scheduler_alive"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(async move {
            let running = self.running.load(Ordering::Acquire);
            match self.total.get() {
                _ if self.stop.is_cancelled() => ReadinessItem::failing("scheduler stopped"),
                None => ReadinessItem::failing("scheduler not started"),
                Some(&total) if running < total => {
                    ReadinessItem::failing(format!("{running} of {total} janitor loops running"))
                }
                Some(_) => ReadinessItem::ok(),
            }
        })
    }
}

/// Counts one janitor loop as running until it is dropped, however the loop ends.
pub(crate) struct LoopGuard(Arc<AtomicUsize>);

impl LoopGuard {
    pub(crate) fn new(running: &Arc<AtomicUsize>) -> Self {
        running.fetch_add(1, Ordering::AcqRel);
        LoopGuard(Arc::clone(running))
    }
}

impl Drop for LoopGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
