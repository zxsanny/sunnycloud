//! The self-abort watchdog (10 section 3). Docker does not restart a container whose
//! healthcheck fails, so a hung runner must end itself: the main loop bumps a counter on every
//! pass, and a separate thread aborts the process when the counter stops moving.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::RunnerError;

const MIN_POLL: Duration = Duration::from_millis(5);
const MAX_POLL: Duration = Duration::from_millis(100);

/// The monotonic counter the main loop bumps. Every loop that can wait (accept, read, the wait
/// for a stopping job) calls [`Heartbeat::beat`] at least once per [`crate::TICK`], which is
/// well inside `decoder_alive_interval`.
#[derive(Debug, Default)]
pub struct Heartbeat {
    counter: AtomicU64,
}

impl Heartbeat {
    pub const fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }

    pub fn beat(&self) {
        #[cfg(feature = "test-freeze")]
        crate::test_freeze::block_if_requested();
        self.counter.fetch_add(1, Ordering::Release);
    }

    pub fn count(&self) -> u64 {
        self.counter.load(Ordering::Acquire)
    }
}

/// An abort must end the process at once, so the runner is made non-dumpable (see
/// [`crate::sys::disable_dumps`]). Call once at start.
pub fn disable_dumps() -> Result<(), RunnerError> {
    crate::sys::disable_dumps().map_err(RunnerError::io("make the process non-dumpable"))
}

/// A running watchdog thread; dropping the handle stops it.
#[derive(Debug)]
pub struct Watchdog {
    stop: Arc<AtomicBool>,
}

impl Watchdog {
    /// Starts the thread. It calls `abort()` once the counter has not moved for `limit`.
    pub fn spawn(heartbeat: Arc<Heartbeat>, limit: Duration) -> Result<Self, RunnerError> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("decoder-watchdog".into())
            .spawn(move || watch(&heartbeat, limit, &thread_stop))
            .map_err(RunnerError::io("spawn watchdog thread"))?;
        Ok(Self { stop })
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn watch(heartbeat: &Heartbeat, limit: Duration, stop: &AtomicBool) {
    let poll = (limit / 20).clamp(MIN_POLL, MAX_POLL);
    let mut seen = heartbeat.count();
    let mut moved_at = Instant::now();
    loop {
        thread::sleep(poll);
        if stop.load(Ordering::Acquire) {
            return;
        }
        let now = heartbeat.count();
        if now != seen {
            seen = now;
            moved_at = Instant::now();
        } else if moved_at.elapsed() >= limit {
            // No log line first: the logger writes to a stdout that the stuck main loop may
            // hold, and the abort must not depend on it. The SIGABRT exit status is the record.
            std::process::abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_beat_moves_the_counter_up() {
        let heartbeat = Heartbeat::new();

        heartbeat.beat();
        heartbeat.beat();

        assert_eq!(heartbeat.count(), 2);
    }

    #[test]
    fn a_moving_counter_is_never_aborted_and_a_dropped_watchdog_stays_quiet() {
        let heartbeat = Arc::new(Heartbeat::new());
        let watchdog =
            Watchdog::spawn(Arc::clone(&heartbeat), Duration::from_millis(300)).expect("spawn");

        let until = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < until {
            heartbeat.beat();
            thread::sleep(Duration::from_millis(50));
        }
        drop(watchdog);
        thread::sleep(Duration::from_millis(600));

        // An abort would have ended this process before the assertion.
        assert!(heartbeat.count() > 10);
    }
}
