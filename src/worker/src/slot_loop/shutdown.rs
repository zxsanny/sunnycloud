use std::sync::Arc;

use tokio::sync::watch;

/// How far the process has gone towards stopping. The order is the order of the steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Running,
    /// SIGTERM: no new claim; jobs already running go on.
    Draining,
    /// `worker_shutdown_grace` passed: running jobs are cancelled and released.
    Aborting,
}

/// The two-step stop of the slot loops (09 section 3, step 7). Clones share the phase; the supervisor moves it
/// forward with [`Shutdown::drain`] and later [`Shutdown::abort`], and never back.
#[derive(Debug, Clone)]
pub struct Shutdown {
    phase: Arc<watch::Sender<Phase>>,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    pub fn new() -> Self {
        Self {
            phase: Arc::new(watch::channel(Phase::Running).0),
        }
    }

    /// Slots stop claiming; running jobs are left to finish.
    pub fn drain(&self) {
        self.advance(Phase::Draining);
    }

    /// Running jobs get `cancel` and a lease release. Implies [`Shutdown::drain`].
    pub fn abort(&self) {
        self.advance(Phase::Aborting);
    }

    pub fn is_draining(&self) -> bool {
        *self.phase.borrow() >= Phase::Draining
    }

    pub(super) async fn draining(&self) {
        self.reached(Phase::Draining).await;
    }

    pub(super) async fn aborting(&self) {
        self.reached(Phase::Aborting).await;
    }

    fn advance(&self, to: Phase) {
        self.phase.send_if_modified(|phase| {
            let moved = *phase < to;
            if moved {
                *phase = to;
            }
            moved
        });
    }

    async fn reached(&self, phase: Phase) {
        let mut seen = self.phase.subscribe();
        // The sender lives in `self`, so the wait cannot fail while this future exists.
        if seen.wait_for(|now| *now >= phase).await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}
