//! The activity gauge of the API process: how many block writes and downloads are in flight. The scrub pauses
//! while the count is above `scrub_pause_active_streams`. A process without a gauge (the CLI) is not paused.

use std::sync::Arc;

use tokio::sync::watch;

/// Cloned into the writer, the reader and the scrubber of one process; every clone counts into the same value.
#[derive(Clone, Default)]
pub struct ActivityGauge {
    active: Arc<watch::Sender<u64>>,
}

/// One block write or one open download. The count drops when it is dropped.
#[must_use = "the activity ends when the guard is dropped"]
pub struct ActivityGuard {
    gauge: ActivityGauge,
}

impl ActivityGauge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enter(&self) -> ActivityGuard {
        self.active.send_modify(|count| *count += 1);
        ActivityGuard {
            gauge: self.clone(),
        }
    }

    pub fn active(&self) -> u64 {
        *self.active.borrow()
    }

    /// Returns when the count is at most `limit`. Woken by each change of the count, so no polling interval.
    pub(crate) async fn wait_until_at_most(&self, limit: u64) {
        let mut changes = self.active.subscribe();
        // The sender lives in this gauge, so the channel cannot close while it waits; a closed channel would only
        // end the wait.
        match changes.wait_for(|count| *count <= limit).await {
            Ok(_) | Err(watch::error::RecvError { .. }) => {}
        }
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.gauge.active.send_modify(|count| *count -= 1);
    }
}
