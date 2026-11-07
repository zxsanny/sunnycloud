//! AC-5 (IT-05-6 hold cap): the fifth concurrent hold of one principal is refused at once; the four stay parked,
//! and a slot is free again when a hold ends or is cancelled.

use std::time::Duration;

use contracts::ChangeFeedError;
use sunnycloud_sync_feed::PROPOSED_HOLD_RETRY_AFTER;

use crate::hold_support::{Rig, config, cursor, eventually, outcome, principal, settle, valid};

const WAIT: Duration = Duration::from_secs(300);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_fifth_hold_of_a_principal_is_refused_with_the_retry_after_and_the_four_stay_parked() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), 4).await;
    let account = rig.account().await;
    let me = principal(account.id);
    let parked: Vec<_> = (0..4)
        .map(|_| rig.read(&me, valid(), cursor(0), WAIT))
        .collect();
    rig.clock.sleeps_reach(4).await;
    let colleague = principal(account.id);

    // Act
    let fifth = outcome(rig.read(&me, valid(), cursor(0), WAIT)).await;
    let other = rig.read(&colleague, valid(), cursor(0), WAIT);
    rig.clock.sleeps_reach(5).await;
    settle().await;

    // Assert
    assert!(matches!(
        fifth,
        Err(ChangeFeedError::TooManyHolds { retry_after }) if retry_after == PROPOSED_HOLD_RETRY_AFTER
    ));
    assert_eq!(PROPOSED_HOLD_RETRY_AFTER, Duration::from_secs(5));
    assert!(parked.iter().all(|held| !held.is_finished()));
    assert_eq!(rig.feed.holds_of(me.session_or_device_id), 4);
    assert!(!other.is_finished(), "another principal is not limited");
    rig.shutdown.cancel();
    for held in parked.into_iter().chain([other]) {
        outcome(held).await.expect("empty page on shutdown");
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_slots_are_free_again_after_the_holds_return_a_page() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), 4).await;
    let account = rig.account().await;
    let me = principal(account.id);
    let parked: Vec<_> = (0..4)
        .map(|_| rig.read(&me, valid(), cursor(0), WAIT))
        .collect();
    rig.clock.sleeps_reach(4).await;
    let seq = rig.commit_and_notify(&account).await;
    for held in parked {
        outcome(held).await.expect("page");
    }

    // Act
    let again = rig.read(&me, valid(), cursor(seq), WAIT);
    rig.clock.sleeps_reach(5).await;

    // Assert
    assert_eq!(rig.feed.holds_of(me.session_or_device_id), 1);
    rig.shutdown.cancel();
    outcome(again).await.expect("empty page on shutdown");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancelled_hold_releases_its_slot() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), 4).await;
    let account = rig.account().await;
    let me = principal(account.id);
    let mut parked: Vec<_> = (0..4)
        .map(|_| rig.read(&me, valid(), cursor(0), WAIT))
        .collect();
    rig.clock.sleeps_reach(4).await;

    // Act
    let cancelled = parked.pop().expect("a hold");
    cancelled.abort();
    assert!(cancelled.await.expect_err("aborted").is_cancelled());
    eventually("the slot is released", || {
        rig.feed.holds_of(me.session_or_device_id) == 3
    })
    .await;
    let replacement = rig.read(&me, valid(), cursor(0), WAIT);
    rig.clock.sleeps_reach(5).await;

    // Assert
    assert_eq!(rig.feed.holds_of(me.session_or_device_id), 4);
    assert_eq!(rig.notifier.waiters(account.id), 4);
    rig.shutdown.cancel();
    for held in parked.into_iter().chain([replacement]) {
        outcome(held).await.expect("empty page on shutdown");
    }
    rig.finish().await;
}
