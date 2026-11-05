//! AC-1 (retention without sleeping), AC-6 (2 s settle rule) and ManualClock sleeper behaviour.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_clock::{Cancelled, Clock, ManualClock, Timestamp};

const DAY: Duration = Duration::from_secs(86_400);
const START: Timestamp = Timestamp::from_unix_micros(1_700_000_000_000_000);

/// Retention rule: a tombstone is due once it is older than the retention window.
fn retention_due(clock: &dyn Clock, deleted_at: Timestamp, retention: Duration) -> bool {
    clock
        .now_utc()
        .checked_duration_since(deleted_at)
        .is_some_and(|age| age >= retention)
}

/// Settle rule: a path is settled once the monotonic clock is `window` past its last change.
fn settled(clock: &dyn Clock, last_change: sc_clock::MonotonicInstant, window: Duration) -> bool {
    clock.monotonic().saturating_duration_since(last_change) >= window
}

#[test]
fn ac1_eight_days_flip_retention_without_sleeping() {
    let started = Instant::now();
    let clock = ManualClock::new(START);
    let deleted_at = clock.now_utc();
    let retention = 7 * DAY;

    assert!(!retention_due(&clock, deleted_at, retention));
    clock.tick(8 * DAY);
    assert!(retention_due(&clock, deleted_at, retention));

    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn ac1_retention_boundary_is_exact() {
    let clock = ManualClock::new(START);
    let deleted_at = clock.now_utc();
    let retention = 7 * DAY;

    clock.tick(retention - Duration::from_micros(1));
    assert!(!retention_due(&clock, deleted_at, retention));
    clock.tick(Duration::from_micros(1));
    assert!(retention_due(&clock, deleted_at, retention));
}

#[test]
fn ac6_settle_fires_only_after_two_seconds() {
    let started = Instant::now();
    let clock = ManualClock::new(START);
    let last_change = clock.monotonic();
    let window = Duration::from_secs(2);

    clock.tick(Duration::from_millis(1_900));
    assert!(!settled(&clock, last_change, window));
    clock.tick(Duration::from_millis(200));
    assert!(settled(&clock, last_change, window));

    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn tick_moves_wall_and_monotonic_together_and_nothing_else_does() {
    let clock = ManualClock::new(START);
    let (wall0, mono0) = (clock.now_utc(), clock.monotonic());

    assert_eq!(clock.now_utc(), wall0);
    assert_eq!(clock.monotonic(), mono0);

    clock.tick(Duration::from_secs(3));
    assert_eq!(
        wall0.checked_add(Duration::from_secs(3)),
        Some(clock.now_utc())
    );
    assert_eq!(
        Duration::from_secs(3),
        clock.monotonic().saturating_duration_since(mono0)
    );
}

#[tokio::test]
async fn sleeper_wakes_only_when_a_tick_passes_its_instant() {
    let clock = Arc::new(ManualClock::new(START));
    let at = clock
        .monotonic()
        .checked_add(Duration::from_secs(10))
        .expect("no overflow");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(at).await }
    });

    tokio::task::yield_now().await;
    clock.tick(Duration::from_secs(9));
    tokio::task::yield_now().await;
    assert!(!sleeper.is_finished());

    clock.tick(Duration::from_secs(1));
    assert_eq!(Ok(()), sleeper.await.expect("sleeper task"));
}

#[tokio::test]
async fn sleep_until_a_passed_instant_returns_at_once() {
    let clock = ManualClock::new(START);
    let at = clock.monotonic();
    clock.tick(Duration::from_secs(1));
    assert_eq!(Ok(()), clock.sleep_until_monotonic(at).await);
}

#[tokio::test]
async fn shutdown_cancels_a_manual_sleeper() {
    let clock = Arc::new(ManualClock::new(START));
    let at = clock.monotonic().checked_add(DAY).expect("no overflow");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(at).await }
    });

    tokio::task::yield_now().await;
    clock.shutdown();
    assert_eq!(Err(Cancelled), sleeper.await.expect("sleeper task"));
}
