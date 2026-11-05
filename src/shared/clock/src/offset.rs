use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::system::system_now_utc;
use crate::{
    CancellationToken, Clock, ClockError, MonotonicInstant, SleepFuture, SystemClock, Timestamp,
};

/// The `-test` image clock: [`SystemClock`] plus an offset on wall time only.
///
/// The offset starts at zero, is never negative and only grows. `monotonic` and sleeps are never offset,
/// so an advance cannot wake a sleeper early. Compiled only with the `test-clock` feature.
#[derive(Debug)]
pub struct OffsetClock {
    system: SystemClock,
    offset_nanos: AtomicU64,
}

impl OffsetClock {
    pub fn new() -> Self {
        Self::with_shutdown(CancellationToken::new())
    }

    pub fn with_shutdown(shutdown: CancellationToken) -> Self {
        Self {
            system: SystemClock::with_shutdown(shutdown),
            offset_nanos: AtomicU64::new(0),
        }
    }

    /// Ends every current and future sleep with `Cancelled`.
    pub fn shutdown(&self) {
        self.system.shutdown();
    }

    /// Adds `by` to the offset and returns the new offset.
    pub fn advance(&self, by: Duration) -> Result<Duration, ClockError> {
        let by_nanos = u64::try_from(by.as_nanos()).map_err(|_| ClockError::OffsetOverflow)?;
        let before = self
            .offset_nanos
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(by_nanos)
            })
            .map_err(|_| ClockError::OffsetOverflow)?;
        Ok(Duration::from_nanos(before + by_nanos))
    }

    /// Wire-shape variant of [`OffsetClock::advance`] for `{"advance_seconds": N}`: a negative `N` is
    /// refused with [`ClockError::NegativeAdvance`] and leaves the offset unchanged.
    pub fn advance_seconds(&self, seconds: i64) -> Result<Duration, ClockError> {
        let seconds = u64::try_from(seconds).map_err(|_| ClockError::NegativeAdvance)?;
        self.advance(Duration::from_secs(seconds))
    }

    /// The current offset.
    pub fn offset(&self) -> Duration {
        Duration::from_nanos(self.offset_nanos.load(Ordering::Acquire))
    }
}

impl Default for OffsetClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for OffsetClock {
    fn now_utc(&self) -> Timestamp {
        let now = system_now_utc();
        now.checked_add(self.offset())
            .unwrap_or(Timestamp::from_unix_micros(i64::MAX))
    }

    fn monotonic(&self) -> MonotonicInstant {
        self.system.monotonic()
    }

    fn sleep_until_monotonic(&self, at: MonotonicInstant) -> SleepFuture<'_> {
        self.system.sleep_until_monotonic(at)
    }
}
