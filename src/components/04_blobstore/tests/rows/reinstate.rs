//! AC-3: reinstate outcomes, and `remove_row_in_tx` removing both rows.

use std::fs;

use contracts::{BlobRegistry as _, Due, GcQueue, GcReason, ReinstateOutcome};
use sc_db::{RunError, Scope};
use sunnycloud_blobstore::blob_path;
use uuid::Uuid;

use crate::support::{Rig, Stop, account, committed_blob};

async fn queue(rig: &Rig, a: Uuid, blob: Uuid) {
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.queue
                .schedule_in_tx(tx, blob, GcReason::Deleted, Due::AfterRetention)
                .await
        })
        .await
        .expect("schedule");
}

async fn reinstate(rig: &Rig, a: Uuid, blob: Uuid) -> ReinstateOutcome {
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.registry.reinstate_in_tx(tx, a, blob).await
        })
        .await
        .expect("reinstate")
}

#[tokio::test]
async fn a_pending_blob_within_retention_is_reinstated_and_leaves_the_queue() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = committed_blob(&rig, a).await;
    rig.put_file(blob);
    queue(&rig, a, blob).await;

    // Act
    let outcome = reinstate(&rig, a, blob).await;

    // Assert
    assert_eq!(outcome, ReinstateOutcome::Reinstated);
    assert_eq!(rig.queue_row(blob).await, None);
    assert_eq!(
        rig.row(blob).await,
        Some(("committed".to_owned(), Some(10)))
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_blob_whose_rows_were_removed_is_beyond_retention_with_or_without_its_file() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let (gone, leftover) = (committed_blob(&rig, a).await, committed_blob(&rig, a).await);
    rig.put_file(leftover);
    for blob in [gone, leftover] {
        queue(&rig, a, blob).await;
        rig.api
            .run(Scope::Account(a), async |tx| {
                rig.registry.remove_row_in_tx(tx, a, blob).await
            })
            .await
            .expect("remove");
    }

    // Act
    let without_file = reinstate(&rig, a, gone).await;
    let with_file = reinstate(&rig, a, leftover).await;

    // Assert
    assert_eq!(without_file, ReinstateOutcome::BeyondRetention);
    assert_eq!(with_file, ReinstateOutcome::BeyondRetention);
    rig.finish().await;
}

#[tokio::test]
async fn a_row_whose_file_is_absent_is_file_missing_and_stays_unchanged() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let committed = committed_blob(&rig, a).await;
    let pending = committed_blob(&rig, a).await;
    queue(&rig, a, pending).await;
    let queued_row = rig.queue_row(pending).await;

    // Act
    let first = reinstate(&rig, a, committed).await;
    let second = reinstate(&rig, a, pending).await;

    // Assert
    assert_eq!(first, ReinstateOutcome::FileMissing);
    assert_eq!(second, ReinstateOutcome::FileMissing);
    assert_eq!(
        rig.row(committed).await,
        Some(("committed".to_owned(), Some(10)))
    );
    assert_eq!(
        rig.row(pending).await,
        Some(("gc_pending".to_owned(), Some(10)))
    );
    assert_eq!(rig.queue_row(pending).await, queued_row);
    rig.finish().await;
}

#[tokio::test]
async fn a_rolled_back_reinstate_keeps_the_queue_row_and_the_pending_state() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = committed_blob(&rig, a).await;
    rig.put_file(blob);
    queue(&rig, a, blob).await;

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            let outcome = rig.registry.reinstate_in_tx(tx, a, blob).await?;
            assert_eq!(outcome, ReinstateOutcome::Reinstated);
            Err::<(), _>(Stop::Rollback)
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(Stop::Rollback))));
    assert!(rig.queue_row(blob).await.is_some());
    assert_eq!(
        rig.row(blob).await,
        Some(("gc_pending".to_owned(), Some(10)))
    );
    rig.finish().await;
}

#[tokio::test]
async fn another_accounts_blob_is_beyond_retention_and_stays_queued() {
    // Arrange
    let rig = Rig::new().await;
    let (a, b) = (account(&rig).await, account(&rig).await);
    let blob = committed_blob(&rig, b).await;
    rig.put_file(blob);
    queue(&rig, b, blob).await;

    // Act
    let outcome = reinstate(&rig, a, blob).await;

    // Assert
    assert_eq!(outcome, ReinstateOutcome::BeyondRetention);
    assert!(rig.queue_row(blob).await.is_some());
    rig.finish().await;
}

#[tokio::test]
async fn an_unfinished_upload_is_not_restorable() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = Uuid::new_v4();
    rig.put_file(blob);
    let mut conn = rig.owner().await;
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'uploading', 'k', 3, 'epoch'::timestamptz)",
    )
    .bind(blob)
    .bind(a)
    .execute(&mut conn)
    .await
    .expect("uploading row");
    sqlx::Connection::close(conn).await.expect("close");

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry.reinstate_in_tx(tx, a, blob).await
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(_))));
    assert_eq!(rig.row(blob).await, Some(("uploading".to_owned(), None)));
    rig.finish().await;
}

#[tokio::test]
async fn a_file_check_failure_other_than_absence_is_an_error_and_touches_no_row() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = committed_blob(&rig, a).await;
    queue(&rig, a, blob).await;
    fs::write(rig.root.join("blobs"), b"not a folder").expect("block the blob folder");
    assert!(!blob_path(&rig.root, blob).exists());

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.registry.reinstate_in_tx(tx, a, blob).await
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(_))));
    assert_eq!(
        rig.row(blob).await,
        Some(("gc_pending".to_owned(), Some(10)))
    );
    assert!(rig.queue_row(blob).await.is_some());
    rig.finish().await;
}

#[tokio::test]
async fn remove_row_deletes_the_blob_and_its_queue_row_and_accepts_an_absent_row() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = committed_blob(&rig, a).await;
    queue(&rig, a, blob).await;

    // Act
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.registry.remove_row_in_tx(tx, a, blob).await?;
            rig.registry.remove_row_in_tx(tx, a, blob).await
        })
        .await
        .expect("remove twice");

    // Assert
    assert_eq!(rig.row(blob).await, None);
    assert_eq!(rig.queue_row(blob).await, None);
    rig.finish().await;
}
