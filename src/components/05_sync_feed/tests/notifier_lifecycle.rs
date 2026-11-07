//! AC-9 (IT-05-15): the notifier keeps no state for accounts without waiters. This binary holds these tests
//! only, so the resident-set reading is not disturbed by other suites' allocations in the process.

use std::fs;
use std::time::{Duration, Instant};

use contracts::FeedNotifier;
use sunnycloud_sync_feed::InProcessNotifier;
use uuid::Uuid;

const NOTIFIES: u128 = 100_000;
const RSS_BUDGET_BYTES: u64 = 5 * 1024 * 1024;
const TIME_BUDGET: Duration = Duration::from_millis(200);

fn resident_bytes() -> u64 {
    let statm = fs::read_to_string("/proc/self/statm").expect("read statm");
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .expect("resident field")
        .parse()
        .expect("resident pages");
    pages * 4096
}

fn notify_distinct_accounts(notifier: &InProcessNotifier) {
    for index in 0..NOTIFIES {
        notifier.notify(Uuid::from_u128(index + 1), 1);
    }
}

#[test]
fn a_hundred_thousand_notifies_without_waiters_leave_no_entry_and_no_memory_growth() {
    // Arrange
    let notifier = InProcessNotifier::new();
    notify_distinct_accounts(&notifier);
    let before = resident_bytes();

    // Act
    notify_distinct_accounts(&notifier);
    let grown = resident_bytes().saturating_sub(before);

    // Assert
    assert_eq!(notifier.waiting_accounts(), 0);
    assert!(grown <= RSS_BUDGET_BYTES, "grew by {grown} bytes");
}

#[tokio::test]
async fn with_waiters_a_notify_wakes_exactly_that_accounts_waiters() {
    // Arrange
    let notifier = InProcessNotifier::new();
    let (target, bystander) = (Uuid::from_u128(1), Uuid::from_u128(u128::MAX));
    let mut waiting = [notifier.subscribe(target), notifier.subscribe(target)];
    let mut other = notifier.subscribe(bystander);

    // Act
    notify_distinct_accounts(&notifier);
    let woken = waiting
        .each_mut()
        .map(|subscription| subscription.notified());
    for wake in woken {
        wake.await;
    }

    // Assert
    assert!(
        tokio::time::timeout(Duration::ZERO, other.notified())
            .await
            .is_err(),
        "an account outside the notified range stays parked"
    );
    assert_eq!(notifier.waiting_accounts(), 2);
    drop((waiting, other));
    assert_eq!(notifier.waiting_accounts(), 0);
}

/// The 200 ms budget of AC-9 is a wall-clock bound; it is measured alone on a quiet host.
#[test]
#[ignore = "wall-clock budget: run alone, `cargo test -- --ignored`, on a host with no other load"]
fn a_hundred_thousand_notifies_without_waiters_take_under_two_hundred_milliseconds() {
    // Arrange
    let notifier = InProcessNotifier::new();

    // Act
    let started = Instant::now();
    notify_distinct_accounts(&notifier);
    let elapsed = started.elapsed();

    // Assert
    assert!(elapsed < TIME_BUDGET, "took {elapsed:?}");
}
