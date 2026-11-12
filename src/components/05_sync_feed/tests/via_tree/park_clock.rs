//! A clock that tells the test when a hold has parked: every `sleep_until_monotonic` call is counted, and a hold
//! calls it once per loop turn right before it awaits. Time itself comes from the wrapped clock (a manual one
//! that moves only when ticked, or the system clock).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sc_clock::{Clock, MonotonicInstant, SleepFuture, Timestamp};
use tokio::sync::Notify;

const HANG_GUARD: Duration = Duration::from_secs(60);

pub struct ParkClock {
    inner: Arc<dyn Clock>,
    sleeps: AtomicUsize,
    slept: Notify,
}

impl ParkClock {
    pub fn over(inner: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(ParkClock {
            inner,
            sleeps: AtomicUsize::new(0),
            slept: Notify::new(),
        })
    }

    pub fn sleeps(&self) -> usize {
        self.sleeps.load(Ordering::SeqCst)
    }

    /// Returns once `target` sleeps have been started in total; a hang guard fails the test instead of hanging.
    pub async fn sleeps_reach(&self, target: usize) {
        let reached = async {
            loop {
                let seen = self.slept.notified();
                tokio::pin!(seen);
                seen.as_mut().enable();
                if self.sleeps() >= target {
                    return;
                }
                seen.await;
            }
        };
        tokio::time::timeout(HANG_GUARD, reached)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "only {} of {target} sleeps started: a hold never parked",
                    self.sleeps()
                )
            });
    }
}

impl Clock for ParkClock {
    fn now_utc(&self) -> Timestamp {
        self.inner.now_utc()
    }

    fn monotonic(&self) -> MonotonicInstant {
        self.inner.monotonic()
    }

    fn sleep_until_monotonic(&self, at: MonotonicInstant) -> SleepFuture<'_> {
        self.sleeps.fetch_add(1, Ordering::SeqCst);
        self.slept.notify_waiters();
        self.inner.sleep_until_monotonic(at)
    }
}
