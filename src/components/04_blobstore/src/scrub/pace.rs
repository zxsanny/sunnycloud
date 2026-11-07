//! The byte-rate cap of a scrub run. Every chunk read is charged here, and the reader sleeps until the instant
//! the charge returns, so over any window the bytes read stay within the rate plus one chunk. Idle time earns no
//! credit: a run that was paused does not catch up with a burst.

use std::time::Duration;

use sc_clock::MonotonicInstant;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

#[derive(Clone)]
pub(super) struct Pacer {
    rate: u64,
    free_at: Option<MonotonicInstant>,
}

impl Pacer {
    /// `rate` is bytes per second and at least 1 (the constructor of the scrubber refuses 0).
    pub fn new(rate: u64) -> Self {
        Pacer {
            rate: rate.max(1),
            free_at: None,
        }
    }

    /// Charges `bytes` read at `now`. Returns the instant the reader has to wait for before it reads again.
    pub fn charge(&mut self, now: MonotonicInstant, bytes: u64) -> MonotonicInstant {
        let start = self.free_at.map_or(now, |free_at| free_at.max(now));
        let nanos = u128::from(bytes) * NANOS_PER_SECOND / u128::from(self.rate);
        let cost = Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
        let free_at = start.checked_add(cost).unwrap_or(start);
        self.free_at = Some(free_at);
        free_at
    }
}

#[cfg(test)]
mod tests {
    use sc_clock::{Clock, ManualClock, Timestamp};

    use super::*;

    const MIB: u64 = 1 << 20;

    fn clock() -> ManualClock {
        ManualClock::new(Timestamp::from_unix_micros(0))
    }

    #[test]
    fn consecutive_charges_are_spaced_by_bytes_over_rate() {
        // Arrange
        let clock = clock();
        let mut pacer = Pacer::new(10 * MIB);
        let now = clock.monotonic();

        // Act
        let first = pacer.charge(now, 5 * MIB);
        let second = pacer.charge(now, 5 * MIB);

        // Assert
        assert_eq!(
            first.saturating_duration_since(now),
            Duration::from_millis(500)
        );
        assert_eq!(
            second.saturating_duration_since(now),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn a_run_that_was_idle_earns_no_credit() {
        // Arrange
        let clock = clock();
        let mut pacer = Pacer::new(10 * MIB);
        let start = clock.monotonic();
        pacer.charge(start, MIB);
        clock.tick(Duration::from_secs(60));
        let later = clock.monotonic();

        // Act
        let free_at = pacer.charge(later, 10 * MIB);

        // Assert
        assert_eq!(
            free_at.saturating_duration_since(later),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn bytes_charged_over_a_window_never_exceed_the_rate() {
        // Arrange
        let clock = clock();
        let mut pacer = Pacer::new(MIB);
        let start = clock.monotonic();
        let chunk = 65_552;
        let mut charged = 0;

        // Act
        let mut free_at = start;
        for _ in 0..100 {
            free_at = pacer.charge(clock.monotonic().max(free_at), chunk);
            charged += chunk;
        }

        // Assert
        let window = free_at.saturating_duration_since(start).as_secs_f64();
        assert!(charged as f64 / window <= MIB as f64 * 1.0001);
    }
}
