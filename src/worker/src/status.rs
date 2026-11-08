//! The status report (09 section 3, step 8): every `worker_status_interval` the supervisor tells the API
//! whether every decoder slot is healthy and how old the decoder image is. `doctor` reads it from the API.

use std::sync::Arc;
use std::time::Duration;

use media_shared::types::WorkerStatus;
use sc_clock::{Clock, Timestamp};
use sc_observability::{log_error, log_warn};
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval, timeout};

use crate::api::{ApiError, WorkerApiClient};
use crate::decoder_client::{DecoderMetrics, SlotName, SlotState};
use crate::slot_loop::ApiAccess;

const SECONDS_PER_DAY: u64 = 86_400;

/// Whole days between a decoder's `build_timestamp` (Unix seconds) and `now`; 0 for a timestamp in the future.
pub(crate) fn image_age_days(build_timestamp: u64, now: Timestamp) -> u32 {
    let built = i64::try_from(build_timestamp)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000))
        .map(Timestamp::from_unix_micros);
    let age = built.and_then(|built| now.checked_duration_since(built));
    age.map_or(0, |age| {
        u32::try_from(age.as_secs() / SECONDS_PER_DAY).unwrap_or(u32::MAX)
    })
}

/// What the reporter reads of one slot.
pub(crate) struct SlotView {
    pub(crate) name: SlotName,
    pub(crate) state: watch::Receiver<SlotState>,
}

pub(crate) struct StatusReporter {
    pub(crate) api: Arc<WorkerApiClient>,
    pub(crate) access: ApiAccess,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) decoders: Arc<DecoderMetrics>,
    pub(crate) slots: Vec<SlotView>,
    pub(crate) every: Duration,
}

impl StatusReporter {
    /// One report at start and one per interval, until the API refuses the worker secret. A failed report is
    /// logged and the next interval sends again; the caller stops the reporter by dropping the future.
    pub(crate) async fn run(self) {
        let mut ticks = interval(self.every);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            if self.access.is_refused() {
                return;
            }
            match timeout(self.every, self.api.report_status(&self.status())).await {
                Ok(Ok(())) => {}
                Ok(Err(ApiError::Forbidden)) => {
                    log_error!("the API refused the worker secret; no more status reports");
                    self.access.refuse();
                    return;
                }
                Ok(Err(error)) => log_warn!("status report failed", err = &error),
                Err(_) => log_warn!("status report timed out"),
            }
        }
    }

    /// `decoder_ok` holds while every slot has passed its gate and is healthy. The image age is that of the
    /// oldest decoder that has said `hello` yet; with none yet it is 0, because the body has no way to say
    /// "unknown" and `decoder_ok = false` already marks the worker.
    pub(crate) fn status(&self) -> WorkerStatus {
        let decoder_ok = self
            .slots
            .iter()
            .all(|slot| slot.state.borrow().decoder_ok());
        let now = self.clock.now_utc();
        let oldest = self
            .slots
            .iter()
            .filter_map(|slot| self.decoders.build_timestamp(slot.name))
            .min();
        WorkerStatus {
            decoder_ok,
            image_age_days: oldest.map_or(0, |built| image_age_days(built, now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILT: u64 = 1_700_000_000;

    fn at(unix_seconds: u64) -> Timestamp {
        Timestamp::from_unix_micros(i64::try_from(unix_seconds * 1_000_000).expect("in range"))
    }

    #[test]
    fn twenty_days_after_the_build_the_image_is_twenty_days_old() {
        // Arrange
        let now = at(BUILT + 20 * SECONDS_PER_DAY);

        // Act
        let age = image_age_days(BUILT, now);

        // Assert
        assert_eq!(age, 20);
    }

    #[test]
    fn a_partial_day_is_not_counted() {
        // Arrange
        let now = at(BUILT + 20 * SECONDS_PER_DAY + SECONDS_PER_DAY - 1);

        // Act
        let age = image_age_days(BUILT, now);

        // Assert
        assert_eq!(age, 20);
    }

    #[test]
    fn an_image_built_in_the_future_is_zero_days_old() {
        // Arrange
        let now = at(BUILT - 5 * SECONDS_PER_DAY);

        // Act
        let age = image_age_days(BUILT, now);

        // Assert
        assert_eq!(age, 0);
    }

    #[test]
    fn the_build_day_itself_is_zero_days_old() {
        // Act
        let age = image_age_days(BUILT, at(BUILT));

        // Assert
        assert_eq!(age, 0);
    }
}
