use std::time::{SystemTime, UNIX_EPOCH};

use tokio::time::Instant;

use crate::{CancellationToken, Cancelled, Clock, MonotonicInstant, SleepFuture, Timestamp};

/// The release clock: OS wall time and the OS monotonic clock.
#[derive(Debug, Clone)]
pub struct SystemClock {
    origin: Instant,
    shutdown: CancellationToken,
}

impl SystemClock {
    /// A clock whose sleeps end only at their instant or on [`SystemClock::shutdown`].
    pub fn new() -> Self {
        Self::with_shutdown(CancellationToken::new())
    }

    /// A clock whose sleeps also end with `Cancelled` when `shutdown` is cancelled.
    pub fn with_shutdown(shutdown: CancellationToken) -> Self {
        Self {
            origin: Instant::now(),
            shutdown,
        }
    }

    /// Ends every current and future sleep with `Cancelled`.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

/// OS wall time at microsecond precision; saturates at the `i64` range, negative before 1970.
pub(crate) fn system_now_utc() -> Timestamp {
    let micros = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_micros()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_micros()).map_or(i64::MIN, |m| -m),
    };
    Timestamp::from_unix_micros(micros)
}

impl Clock for SystemClock {
    fn now_utc(&self) -> Timestamp {
        system_now_utc()
    }

    fn monotonic(&self) -> MonotonicInstant {
        MonotonicInstant::from_origin(self.origin.elapsed())
    }

    fn sleep_until_monotonic(&self, at: MonotonicInstant) -> SleepFuture<'_> {
        Box::pin(async move {
            let Some(deadline) = self.origin.checked_add(at.since_origin()) else {
                self.shutdown.cancelled().await;
                return Err(Cancelled);
            };
            tokio::select! {
                biased;
                () = self.shutdown.cancelled() => Err(Cancelled),
                () = tokio::time::sleep_until(deadline) => Ok(()),
            }
        })
    }
}
