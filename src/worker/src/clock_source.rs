//! The clock `serve` hands to `run_worker`: the system clock in a release build; in the `-test` build
//! (feature `test-clock`) the offset clock together with the poll that feeds it. A release build contains
//! neither the poll nor `OffsetClock` (compile-time exclusion, 09 section 2).

use std::sync::Arc;

use sc_clock::Clock;
use sc_config::WorkerSettings;

use crate::error::WorkerError;

#[cfg(not(feature = "test-clock"))]
pub(crate) struct ProcessClock(Arc<sc_clock::SystemClock>);

#[cfg(not(feature = "test-clock"))]
impl ProcessClock {
    pub(crate) fn start(_settings: &WorkerSettings, _worker_id: &str) -> Result<Self, WorkerError> {
        Ok(Self(Arc::new(sc_clock::SystemClock::new())))
    }

    pub(crate) fn clock(&self) -> Arc<dyn Clock> {
        self.0.clone()
    }
}

/// Ends the poll with the process clock.
#[cfg(feature = "test-clock")]
pub(crate) struct ProcessClock {
    clock: Arc<sc_clock::OffsetClock>,
    poll: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "test-clock")]
impl ProcessClock {
    /// Must run inside the tokio runtime: the poll is spawned here.
    pub(crate) fn start(settings: &WorkerSettings, worker_id: &str) -> Result<Self, WorkerError> {
        use crate::api::{ApiConfig, WorkerApiClient};
        use crate::clock_poll::{POLL_INTERVAL, follow_api_offset};

        let api = Arc::new(WorkerApiClient::new(ApiConfig::from_settings(
            settings,
            worker_id.to_owned(),
        ))?);
        let clock = Arc::new(sc_clock::OffsetClock::new());
        let poll = tokio::spawn(follow_api_offset(api, Arc::clone(&clock), POLL_INTERVAL));
        Ok(Self { clock, poll })
    }

    pub(crate) fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }
}

#[cfg(feature = "test-clock")]
impl Drop for ProcessClock {
    fn drop(&mut self) {
        self.poll.abort();
    }
}
