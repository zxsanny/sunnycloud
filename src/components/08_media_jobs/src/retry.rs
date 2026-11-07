//! The pure rules of a failure report and of the clock arithmetic of the queue.

use std::time::Duration;

use media_shared::types::Disposition;
use sc_clock::Timestamp;

/// `jobs.last_error` holds at most this many characters (`jobs_last_error_check`).
pub(crate) const LAST_ERROR_MAX_CHARS: usize = 500;

/// Latest instant the queue stores: a delay that does not fit parks the job there instead of failing the call.
const FAR_FUTURE: Timestamp = Timestamp::from_unix_micros(i64::MAX);

pub(crate) fn add(now: Timestamp, by: Duration) -> Timestamp {
    now.checked_add(by).unwrap_or(FAR_FUTURE)
}

/// The base delay doubled for every attempt after the first: attempt 1 waits `base`, attempt 2 `2 * base`.
pub(crate) fn retry_delay(base: Duration, attempts: u32) -> Duration {
    let doublings = attempts.saturating_sub(1).min(31);
    base.saturating_mul(1 << doublings)
}

/// A job dies on a permanent failure, or when the attempt that just failed was the last one allowed.
pub(crate) fn is_dead(disposition: Disposition, attempts: i64, max_attempts: i64) -> bool {
    disposition == Disposition::Permanent || attempts >= max_attempts
}

/// The worker's short code, cut to the column limit by characters. NUL cannot be stored in text, so it is dropped.
pub(crate) fn bounded_error(code: &str) -> String {
    code.chars()
        .filter(|character| *character != '\0')
        .take(LAST_ERROR_MAX_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Duration = Duration::from_millis(200);

    #[test]
    fn the_delay_doubles_with_every_attempt() {
        // Arrange
        let attempts = [1, 2, 3, 4];

        // Act
        let delays: Vec<Duration> = attempts.map(|n| retry_delay(BASE, n)).to_vec();

        // Assert
        let expected: Vec<Duration> = [200, 400, 800, 1600].map(Duration::from_millis).to_vec();
        assert_eq!(delays, expected);
    }

    #[test]
    fn a_huge_attempt_count_or_base_saturates_instead_of_overflowing() {
        // Arrange
        let huge = Duration::MAX;

        // Act
        let by_attempts = retry_delay(BASE, u32::MAX);
        let by_base = retry_delay(huge, 5);
        let instant = add(Timestamp::from_unix_micros(1), huge);

        // Assert
        assert!(by_attempts >= BASE);
        assert_eq!(by_base, Duration::MAX);
        assert_eq!(instant, FAR_FUTURE);
    }

    #[test]
    fn a_job_dies_when_the_failure_is_permanent_or_the_last_attempt_failed() {
        // Arrange
        let cases = [
            (Disposition::Retry, 1, 3, false),
            (Disposition::Retry, 2, 3, false),
            (Disposition::Retry, 3, 3, true),
            (Disposition::Retry, 1, 1, true),
            (Disposition::Permanent, 1, 3, true),
        ];

        // Act and Assert
        for (disposition, attempts, max, dead) in cases {
            assert_eq!(
                is_dead(disposition, attempts, max),
                dead,
                "{disposition:?} {attempts}/{max}"
            );
        }
    }

    #[test]
    fn the_error_is_cut_by_characters_and_loses_nul() {
        // Arrange
        let long = "é".repeat(2_000);
        let with_nul = "a\0b";

        // Act
        let cut = bounded_error(&long);
        let cleaned = bounded_error(with_nul);

        // Assert
        assert_eq!(cut.chars().count(), LAST_ERROR_MAX_CHARS);
        assert_eq!(cleaned, "ab");
        assert_eq!(bounded_error("decode_failed"), "decode_failed");
    }
}
