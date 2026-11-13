//! When a session's progress is written back to its `upload_sessions` row: after `checkpoint_bytes`
//! acknowledged bytes or `checkpoint_interval` since the last checkpoint, whichever comes first, one checkpoint
//! at a time per session.

use std::time::Duration;

use sc_clock::{MonotonicInstant, Timestamp};

/// The last successful checkpoint of a session (its start or load before the first one): when it ran and the
/// `expires_at` it wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointMark {
    pub at: MonotonicInstant,
    pub expires_at: Timestamp,
}

/// `checkpoint_bytes` and `checkpoint_interval` of the configuration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckpointPolicy {
    pub bytes: u64,
    pub interval: Duration,
}

/// The checkpoint bookkeeping of one session.
#[derive(Debug)]
pub(crate) struct CheckpointState {
    last: CheckpointMark,
    unsaved_bytes: u64,
    running: bool,
}

impl CheckpointState {
    pub(crate) fn new(last: CheckpointMark) -> Self {
        CheckpointState {
            last,
            unsaved_bytes: 0,
            running: false,
        }
    }

    pub(crate) fn expires_at(&self) -> Timestamp {
        self.last.expires_at
    }

    /// Adds `bytes` acknowledged bytes. When a checkpoint is due and none is running, marks one running and
    /// returns the unsaved bytes it covers; the caller ends it with [`Self::succeeded`] or [`Self::failed`].
    pub(crate) fn record(
        &mut self,
        bytes: u64,
        now: MonotonicInstant,
        policy: CheckpointPolicy,
    ) -> Option<u64> {
        self.unsaved_bytes = self.unsaved_bytes.saturating_add(bytes);
        if self.running || self.unsaved_bytes == 0 {
            return None;
        }
        let due = self.unsaved_bytes >= policy.bytes
            || now.saturating_duration_since(self.last.at) >= policy.interval;
        if !due {
            return None;
        }
        self.running = true;
        Some(std::mem::take(&mut self.unsaved_bytes))
    }

    pub(crate) fn succeeded(&mut self, mark: CheckpointMark) {
        self.last = mark;
        self.running = false;
    }

    /// The bytes of a checkpoint that did not happen count again, so the next block retries it.
    pub(crate) fn failed(&mut self, taken: u64) {
        self.unsaved_bytes = self.unsaved_bytes.saturating_add(taken);
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
    use sc_clock::{Clock, ManualClock};

    use super::*;

    const MIB: u64 = 1 << 20;
    const POLICY: CheckpointPolicy = CheckpointPolicy {
        bytes: 8 * MIB,
        interval: Duration::from_secs(1),
    };

    fn state(clock: &ManualClock) -> CheckpointState {
        CheckpointState::new(CheckpointMark {
            at: clock.monotonic(),
            expires_at: clock.now_utc(),
        })
    }

    #[test]
    fn a_checkpoint_is_due_after_eight_mib() {
        // Arrange
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let mut state = state(&clock);
        let now = clock.monotonic();

        // Act
        let first_three: Vec<_> = (0..3).map(|_| state.record(2 * MIB, now, POLICY)).collect();
        let fourth = state.record(2 * MIB, now, POLICY);

        // Assert
        assert_eq!(first_three, [None, None, None]);
        assert_eq!(fourth, Some(8 * MIB));
    }

    #[test]
    fn a_checkpoint_is_due_after_the_interval() {
        // Arrange
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let mut state = state(&clock);
        let early = state.record(MIB, clock.monotonic(), POLICY);
        clock.tick(Duration::from_secs(1));

        // Act
        let late = state.record(MIB, clock.monotonic(), POLICY);

        // Assert
        assert_eq!((early, late), (None, Some(2 * MIB)));
    }

    #[test]
    fn one_checkpoint_runs_at_a_time_and_a_failed_one_is_retried() {
        // Arrange
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let mut state = state(&clock);
        let now = clock.monotonic();
        let taken = state.record(8 * MIB, now, POLICY).expect("due");

        // Act
        let while_running = state.record(8 * MIB, now, POLICY);
        state.failed(taken);
        let retried = state.record(0, now, POLICY);

        // Assert
        assert_eq!(while_running, None);
        assert_eq!(retried, Some(16 * MIB));
    }

    #[test]
    fn a_successful_checkpoint_moves_the_mark_and_the_expiry() {
        // Arrange
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let mut state = state(&clock);
        state
            .record(8 * MIB, clock.monotonic(), POLICY)
            .expect("due");
        clock.tick(Duration::from_millis(500));
        let mark = CheckpointMark {
            at: clock.monotonic(),
            expires_at: clock.now_utc(),
        };

        // Act
        state.succeeded(mark);
        clock.tick(Duration::from_millis(700));
        let after = state.record(MIB, clock.monotonic(), POLICY);

        // Assert
        assert_eq!(state.expires_at(), mark.expires_at);
        assert_eq!(
            after, None,
            "700 ms since the last checkpoint, 1 MiB unsaved"
        );
    }
}
