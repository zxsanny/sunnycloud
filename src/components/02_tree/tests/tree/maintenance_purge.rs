//! AZ-3714 AC-2 (purge restart-safe after a killed run, IT-02-20) and AC-3 (the purge horizon raised as the last
//! statement of each purge transaction, read back through the real 05 reader).

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use contracts::{ChangeFeedError, Cursor, GcQueue, ResyncReason, TreeMaintenance};
use uuid::Uuid;

use crate::maintenance_support::{DyingQueue, Maint, PURGE_AT, config};
use sc_testdb::statements::StatementLog;

/// 100 files with a revision each, deleted one by one through the service; returns their blobs.
async fn hundred_tombstones(maint: &Maint) -> Vec<Uuid> {
    let (tree, account) = (maint.tree(), maint.tree().account);
    let (files, blobs) = maint.files_with_blobs(account, tree.root, 100).await;
    maint.delete_each(account, &files).await;
    blobs
}

#[tokio::test]
async fn a_purge_killed_after_two_batches_is_finished_by_the_next_run_once_per_node() {
    // Arrange
    let maint = Maint::new(config(20, 10)).await;
    let blobs = hundred_tombstones(&maint).await;
    let account = maint.tree().account;
    let queue: Arc<dyn GcQueue> = maint.erase.queue.clone();
    let dying = Arc::new(DyingQueue {
        inner: queue,
        kill_at: 21,
        writes: AtomicU32::new(0),
    });
    let first_run = maint.maintenance_on(maint.tree().fixture.db.api().await, dying);

    // Act
    let first = [
        first_run.purge_tombstones(PURGE_AT, 10).await.ok(),
        first_run.purge_tombstones(PURGE_AT, 10).await.ok(),
    ];
    let cut = first_run.purge_tombstones(PURGE_AT, 10).await;
    drop(first_run);
    let second = maint.purge_until_idle(maint.maintenance.as_ref(), 10).await;

    // Assert
    assert_eq!(first, [Some(10), Some(10)]);
    assert!(cut.is_err(), "the third batch died with its connection");
    assert_eq!(second, [10, 10, 10, 10, 10, 10, 10, 10, 0]);
    let left = maint
        .count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{account}' AND parent_id IS NOT NULL"
        ))
        .await;
    assert_eq!(left, 0, "100 purged in total");
    let revisions = maint.erase.of_account("revisions").await;
    assert_eq!(revisions, 0);
    let queued = maint
        .count(&format!(
            "SELECT count(*) FROM blob_gc g JOIN blobs b ON b.id = g.blob_id \
             WHERE g.blob_id = ANY('{{{}}}'::uuid[]) AND g.reason = 'deleted' AND b.state = 'gc_pending'",
            blobs.iter().map(Uuid::to_string).collect::<Vec<_>>().join(",")
        ))
        .await;
    assert_eq!(queued, 100, "every purged revision's blob is queued");
    maint.finish().await;
}

#[tokio::test]
async fn the_purge_horizon_is_the_highest_purged_seq_raised_last_and_read_by_the_feed() {
    // Arrange
    let maint = Maint::new(config(20, 10)).await;
    hundred_tombstones(&maint).await;
    let (tree, account) = (maint.tree(), maint.tree().account);
    let highest = tree.seq().await;
    let reader = maint.reader();
    let log = StatementLog::install();

    // Act
    let purged = maint.purge_until_idle(maint.maintenance.as_ref(), 10).await;
    let statements = log.take();
    let below = reader
        .read_page(
            account,
            Cursor {
                epoch: 1,
                seq: highest - 1,
            },
            None,
        )
        .await;
    let at = reader
        .read_page(
            account,
            Cursor {
                epoch: 1,
                seq: highest,
            },
            None,
        )
        .await;

    // Assert
    assert_eq!(purged.iter().sum::<u64>(), 100);
    assert_eq!(maint.purge_horizon(account).await, highest);
    let transactions: Vec<&[String]> = statements
        .split(|statement| statement.trim() == "COMMIT")
        .filter(|tx| tx.iter().any(|s| s.contains("DELETE FROM nodes")))
        .collect();
    assert_eq!(transactions.len(), 10, "one purge transaction per batch");
    for tx in transactions {
        let last = tx.last().expect("a statement before COMMIT");
        assert!(last.contains("purge_horizon"), "last statement: {last}");
    }
    assert!(
        matches!(
            below,
            Err(ChangeFeedError::ResyncRequired {
                reason: ResyncReason::BelowPurgeHorizon
            })
        ),
        "{below:?}"
    );
    let page = at.expect("a cursor at the horizon reads a normal page");
    assert!(page.items.is_empty(), "the purged nodes left the feed");
    drop(reader);
    maint.finish().await;
}
