//! AC-4 (IT-05-6 lost wake): a commit that no notify announced, and one made by another process, are found by the
//! safety recheck. `feed_safety_recheck` is 200 ms and the probe clock is ticked to it.

use contracts::Seq;
use sc_observability::NoLabels;
use sqlx::Connection;
use uuid::Uuid;

use crate::hold_support::{Rig, config, cursor, ms, outcome, principal, valid};

const SAFETY_MS: u64 = 200;

/// A change another process makes: the counter and a node stamped by plain SQL as the schema owner, with no call
/// to the notifier.
async fn foreign_commit(rig: &Rig, account: Uuid, node: Uuid) -> Seq {
    let mut conn = rig.fixture.owner_conn().await;
    let seq: Seq = sqlx::query_scalar(
        "WITH bumped AS (UPDATE account_feeds SET seq = seq + 1 WHERE account_id = $1 RETURNING seq), \
         stamped AS (UPDATE nodes SET seq = (SELECT seq FROM bumped) WHERE id = $2 RETURNING id) \
         SELECT seq FROM bumped",
    )
    .bind(account)
    .bind(node)
    .fetch_one(&mut conn)
    .await
    .expect("foreign commit");
    conn.close().await.expect("close owner connection");
    seq
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stamp_without_notify_is_returned_by_the_safety_recheck_and_counted_as_a_missed_wake() {
    // Arrange
    let rig = Rig::new(&config(2_000, SAFETY_MS), 4).await;
    let account = rig.account().await;
    let held = rig.read(&principal(account.id), valid(), cursor(0), ms(30_000));
    rig.clock.sleeps_reach(1).await;
    let seq = rig.commit(&account).await;

    // Act
    rig.clock.tick(ms(SAFETY_MS));
    let page = outcome(held).await.expect("page");

    // Assert
    assert_eq!((page.items.len(), page.next_cursor), (1, cursor(seq)));
    assert_eq!(rig.missed.get(NoLabels), 1);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_commit_by_another_process_is_seen_at_the_next_safety_recheck() {
    // Arrange
    let rig = Rig::new(&config(2_000, SAFETY_MS), 4).await;
    let account = rig.account().await;
    let held = rig.read(&principal(account.id), valid(), cursor(0), ms(30_000));
    rig.clock.sleeps_reach(1).await;
    let node = rig.created_node(&account).await;
    let seq = foreign_commit(&rig, account.id, node).await;

    // Act
    rig.clock.tick(ms(SAFETY_MS));
    let page = outcome(held).await.expect("page");

    // Assert
    assert_eq!((page.items.len(), page.next_cursor), (1, cursor(seq)));
    assert_eq!(page.items[0].node_id, node);
    assert_eq!(rig.missed.get(NoLabels), 1);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_recheck_that_finds_nothing_keeps_the_hold_parked_and_a_notified_commit_is_not_a_missed_wake()
 {
    // Arrange
    let rig = Rig::new(&config(2_000, SAFETY_MS), 4).await;
    let account = rig.account().await;
    let held = rig.read(&principal(account.id), valid(), cursor(0), ms(30_000));
    rig.clock.sleeps_reach(1).await;

    // Act
    rig.clock.tick(ms(SAFETY_MS));
    rig.clock.sleeps_reach(2).await;
    rig.clock.tick(ms(SAFETY_MS));
    rig.clock.sleeps_reach(3).await;
    let still_parked = !held.is_finished();
    let seq = rig.commit_and_notify(&account).await;
    let page = outcome(held).await.expect("page");

    // Assert
    assert!(still_parked);
    assert_eq!(page.next_cursor, cursor(seq));
    assert_eq!(rig.missed.get(NoLabels), 0);
    rig.finish().await;
}
