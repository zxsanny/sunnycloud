//! AC-2 (an advance never wakes a sleeper early) and AC-3 (offset rules). Needs the `test-clock` feature,
//! which the crate's self dev-dependency enables for `cargo test`.
#![cfg(feature = "test-clock")]

use std::sync::Arc;
use std::time::Duration;

use sc_clock::{Clock, ClockError, OffsetClock};
use tokio::time::Instant;

const DAY: Duration = Duration::from_secs(86_400);

#[tokio::test(start_paused = true)]
async fn ac2_advance_during_a_ten_second_sleep_does_not_shorten_it() {
    let clock = Arc::new(OffsetClock::new());
    let started = Instant::now();
    let at = clock
        .monotonic()
        .checked_add(Duration::from_secs(10))
        .expect("no overflow");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(at).await }
    });
    tokio::task::yield_now().await;

    let mono_before = clock.monotonic();
    let wall_before = clock.now_utc();
    clock.advance(DAY).expect("advance");
    assert_eq!(mono_before, clock.monotonic());
    assert!(clock.now_utc() >= wall_before.checked_add(DAY).expect("no overflow"));
    tokio::task::yield_now().await;
    assert!(!sleeper.is_finished());

    assert_eq!(Ok(()), sleeper.await.expect("sleeper task"));
    assert_eq!(Duration::from_secs(10), started.elapsed());
}

#[tokio::test]
async fn ac2_advance_in_real_time_does_not_wake_the_sleeper_early() {
    let clock = Arc::new(OffsetClock::new());
    let started = std::time::Instant::now();
    let at = clock
        .monotonic()
        .checked_add(Duration::from_millis(300))
        .expect("no overflow");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(at).await }
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    clock.advance(DAY).expect("advance");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!sleeper.is_finished());

    assert_eq!(Ok(()), sleeper.await.expect("sleeper task"));
    assert!(started.elapsed() >= Duration::from_millis(300));
}

#[test]
fn ac3_positive_advance_returns_the_new_offset_and_negative_is_refused() {
    let clock = OffsetClock::new();
    assert_eq!(Duration::ZERO, clock.offset());

    assert_eq!(Ok(Duration::from_secs(5)), clock.advance_seconds(5));
    assert_eq!(Duration::from_secs(5), clock.offset());

    assert_eq!(Err(ClockError::NegativeAdvance), clock.advance_seconds(-1));
    assert_eq!(Duration::from_secs(5), clock.offset());
}

#[test]
fn advances_accumulate_and_shift_wall_time_only() {
    let clock = OffsetClock::new();
    assert_eq!(
        Ok(Duration::from_secs(1)),
        clock.advance(Duration::from_secs(1))
    );
    assert_eq!(
        Ok(Duration::from_secs(3)),
        clock.advance(Duration::from_secs(2))
    );

    let shifted = clock.now_utc();
    let unshifted = sc_clock::SystemClock::new().now_utc();
    let ahead = shifted
        .checked_duration_since(unshifted)
        .expect("offset clock is ahead");
    assert!(ahead >= Duration::from_secs(3) - Duration::from_millis(100));
    assert!(ahead < Duration::from_secs(4));
}

#[test]
fn advance_overflow_is_refused_and_keeps_the_offset() {
    let clock = OffsetClock::new();
    clock.advance(Duration::from_secs(10)).expect("advance");
    assert_eq!(
        Err(ClockError::OffsetOverflow),
        clock.advance(Duration::MAX)
    );
    assert_eq!(
        Err(ClockError::OffsetOverflow),
        clock.advance(Duration::from_nanos(u64::MAX))
    );
    assert_eq!(Duration::from_secs(10), clock.offset());
}

#[tokio::test]
async fn shutdown_cancels_an_offset_sleeper() {
    let clock = OffsetClock::new();
    clock.shutdown();
    let at = clock.monotonic().checked_add(DAY).expect("no overflow");
    assert_eq!(
        Err(sc_clock::Cancelled),
        clock.sleep_until_monotonic(at).await
    );
}
