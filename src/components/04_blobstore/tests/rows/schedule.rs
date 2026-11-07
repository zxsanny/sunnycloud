//! AC-4: scheduling writes the state and the queue row together, or neither.

use std::time::Duration;

use contracts::{Due, GcQueue, GcReason};
use sc_db::{RunError, Scope};
use sunnycloud_blobstore::GcQueue as Queue;
use uuid::Uuid;

use crate::support::{DAY, Rig, START, Stop, account, committed_blob, config};

const START_MICROS: i64 = START.as_unix_micros();
const DAY_MICROS: i64 = 86_400_000_000;

async fn schedule(rig: &Rig, a: Uuid, blob: Uuid, reason: GcReason, due: Due) {
    rig.api
        .run(Scope::Account(a), async |tx| {
            rig.queue.schedule_in_tx(tx, blob, reason, due).await
        })
        .await
        .expect("schedule");
}

#[tokio::test]
async fn after_retention_is_now_plus_seven_days_and_marks_the_blob_pending() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = committed_blob(&rig, a).await;
    rig.clock.tick(Duration::from_secs(3600));

    // Act
    schedule(&rig, a, blob, GcReason::Replaced, Due::AfterRetention).await;

    // Assert
    assert_eq!(
        rig.queue_row(blob).await,
        Some((
            START_MICROS + 3_600_000_000 + 7 * DAY_MICROS,
            "replaced".to_owned()
        ))
    );
    assert_eq!(
        rig.row(blob).await,
        Some(("gc_pending".to_owned(), Some(10)))
    );
    rig.finish().await;
}

#[tokio::test]
async fn zero_retention_and_due_now_are_due_at_the_current_time() {
    // Arrange
    let zero = Rig::with_retention(Duration::ZERO).await;
    let seven = Rig::new().await;
    let (a0, a7) = (account(&zero).await, account(&seven).await);
    let (blob0, blob7) = (
        committed_blob(&zero, a0).await,
        committed_blob(&seven, a7).await,
    );
    zero.clock.tick(DAY);
    seven.clock.tick(DAY);

    // Act
    schedule(&zero, a0, blob0, GcReason::Deleted, Due::AfterRetention).await;
    schedule(&seven, a7, blob7, GcReason::AccountDeleted, Due::Now).await;

    // Assert
    let now = START_MICROS + DAY_MICROS;
    assert_eq!(
        zero.queue_row(blob0).await,
        Some((now, "deleted".to_owned()))
    );
    assert_eq!(
        seven.queue_row(blob7).await,
        Some((now, "account_deleted".to_owned()))
    );
    zero.finish().await;
    seven.finish().await;
}

#[tokio::test]
async fn caller_rollback_leaves_neither_the_state_nor_the_queue_row() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let blob = committed_blob(&rig, a).await;

    // Act
    let result = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.queue
                .schedule_in_tx(tx, blob, GcReason::Deleted, Due::Now)
                .await?;
            Err::<(), _>(Stop::Rollback)
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(Stop::Rollback))));
    assert_eq!(rig.queue_row(blob).await, None);
    assert_eq!(
        rig.row(blob).await,
        Some(("committed".to_owned(), Some(10)))
    );
    rig.finish().await;
}

#[tokio::test]
async fn scheduling_twice_keeps_one_row_with_the_earliest_due_time() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let (later_first, now_first) = (committed_blob(&rig, a).await, committed_blob(&rig, a).await);

    // Act
    schedule(
        &rig,
        a,
        later_first,
        GcReason::Replaced,
        Due::AfterRetention,
    )
    .await;
    schedule(&rig, a, later_first, GcReason::Deleted, Due::Now).await;
    schedule(&rig, a, now_first, GcReason::Deleted, Due::Now).await;
    schedule(&rig, a, now_first, GcReason::Replaced, Due::AfterRetention).await;

    // Assert
    assert_eq!(
        rig.queue_row(later_first).await,
        Some((START_MICROS, "deleted".to_owned()))
    );
    assert_eq!(
        rig.queue_row(now_first).await,
        Some((START_MICROS, "deleted".to_owned()))
    );
    assert_eq!(rig.queue_rows().await, 2);
    rig.finish().await;
}

#[tokio::test]
async fn every_reason_is_stored_under_its_data_model_name() {
    // Arrange
    let rig = Rig::new().await;
    let a = account(&rig).await;
    let reasons = [
        (GcReason::Replaced, "replaced"),
        (GcReason::Deleted, "deleted"),
        (GcReason::AccountDeleted, "account_deleted"),
        (GcReason::DerivedRemoved, "derived_removed"),
    ];

    for (reason, text) in reasons {
        let blob = committed_blob(&rig, a).await;

        // Act
        schedule(&rig, a, blob, reason, Due::Now).await;

        // Assert
        assert_eq!(
            rig.queue_row(blob).await.map(|row| row.1),
            Some(text.to_owned())
        );
    }
    rig.finish().await;
}

#[tokio::test]
async fn an_unknown_or_foreign_blob_is_refused_and_writes_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let (a, b) = (account(&rig).await, account(&rig).await);
    let foreign = committed_blob(&rig, b).await;

    // Act
    let unknown = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.queue
                .schedule_in_tx(tx, Uuid::new_v4(), GcReason::Deleted, Due::Now)
                .await
        })
        .await;
    let other = rig
        .api
        .run(Scope::Account(a), async |tx| {
            rig.queue
                .schedule_in_tx(tx, foreign, GcReason::Deleted, Due::Now)
                .await
        })
        .await;

    // Assert
    assert!(matches!(unknown, Err(RunError::Closure(_))));
    assert!(matches!(other, Err(RunError::Closure(_))));
    assert_eq!(rig.queue_rows().await, 0);
    assert_eq!(
        rig.row(foreign).await,
        Some(("committed".to_owned(), Some(10)))
    );
    rig.finish().await;
}

#[tokio::test]
async fn a_retention_beyond_the_time_range_is_refused_at_construction() {
    // Arrange
    let rig = Rig::new().await;
    let settings = config(&rig.root, Duration::MAX);

    // Act
    let result = Queue::new(&settings, rig.api.clone(), rig.clock.clone());

    // Assert
    assert!(result.is_err());
    rig.finish().await;
}
