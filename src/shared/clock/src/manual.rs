use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

use crate::{CancellationToken, Cancelled, Clock, MonotonicInstant, SleepFuture, Timestamp};

/// A clock for tests: wall and monotonic time move together, and only on [`ManualClock::tick`].
///
/// Monotonic time starts at zero; wall time starts at the `start` given to [`ManualClock::new`].
/// A sleeper wakes when a tick passes its instant.
#[derive(Debug)]
pub struct ManualClock {
    start: Timestamp,
    elapsed_nanos: AtomicU64,
    ticked: Notify,
    shutdown: CancellationToken,
}

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        Self::with_shutdown(start, CancellationToken::new())
    }

    pub fn with_shutdown(start: Timestamp, shutdown: CancellationToken) -> Self {
        Self {
            start,
            elapsed_nanos: AtomicU64::new(0),
            ticked: Notify::new(),
            shutdown,
        }
    }

    /// Moves wall and monotonic time forward by `by` (saturating at about 584 years) and wakes the
    /// sleepers whose instant has passed.
    pub fn tick(&self, by: Duration) {
        let by_nanos = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        // The closure always returns Some, so the update cannot fail.
        self.elapsed_nanos
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |elapsed| {
                Some(elapsed.saturating_add(by_nanos))
            })
            .unwrap_or_else(|unchanged| unchanged);
        self.ticked.notify_waiters();
    }

    /// Ends every current and future sleep with `Cancelled`.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.elapsed_nanos.load(Ordering::Acquire))
    }
}

impl Clock for ManualClock {
    fn now_utc(&self) -> Timestamp {
        self.start
            .checked_add(self.elapsed())
            .unwrap_or(Timestamp::from_unix_micros(i64::MAX))
    }

    fn monotonic(&self) -> MonotonicInstant {
        MonotonicInstant::from_origin(self.elapsed())
    }

    fn sleep_until_monotonic(&self, at: MonotonicInstant) -> SleepFuture<'_> {
        Box::pin(async move {
            loop {
                if self.shutdown.is_cancelled() {
                    return Err(Cancelled);
                }
                let ticked = self.ticked.notified();
                tokio::pin!(ticked);
                // Registered before the time check, so a tick between check and await is not missed.
                ticked.as_mut().enable();
                if self.monotonic() >= at {
                    return Ok(());
                }
                tokio::select! {
                    biased;
                    () = self.shutdown.cancelled() => return Err(Cancelled),
                    () = ticked => {}
                }
            }
        })
    }
}
