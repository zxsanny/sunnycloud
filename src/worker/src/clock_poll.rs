//! The test image's offset poll (feature `test-clock`, 09 section 2 "Test clock"): the supervisor follows the
//! API's clock offset so `image_age_days` agrees with the API's wall time. Durations stay monotonic and are
//! never offset (helper 03).

use std::sync::Arc;
use std::time::Duration;

use sc_clock::{ClockError, OffsetClock};
use sc_observability::{log_error, log_warn};
use thiserror::Error;
use tokio::time::{MissedTickBehavior, interval, timeout};

use crate::api::{ApiError, WorkerApiClient};

/// At most one request per second (helper 03); no key sets it.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OffsetError {
    /// The test setup reset the API's offset; the offset clock only grows, so the supervisor needs a restart.
    #[error(
        "the API offset {polled:?} is below the supervisor's {current:?}; the offset only grows"
    )]
    WentBack { polled: Duration, current: Duration },
    #[error(transparent)]
    Clock(#[from] ClockError),
}

/// Moves `clock` to the offset the API reported. An equal offset changes nothing.
pub fn apply_offset(clock: &OffsetClock, polled: Duration) -> Result<(), OffsetError> {
    let current = clock.offset();
    let behind = polled
        .checked_sub(current)
        .ok_or(OffsetError::WentBack { polled, current })?;
    clock.advance(behind)?;
    Ok(())
}

/// Polls every `every` and applies the offset. A failed poll is logged and the next tick asks again; a 403
/// ends the poll (the claim loop reports the refused secret). The caller stops it by dropping the future.
pub async fn follow_api_offset(
    api: Arc<WorkerApiClient>,
    clock: Arc<OffsetClock>,
    every: Duration,
) {
    let mut ticks = interval(every);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // A lasting problem (an offset that went back, an API without the test clock) is logged when it starts
    // or changes, not on every tick.
    let mut last_problem: Option<String> = None;
    loop {
        ticks.tick().await;
        let problem: Option<Box<dyn std::error::Error + Send + Sync>> =
            match timeout(every, api.clock_offset()).await {
                Ok(Ok(polled)) => apply_offset(&clock, polled).err().map(Into::into),
                Ok(Err(ApiError::Forbidden)) => {
                    log_error!("the API refused the worker secret; no more clock offset polls");
                    return;
                }
                Ok(Err(error)) => Some(error.into()),
                Err(elapsed) => Some(elapsed.into()),
            };
        let text = problem.as_ref().map(ToString::to_string);
        if let Some(error) = &problem
            && last_problem != text
        {
            log_warn!("clock offset poll failed", err = &**error);
        }
        last_problem = text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3_600);

    #[test]
    fn a_polled_offset_becomes_the_clock_offset() {
        // Arrange
        let clock = OffsetClock::new();

        // Act
        let applied = apply_offset(&clock, HOUR);

        // Assert
        assert_eq!(applied, Ok(()));
        assert_eq!(clock.offset(), HOUR);
    }

    #[test]
    fn a_larger_offset_adds_only_the_difference() {
        // Arrange
        let clock = OffsetClock::new();
        apply_offset(&clock, HOUR).expect("first offset");

        // Act
        let applied = apply_offset(&clock, HOUR * 73);

        // Assert
        assert_eq!(applied, Ok(()));
        assert_eq!(clock.offset(), HOUR * 73);
    }

    #[test]
    fn the_same_offset_changes_nothing() {
        // Arrange
        let clock = OffsetClock::new();
        apply_offset(&clock, HOUR).expect("first offset");

        // Act
        let applied = apply_offset(&clock, HOUR);

        // Assert
        assert_eq!(applied, Ok(()));
        assert_eq!(clock.offset(), HOUR);
    }

    #[test]
    fn a_lower_offset_is_refused_and_the_clock_keeps_its_offset() {
        // Arrange
        let clock = OffsetClock::new();
        apply_offset(&clock, HOUR).expect("first offset");

        // Act
        let applied = apply_offset(&clock, Duration::ZERO);

        // Assert
        assert_eq!(
            applied,
            Err(OffsetError::WentBack {
                polled: Duration::ZERO,
                current: HOUR
            })
        );
        assert_eq!(clock.offset(), HOUR);
    }
}
