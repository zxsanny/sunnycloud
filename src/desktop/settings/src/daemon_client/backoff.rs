use std::time::Duration;

/// Reconnect delays: `min`, doubling, never above `max`.
#[derive(Debug, Clone)]
pub struct Backoff {
    min: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        Self {
            min,
            max,
            next: min,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = (self.next * 2).min(self.max);
        delay
    }

    pub fn reset(&mut self) {
        self.next = self.min;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon_client::{BACKOFF_MAX, BACKOFF_MIN};

    #[test]
    fn the_production_schedule_starts_at_250_ms_and_stops_growing_at_2_s() {
        let mut backoff = Backoff::new(BACKOFF_MIN, BACKOFF_MAX);

        let delays: Vec<u64> = (0..7)
            .map(|_| backoff.next_delay().as_millis() as u64)
            .collect();

        assert_eq!(delays, [250, 500, 1000, 2000, 2000, 2000, 2000]);
    }

    #[test]
    fn reset_returns_to_the_first_delay() {
        let mut backoff = Backoff::new(BACKOFF_MIN, BACKOFF_MAX);
        backoff.next_delay();
        backoff.next_delay();

        backoff.reset();

        assert_eq!(backoff.next_delay(), BACKOFF_MIN);
    }

    #[test]
    fn a_delay_is_never_above_the_maximum_even_when_doubling_overshoots() {
        let mut backoff = Backoff::new(Duration::from_millis(300), Duration::from_millis(500));

        let delays: Vec<u64> = (0..3)
            .map(|_| backoff.next_delay().as_millis() as u64)
            .collect();

        assert_eq!(delays, [300, 500, 500]);
    }
}
