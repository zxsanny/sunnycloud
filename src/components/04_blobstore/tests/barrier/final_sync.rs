//! `SyncBarrier::sync_open_groups`, the final group sync of `serve`'s shutdown (AZ-3791): files whose waiters
//! are gone are synced, a running flush is waited for, and a failed final flush is reported.

use std::sync::Arc;
use std::time::Duration;

use contracts::BlobError;

use super::support::{Rig, wait_until};

#[tokio::test(flavor = "multi_thread")]
async fn files_whose_waiters_were_dropped_are_synced_by_the_final_sync() {
    // Arrange: two written files, registered, never awaited (their requests ended with the connections).
    let rig = Rig::new(Duration::ZERO);
    let (first_id, first_file) = rig.scratch.file();
    let (second_id, second_file) = rig.scratch.file();
    let first = rig.write_and_register(first_id, &first_file, 0);
    let second = rig.write_and_register(second_id, &second_file, 0);

    // Act
    let result = rig.barrier.sync_open_groups().await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(rig.hook.flush_count(), 1);
    let records = rig.hook.records();
    let synced: Vec<_> = records.iter().map(|record| record.blob_id).collect();
    assert_eq!(synced, [first_id, second_id]);
    assert!(
        records
            .iter()
            .all(|record| record.stamp > first.stamp.max(second.stamp)),
        "the sync started after both writes completed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_final_sync_waits_for_the_running_flush_then_syncs_the_open_group() {
    // Arrange: one group flushing (held), a later file in the open group.
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.hold();
    let (running_id, running_file) = rig.scratch.file();
    let running = rig.write_and_register(running_id, &running_file, 0);
    let waiter = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.barrier.await_durable(running.token).await })
    };
    wait_until(|| rig.hook.flush_count() == 1).await;
    let (open_id, open_file) = rig.scratch.file();
    rig.write_and_register(open_id, &open_file, 0);
    let final_sync = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.barrier.sync_open_groups().await })
    };

    // Act
    tokio::time::sleep(Duration::from_millis(50)).await;
    let finished_while_held = final_sync.is_finished();
    rig.hook.release();
    let result = final_sync.await.expect("task finishes");

    // Assert
    assert!(
        !finished_while_held,
        "the final sync returned during a flush"
    );
    assert!(result.is_ok(), "{result:?}");
    waiter
        .await
        .expect("task finishes")
        .expect("the running group syncs");
    let flush_of = |id| {
        rig.hook
            .records()
            .iter()
            .find(|record| record.blob_id == id)
            .map(|record| record.flush)
    };
    assert_eq!(rig.hook.flush_count(), 2);
    assert_eq!(flush_of(running_id), Some(1));
    assert_eq!(flush_of(open_id), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn with_nothing_registered_the_final_sync_returns_without_a_flush() {
    // Arrange
    let rig = Rig::new(Duration::ZERO);

    // Act
    let result = rig.barrier.sync_open_groups().await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(rig.hook.flush_count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_final_flush_is_reported_as_io() {
    // Arrange
    let rig = Rig::new(Duration::ZERO);
    rig.hook.fail_next_flushes(1);
    let (id, file) = rig.scratch.file();
    rig.write_and_register(id, &file, 0);

    // Act
    let result = rig.barrier.sync_open_groups().await;

    // Assert
    assert!(matches!(result, Err(BlobError::Io(_))), "{result:?}");
    assert_eq!(rig.hook.flush_count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_final_sync_after_the_first_finds_nothing_open() {
    // Arrange
    let rig = Rig::new(Duration::ZERO);
    let (id, file) = rig.scratch.file();
    rig.write_and_register(id, &file, 0);
    rig.barrier.sync_open_groups().await.expect("first sync");

    // Act
    let again = rig.barrier.sync_open_groups().await;

    // Assert
    assert!(again.is_ok(), "{again:?}");
    assert_eq!(rig.hook.flush_count(), 1);
}
