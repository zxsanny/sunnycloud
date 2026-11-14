//! AZ-3715 AC-4 to AC-6 (IT-02-21): typed refusals roll everything back, a successful restore moves usage by
//! the exact delta and the feed by one `seq`, and a restore waits for the account advisory lock.

use std::time::Duration;

use contracts::{Placement, RestoreError};
use sqlx::Connection;

use crate::mutation_support::close;
use crate::restore_support::{R1_SIZE, R2_SIZE, Restore, TwoRevisions, hours_ago, micros_after};
use crate::support::START;

/// The key `sc_db` locks for an account (`DbTx::advisory_lock_account`), taken here from a plain connection.
const HOLD_ACCOUNT_LOCK: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('sunnycloud.account:' || $1::text, 0))";

const WAITING_ADVISORY: &str = "SELECT count(*) FROM pg_locks \
    WHERE locktype = 'advisory' AND NOT granted \
    AND database = (SELECT oid FROM pg_database WHERE datname = current_database())";

const POLL: Duration = Duration::from_millis(10);
const POLL_LIMIT: u32 = 500;

/// A deleted file with r1 and r2, ready for a restore of r1.
async fn deleted_file(rig: &Restore) -> TwoRevisions {
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    rig.delete(file.node).await;
    file
}

/// Runs a restore of r1 that must be refused, and returns its result with whether every row stayed as it was.
async fn refused(
    rig: &Restore,
    file: &TwoRevisions,
    at: sc_clock::Timestamp,
) -> (RestoreError, bool) {
    let before = rig.snapshot().await;
    let error = rig
        .by_node(file.node, at)
        .await
        .expect_err("the restore is refused");
    (error, rig.snapshot().await == before)
}

#[tokio::test]
async fn an_instant_beyond_the_retention_is_refused_without_a_change() {
    // Arrange
    let rig = Restore::new().await;
    let file = deleted_file(&rig).await;
    let eight_days_ago = hours_ago(8 * 24);

    // Act
    let (error, unchanged) = refused(&rig, &file, eight_days_ago).await;

    // Assert
    assert!(matches!(error, RestoreError::BeyondRetention), "{error:?}");
    assert!(unchanged);
    rig.finish().await;
}

#[tokio::test]
async fn an_absent_blob_file_is_file_missing_without_a_change() {
    // Arrange
    let rig = Restore::new().await;
    let file = deleted_file(&rig).await;
    rig.drop_file(file.blob1);

    // Act
    let (error, unchanged) = refused(&rig, &file, micros_after(hours_ago(3), 1)).await;

    // Assert
    assert!(matches!(error, RestoreError::FileMissing), "{error:?}");
    assert!(unchanged);
    rig.finish().await;
}

#[tokio::test]
async fn a_reinstate_refusal_for_a_removed_blob_row_rolls_back_everything() {
    // Arrange
    let rig = Restore::new().await;
    let file = deleted_file(&rig).await;
    rig.remove_blob_rows(file.blob1).await;

    // Act
    let (error, unchanged) = refused(&rig, &file, micros_after(hours_ago(3), 1)).await;

    // Assert
    assert!(matches!(error, RestoreError::BeyondRetention), "{error:?}");
    assert!(unchanged);
    rig.finish().await;
}

#[tokio::test]
async fn a_restore_replacing_the_current_revision_moves_usage_exactly_and_the_feed_by_one() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    let (used, _) = rig.tree.quota().await;
    let seq = rig.tree.seq().await;

    // Act
    let outcome = rig
        .by_path("a.txt", micros_after(hours_ago(3), 1))
        .await
        .expect("restore");

    // Assert
    assert_eq!(outcome.placement, Placement::Original);
    assert_eq!(rig.tree.quota().await.0, used + R1_SIZE - R2_SIZE);
    assert_eq!(rig.tree.live_bytes().await, rig.tree.quota().await.0);
    assert_eq!(rig.tree.seq().await, seq + 1);
    let node_seq = rig
        .tree
        .count(&format!("SELECT seq FROM nodes WHERE id = '{}'", file.node))
        .await;
    assert_eq!(node_seq, seq + 1);
    let (reason, _) = rig.tree.gc_row(file.blob2).await.expect("r2 queued");
    assert_eq!(reason, "replaced");
    assert_eq!(rig.tree.gc_row(file.blob1).await, None);
    rig.finish().await;
}

#[tokio::test]
async fn a_restore_of_the_live_current_revision_changes_nothing() {
    // Arrange
    let rig = Restore::new().await;
    let file = rig.two_revisions(rig.tree.root, "a.txt").await;
    let before = rig.snapshot().await;

    // Act
    let outcome = rig.by_node(file.node, START).await.expect("restore");

    // Assert
    assert_eq!(outcome.restored_revision, file.r2);
    assert_eq!(outcome.placement, Placement::Original);
    assert_eq!(rig.snapshot().await, before);
    rig.finish().await;
}

#[tokio::test]
async fn a_restore_waits_while_another_connection_holds_the_account_lock() {
    // Arrange
    let rig = Restore::new().await;
    let file = deleted_file(&rig).await;
    let mut holder = rig.tree.fixture.owner_conn().await;
    let mut held = holder.begin().await.expect("begin the holder");
    sqlx::query(HOLD_ACCOUNT_LOCK)
        .bind(rig.tree.account)
        .execute(&mut *held)
        .await
        .expect("hold the account lock");
    let seq = rig.tree.seq().await;

    // Act
    let (restored, seq_while_blocked) = tokio::join!(
        rig.by_node(file.node, micros_after(hours_ago(3), 1)),
        async {
            let mut waited = false;
            for _ in 0..POLL_LIMIT {
                if rig.tree.count(WAITING_ADVISORY).await == 1 {
                    waited = true;
                    break;
                }
                tokio::time::sleep(POLL).await;
            }
            let seq_while_blocked = rig.tree.seq().await;
            held.commit().await.expect("release the account lock");
            (waited, seq_while_blocked)
        }
    );

    // Assert
    assert_eq!(
        seq_while_blocked,
        (true, seq),
        "the restore waited uncommitted"
    );
    assert_eq!(
        restored
            .expect("restore after the release")
            .restored_revision,
        file.r1
    );
    assert_eq!(rig.tree.seq().await, seq + 1);
    close(holder).await;
    rig.finish().await;
}
