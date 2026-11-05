//! AC-7: shutdown ends a long sleep promptly. Runs on the real runtime clock.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_clock::{CancellationToken, Cancelled, Clock, SystemClock};

#[tokio::test]
async fn ac7_shutdown_cancels_an_hour_long_sleep_within_100_ms() {
    let token = CancellationToken::new();
    let clock = Arc::new(SystemClock::with_shutdown(token.clone()));
    let at = clock
        .monotonic()
        .checked_add(Duration::from_secs(3_600))
        .expect("no overflow");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(at).await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!sleeper.is_finished());

    let signalled = Instant::now();
    token.cancel();
    let result = tokio::time::timeout(Duration::from_millis(100), sleeper)
        .await
        .expect("the sleep returned within 100 ms")
        .expect("sleeper task");

    assert_eq!(Err(Cancelled), result);
    assert!(signalled.elapsed() < Duration::from_millis(100));
}

#[tokio::test]
async fn own_shutdown_method_cancels_the_sleep() {
    let clock = Arc::new(SystemClock::new());
    let at = clock
        .monotonic()
        .checked_add(Duration::from_secs(3_600))
        .expect("no overflow");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(at).await }
    });
    tokio::task::yield_now().await;

    clock.shutdown();
    assert_eq!(Err(Cancelled), sleeper.await.expect("sleeper task"));
}

#[tokio::test(start_paused = true)]
async fn a_system_sleep_ends_at_its_instant_and_a_passed_instant_returns_at_once() {
    let clock = SystemClock::new();
    let started = tokio::time::Instant::now();
    let at = clock
        .monotonic()
        .checked_add(Duration::from_secs(5))
        .expect("no overflow");

    assert_eq!(Ok(()), clock.sleep_until_monotonic(at).await);
    assert_eq!(Duration::from_secs(5), started.elapsed());
    assert_eq!(Ok(()), clock.sleep_until_monotonic(at).await);
    assert_eq!(Duration::from_secs(5), started.elapsed());
}

#[tokio::test]
async fn an_unrepresentable_instant_waits_for_shutdown() {
    let clock = Arc::new(SystemClock::new());
    let far = clock
        .monotonic()
        .checked_add(Duration::from_secs(u64::MAX / 4))
        .expect("fits a monotonic instant");
    let sleeper = tokio::spawn({
        let clock = Arc::clone(&clock);
        async move { clock.sleep_until_monotonic(far).await }
    });
    tokio::task::yield_now().await;
    clock.shutdown();
    assert_eq!(Err(Cancelled), sleeper.await.expect("sleeper task"));
}
